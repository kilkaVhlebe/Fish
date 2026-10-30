//! Game rules (Fabric's game rule API): rules a mod adds to `/gamerule`, with a category, a type
//! and a default, whose values every world keeps and saves.
//!
//! Register while `pre_launch` or `main` runs (the same freeze as [`registry`](crate::registry)):
//!
//! ```no_run
//! use fish_api::game_rule::{Category, GameRules};
//!
//! # fn init() -> Result<(), fish_api::Error> {
//! let sparkles = GameRules::register_bool("mymod:sparkles", Category::Misc, true)?;
//! let limit = GameRules::register_int("mymod:limit", Category::Mobs, 8, 0, 64)?;
//! GameRules::on_change("mymod:limit", |change| {
//!     log::info!("limit {:?} -> {:?}", change.old, change.new);
//! })?;
//! // later, inside any event listener of a game with a world:
//! if sparkles.get()? && limit.get()? > 4 {
//!     limit.set(4)?;
//! }
//! # Ok(()) }
//! ```
//!
//! `get` and `set` act on the world of the game whose event is running. Outside of an event there
//! is no world: `get` answers the default and `set` fails with `UNSUPPORTED`. Players change the
//! values with `/gamerule <name> [<value>]`; the game saves them in the world.

use std::ffi::c_void;

use fish_abi::client::StrSink;
use fish_abi::gamerules::{
    CHANGED, GameRuleChangedPayload, GameRuleServiceV1, RuleDefV1, RuleValueV1, SERVICE_GAME_RULE,
    kind,
};
use fish_abi::{FishStatus, FishStr};

use crate::client::service;
use crate::error::{Error, check};
use crate::event::{Combine, Event, Payload};
use crate::host;

/// The category a rule is listed under (the game groups the rule screen by it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Rules about players.
    Player,
    /// Rules about mobs.
    Mobs,
    /// Rules about spawning.
    Spawning,
    /// Rules about drops.
    Drops,
    /// Rules about block updates.
    Updates,
    /// Rules about chat.
    Chat,
    /// Everything else.
    Misc,
}

impl Category {
    /// The category's id on the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            Category::Player => "player",
            Category::Mobs => "mobs",
            Category::Spawning => "spawning",
            Category::Drops => "drops",
            Category::Updates => "updates",
            Category::Chat => "chat",
            Category::Misc => "misc",
        }
    }
}

/// The value of a rule.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A boolean rule.
    Bool(bool),
    /// An integer rule.
    Int(i32),
    /// A decimal rule.
    Double(f64),
    /// An enum rule: one of its choices.
    Choice(String),
}

/// What [`GameRules::on_change`] listeners receive.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// The rule.
    pub name: String,
    /// The world (its directory); empty when unknown.
    pub world: String,
    /// The value before.
    pub old: Value,
    /// The value now.
    pub new: Value,
}

/// The payload of [`CHANGED`].
#[repr(transparent)]
pub struct ChangeContext(GameRuleChangedPayload);

// SAFETY: `repr(transparent)` over an ABI payload that starts with `size: u32`.
unsafe impl Payload for ChangeContext {}

/// Reads a value the host put in the ABI; text is read from `text`.
fn read(v: &RuleValueV1, text: String) -> Value {
    match v.kind {
        kind::BOOL => Value::Bool(v.boolean != 0),
        kind::INT => Value::Int(v.int),
        kind::DOUBLE => Value::Double(v.double),
        _ => Value::Choice(text),
    }
}

impl ChangeContext {
    /// The change as owned data.
    pub fn change(&self) -> Change {
        let p = &self.0;
        // SAFETY: the host passes strings valid for the callback.
        let (name, world, old, new) = unsafe {
            (
                p.name.as_str().unwrap_or_default().to_string(),
                p.world.as_str().unwrap_or_default().to_string(),
                p.old.text.as_str().unwrap_or_default().to_string(),
                p.new_value.text.as_str().unwrap_or_default().to_string(),
            )
        };
        Change {
            name,
            world,
            old: read(&p.old, old),
            new: read(&p.new_value, new),
        }
    }
}

/// Fired after a rule's value changed in some world (by `/gamerule`, the rule screen or a mod).
pub static CHANGED_EVENT: Event<ChangeContext> = Event::new(CHANGED, Combine::Notify);

unsafe extern "C" fn collect(user: *mut c_void, text: FishStr) {
    // SAFETY: `user` is the `String` `get` passes; the host string is valid UTF-8 or skipped.
    unsafe {
        if let Ok(s) = text.as_str() {
            (*user.cast::<String>()).push_str(s);
        }
    }
}

fn abi_value(v: &Value) -> RuleValueV1 {
    match v {
        Value::Bool(b) => RuleValueV1::boolean(*b),
        Value::Int(i) => RuleValueV1::int(*i),
        Value::Double(d) => RuleValueV1::double(*d),
        Value::Choice(c) => RuleValueV1::choice(c),
    }
}

fn register_def(name: &str, category: Category, def: &RuleDefV1) -> Result<(), Error> {
    let (svc, host_ptr) = service::<GameRuleServiceV1>(SERVICE_GAME_RULE)?;
    let table = host::get()?;
    // SAFETY: valid service table; the strings and the definition outlive the call.
    let status = unsafe {
        (svc.register)(
            host_ptr,
            table.mod_token,
            FishStr::new(name),
            FishStr::new(category.as_str()),
            def,
        )
    };
    if status == FishStatus::INVALID_ARGUMENT && is_frozen()? {
        return Err(Error::InvalidArgument("registries are frozen".to_string()));
    }
    check("registering a game rule", status)
}

fn is_frozen() -> Result<bool, Error> {
    crate::registry::is_frozen()
}

fn definition(kind: u8, default_value: RuleValueV1) -> RuleDefV1 {
    RuleDefV1 {
        size: size_of::<RuleDefV1>() as u32,
        kind,
        reserved: [0; 3],
        default_value,
        min_int: 0,
        max_int: 0,
        min_double: 0.0,
        max_double: 0.0,
        choices: std::ptr::null(),
        choice_count: 0,
    }
}

/// A boolean rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoolRule(String);

/// An integer rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntRule(String);

/// A decimal rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoubleRule(String);

/// A rule with named choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumRule(String);

fn unexpected(name: &str, v: Value) -> Error {
    Error::InvalidArgument(format!(
        "game rule {name} holds {v:?}, not the expected type"
    ))
}

impl BoolRule {
    /// The rule's id.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// The value in the current world (the default outside of one).
    pub fn get(&self) -> Result<bool, Error> {
        match GameRules::get(&self.0)? {
            Value::Bool(b) => Ok(b),
            other => Err(unexpected(&self.0, other)),
        }
    }

    /// Sets the value in the current world.
    pub fn set(&self, value: bool) -> Result<(), Error> {
        GameRules::set(&self.0, Value::Bool(value))
    }
}

impl IntRule {
    /// The rule's id.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// The value in the current world (the default outside of one).
    pub fn get(&self) -> Result<i32, Error> {
        match GameRules::get(&self.0)? {
            Value::Int(i) => Ok(i),
            other => Err(unexpected(&self.0, other)),
        }
    }

    /// Sets the value in the current world; it must lie within the rule's bounds.
    pub fn set(&self, value: i32) -> Result<(), Error> {
        GameRules::set(&self.0, Value::Int(value))
    }
}

impl DoubleRule {
    /// The rule's id.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// The value in the current world (the default outside of one).
    pub fn get(&self) -> Result<f64, Error> {
        match GameRules::get(&self.0)? {
            Value::Double(d) => Ok(d),
            other => Err(unexpected(&self.0, other)),
        }
    }

    /// Sets the value in the current world; it must lie within the rule's bounds.
    pub fn set(&self, value: f64) -> Result<(), Error> {
        GameRules::set(&self.0, Value::Double(value))
    }
}

impl EnumRule {
    /// The rule's id.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// The choice in the current world (the default outside of one).
    pub fn get(&self) -> Result<String, Error> {
        match GameRules::get(&self.0)? {
            Value::Choice(c) => Ok(c),
            other => Err(unexpected(&self.0, other)),
        }
    }

    /// Sets the choice in the current world; it must be one of the rule's choices.
    pub fn set(&self, choice: &str) -> Result<(), Error> {
        GameRules::set(&self.0, Value::Choice(choice.to_string()))
    }
}

#[cfg(test)]
impl BoolRule {
    pub(crate) fn from_name(name: &str) -> BoolRule {
        BoolRule(name.to_string())
    }
}

#[cfg(test)]
impl IntRule {
    pub(crate) fn from_name(name: &str) -> IntRule {
        IntRule(name.to_string())
    }
}

#[cfg(test)]
impl DoubleRule {
    pub(crate) fn from_name(name: &str) -> DoubleRule {
        DoubleRule(name.to_string())
    }
}

#[cfg(test)]
impl EnumRule {
    pub(crate) fn from_name(name: &str) -> EnumRule {
        EnumRule(name.to_string())
    }
}

/// Registers, reads and writes game rules.
pub struct GameRules;

impl GameRules {
    /// Registers a boolean rule named `name` (`namespace:path`, not in `minecraft`).
    pub fn register_bool(name: &str, category: Category, default: bool) -> Result<BoolRule, Error> {
        let def = definition(kind::BOOL, RuleValueV1::boolean(default));
        register_def(name, category, &def)?;
        Ok(BoolRule(name.to_string()))
    }

    /// Registers an integer rule with the bounds `min..=max`.
    pub fn register_int(
        name: &str,
        category: Category,
        default: i32,
        min: i32,
        max: i32,
    ) -> Result<IntRule, Error> {
        let mut def = definition(kind::INT, RuleValueV1::int(default));
        def.min_int = min;
        def.max_int = max;
        register_def(name, category, &def)?;
        Ok(IntRule(name.to_string()))
    }

    /// Registers a decimal rule with the bounds `min..=max`.
    pub fn register_double(
        name: &str,
        category: Category,
        default: f64,
        min: f64,
        max: f64,
    ) -> Result<DoubleRule, Error> {
        let mut def = definition(kind::DOUBLE, RuleValueV1::double(default));
        def.min_double = min;
        def.max_double = max;
        register_def(name, category, &def)?;
        Ok(DoubleRule(name.to_string()))
    }

    /// Registers a rule whose value is one of `choices` (letters, digits, `_`, `-` and `.`).
    pub fn register_enum(
        name: &str,
        category: Category,
        default: &str,
        choices: &[&str],
    ) -> Result<EnumRule, Error> {
        let wire: Vec<FishStr> = choices.iter().map(|c| FishStr::new(c)).collect();
        let mut def = definition(kind::ENUM, RuleValueV1::choice(default));
        def.choices = wire.as_ptr();
        def.choice_count = wire.len();
        register_def(name, category, &def)?;
        Ok(EnumRule(name.to_string()))
    }

    /// The value of rule `name` (any mod's) in the current world; the default outside of one.
    pub fn get(name: &str) -> Result<Value, Error> {
        let (svc, host_ptr) = service::<GameRuleServiceV1>(SERVICE_GAME_RULE)?;
        let mut out = RuleValueV1::int(0);
        let mut text = String::new();
        // SAFETY: valid service table; `out`, `text` and the name outlive the call.
        let status = unsafe {
            (svc.get)(
                host_ptr,
                FishStr::new(name),
                &mut out,
                collect as StrSink,
                (&raw mut text).cast(),
            )
        };
        check("reading a game rule", status)?;
        Ok(read(&out, text))
    }

    /// Sets rule `name` (any mod's) in the current world. Fails when the rule is unknown, the
    /// value has another type or lies outside the rule's bounds, or no world is active.
    pub fn set(name: &str, value: Value) -> Result<(), Error> {
        let (svc, host_ptr) = service::<GameRuleServiceV1>(SERVICE_GAME_RULE)?;
        let abi = abi_value(&value);
        // SAFETY: valid service table; the name and the value outlive the call.
        let status = unsafe { (svc.set)(host_ptr, FishStr::new(name), &abi) };
        check("setting a game rule", status)
    }

    /// Calls `listener` whenever the value of rule `name` changes in any world.
    pub fn on_change<F>(name: &str, listener: F) -> Result<(), Error>
    where
        F: Fn(&Change) + Send + Sync + 'static,
    {
        let name = name.to_string();
        CHANGED_EVENT.register(move |ctx| {
            let change = ctx.change();
            if change.name == name {
                listener(&change);
            }
        })
    }
}
