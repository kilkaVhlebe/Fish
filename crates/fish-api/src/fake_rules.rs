//! A fake `fish:game_rule` (and `fish:registry`) service, and the tests of `game_rule` built on it.

use std::ffi::c_void;
use std::sync::Mutex;

use fish_abi::client::StrSink;
use fish_abi::gamerules::{
    GameRuleChangedPayload, GameRuleServiceV1, RuleDefV1, RuleValueV1, SERVICE_GAME_RULE, kind,
};
use fish_abi::registry::{RegistryServiceV1, SERVICE_REGISTRY};
use fish_abi::{FishServiceHeader, FishStatus, FishStr};

use crate::game_rule::{Category, ChangeContext, GameRules, Value};

/// What the fake knows of one rule.
struct Stored {
    category: String,
    /// Bounds: integer min and max, decimal min and max.
    bounds: (i32, i32, f64, f64),
    choices: Vec<String>,
    value: Value,
}

static RULES: Mutex<Vec<(String, Stored)>> = Mutex::new(Vec::new());
static WORLD: Mutex<bool> = Mutex::new(true);
static FROZEN: Mutex<bool> = Mutex::new(false);
static SERIAL: Mutex<()> = Mutex::new(());

fn text(s: FishStr) -> String {
    // SAFETY: the API passes valid strings.
    unsafe { s.as_str() }.unwrap().to_string()
}

fn value_of(v: &RuleValueV1) -> Value {
    match v.kind {
        kind::BOOL => Value::Bool(v.boolean != 0),
        kind::INT => Value::Int(v.int),
        kind::DOUBLE => Value::Double(v.double),
        _ => Value::Choice(text(v.text)),
    }
}

unsafe extern "C" fn register(
    _h: *mut c_void,
    _token: u64,
    name: FishStr,
    category: FishStr,
    def: *const RuleDefV1,
) -> FishStatus {
    let name = text(name);
    if *FROZEN.lock().unwrap() {
        return FishStatus::INVALID_ARGUMENT;
    }
    // SAFETY: the API passes a valid table.
    let def = unsafe { *def };
    let mut rules = RULES.lock().unwrap();
    if rules.iter().any(|(n, _)| *n == name) {
        return FishStatus::ALREADY_EXISTS;
    }
    let choices = if def.choices.is_null() {
        Vec::new()
    } else {
        // SAFETY: the API passes `choice_count` valid strings.
        unsafe { std::slice::from_raw_parts(def.choices, def.choice_count) }
            .iter()
            .map(|c| text(*c))
            .collect()
    };
    let value = value_of(&def.default_value);
    rules.push((
        name,
        Stored {
            category: text(category),
            bounds: (def.min_int, def.max_int, def.min_double, def.max_double),
            choices,
            value,
        },
    ));
    FishStatus::OK
}

unsafe extern "C" fn get(
    _h: *mut c_void,
    name: FishStr,
    out: *mut RuleValueV1,
    sink: StrSink,
    user: *mut c_void,
) -> FishStatus {
    let name = text(name);
    let rules = RULES.lock().unwrap();
    let Some((_, s)) = rules.iter().find(|(n, _)| *n == name) else {
        return FishStatus::NOT_FOUND;
    };
    // SAFETY: the API passes valid pointers.
    unsafe {
        *out = match &s.value {
            Value::Bool(b) => RuleValueV1::boolean(*b),
            Value::Int(i) => RuleValueV1::int(*i),
            Value::Double(d) => RuleValueV1::double(*d),
            Value::Choice(c) => {
                sink(user, FishStr::new(c));
                RuleValueV1::choice("")
            }
        };
    }
    FishStatus::OK
}

unsafe extern "C" fn set(_h: *mut c_void, name: FishStr, value: *const RuleValueV1) -> FishStatus {
    let name = text(name);
    // SAFETY: the API passes a valid value.
    let value = value_of(unsafe { &*value });
    let mut rules = RULES.lock().unwrap();
    let Some((_, s)) = rules.iter_mut().find(|(n, _)| *n == name) else {
        return FishStatus::NOT_FOUND;
    };
    if !*WORLD.lock().unwrap() {
        return FishStatus::UNSUPPORTED;
    }
    let fits = match (&s.value, &value) {
        (Value::Bool(_), Value::Bool(_)) => true,
        (Value::Int(_), Value::Int(v)) => (s.bounds.0..=s.bounds.1).contains(v),
        (Value::Double(_), Value::Double(v)) => (s.bounds.2..=s.bounds.3).contains(v),
        (Value::Choice(_), Value::Choice(c)) => s.choices.contains(c),
        _ => return FishStatus::TYPE_MISMATCH,
    };
    if !fits {
        return FishStatus::INVALID_ARGUMENT;
    }
    s.value = value;
    FishStatus::OK
}

static GAME_RULE: GameRuleServiceV1 = GameRuleServiceV1 {
    header: FishServiceHeader {
        size: size_of::<GameRuleServiceV1>(),
        version: 1,
    },
    register,
    get,
    set,
};

unsafe extern "C" fn reg_register(
    _h: *mut c_void,
    _t: u64,
    _r: FishStr,
    _i: FishStr,
    _o: *mut u32,
) -> FishStatus {
    FishStatus::UNSUPPORTED
}

unsafe extern "C" fn reg_raw(
    _h: *mut c_void,
    _r: FishStr,
    _i: FishStr,
    _o: *mut u32,
) -> FishStatus {
    FishStatus::NOT_FOUND
}

unsafe extern "C" fn reg_frozen(_h: *mut c_void) -> u8 {
    u8::from(*FROZEN.lock().unwrap())
}

static REGISTRY: RegistryServiceV1 = RegistryServiceV1 {
    header: FishServiceHeader {
        size: size_of::<RegistryServiceV1>(),
        version: 1,
    },
    register: reg_register,
    raw_id: reg_raw,
    frozen: reg_frozen,
};

/// The table for a service id of this fake.
pub(crate) fn lookup(id: &str) -> Option<*const c_void> {
    Some(match id {
        SERVICE_GAME_RULE => (&raw const GAME_RULE).cast(),
        SERVICE_REGISTRY => (&raw const REGISTRY).cast(),
        _ => return None,
    })
}

/// Starts a test with an empty fake (tests share one process-wide fake host).
fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    crate::fake::install();
    RULES.lock().unwrap().clear();
    *WORLD.lock().unwrap() = true;
    *FROZEN.lock().unwrap() = false;
    guard
}

#[test]
fn every_rule_type_is_registered_with_its_bounds_and_read_back() {
    let _g = fresh();
    let flag = GameRules::register_bool("t:flag", Category::Player, true).unwrap();
    let count = GameRules::register_int("t:count", Category::Mobs, 3, 0, 9).unwrap();
    let ratio = GameRules::register_double("t:ratio", Category::Drops, 0.5, 0.0, 2.0).unwrap();
    let mode =
        GameRules::register_enum("t:mode", Category::Misc, "easy", &["easy", "hard"]).unwrap();
    {
        let rules = RULES.lock().unwrap();
        let by = |n: &str| &rules.iter().find(|(name, _)| name == n).unwrap().1;
        assert_eq!(by("t:flag").category, "player");
        assert_eq!(by("t:count").category, "mobs");
        assert_eq!((by("t:count").bounds.0, by("t:count").bounds.1), (0, 9));
        assert_eq!(by("t:ratio").bounds.3, 2.0);
        assert_eq!(by("t:mode").choices, ["easy", "hard"]);
        assert_eq!(by("t:mode").category, "misc");
    }
    assert_eq!(flag.name(), "t:flag");
    assert!(flag.get().unwrap());
    assert_eq!(count.get().unwrap(), 3);
    assert_eq!(ratio.get().unwrap(), 0.5);
    assert_eq!(mode.get().unwrap(), "easy");
    flag.set(false).unwrap();
    count.set(9).unwrap();
    ratio.set(1.5).unwrap();
    mode.set("hard").unwrap();
    assert!(!flag.get().unwrap());
    assert_eq!(count.get().unwrap(), 9);
    assert_eq!(ratio.get().unwrap(), 1.5);
    assert_eq!(mode.get().unwrap(), "hard");
    assert_eq!(
        GameRules::get("t:mode").unwrap(),
        Value::Choice("hard".into())
    );
    assert_eq!(count.name(), "t:count");
    assert_eq!(ratio.name(), "t:ratio");
    assert_eq!(mode.name(), "t:mode");
}

#[test]
fn refusals_carry_the_hosts_status() {
    let _g = fresh();
    let count = GameRules::register_int("t:count", Category::Misc, 3, 0, 9).unwrap();
    let mode = GameRules::register_enum("t:mode", Category::Misc, "a", &["a", "b"]).unwrap();
    let st = |e: crate::Error| e.status().unwrap();
    assert_eq!(
        st(GameRules::register_bool("t:count", Category::Misc, true).unwrap_err()),
        FishStatus::ALREADY_EXISTS
    );
    assert_eq!(st(count.set(10).unwrap_err()), FishStatus::INVALID_ARGUMENT);
    assert_eq!(st(mode.set("c").unwrap_err()), FishStatus::INVALID_ARGUMENT);
    assert_eq!(
        st(GameRules::set("t:count", Value::Bool(true)).unwrap_err()),
        FishStatus::TYPE_MISMATCH
    );
    assert_eq!(
        st(GameRules::set("t:none", Value::Bool(true)).unwrap_err()),
        FishStatus::NOT_FOUND
    );
    assert_eq!(
        st(GameRules::get("t:none").unwrap_err()),
        FishStatus::NOT_FOUND
    );
    // no world: the host cannot set
    *WORLD.lock().unwrap() = false;
    assert_eq!(st(count.set(4).unwrap_err()), FishStatus::UNSUPPORTED);
    assert_eq!(count.get().unwrap(), 3);
}

#[test]
fn a_typed_handle_refuses_a_rule_of_another_type() {
    let _g = fresh();
    GameRules::register_int("t:count", Category::Misc, 3, 0, 9).unwrap();
    // a handle to a rule that holds another type (obtained by name from another mod's rule)
    let wrong = GameRules::register_bool("t:flag", Category::Misc, true).unwrap();
    let as_int = crate::game_rule::IntRule::from_name(wrong.name());
    let err = as_int.get().unwrap_err();
    assert!(err.to_string().contains("not the expected type"), "{err}");
    let as_double = crate::game_rule::DoubleRule::from_name(wrong.name());
    assert!(as_double.get().is_err());
    let as_enum = crate::game_rule::EnumRule::from_name(wrong.name());
    assert!(as_enum.get().is_err());
    let as_bool = crate::game_rule::BoolRule::from_name("t:count");
    assert!(as_bool.get().is_err());
}

#[test]
fn registering_after_the_freeze_says_so() {
    let _g = fresh();
    *FROZEN.lock().unwrap() = true;
    let err = GameRules::register_bool("t:late", Category::Misc, true).unwrap_err();
    assert_eq!(err.to_string(), "invalid argument: registries are frozen");
    *FROZEN.lock().unwrap() = false;
    // not frozen: another refusal keeps its status
    GameRules::register_bool("t:a", Category::Misc, true).unwrap();
    assert_eq!(
        GameRules::register_bool("t:a", Category::Misc, true)
            .unwrap_err()
            .status(),
        Some(FishStatus::ALREADY_EXISTS)
    );
}

#[test]
fn every_category_has_its_wire_name() {
    let all = [
        (Category::Player, "player"),
        (Category::Mobs, "mobs"),
        (Category::Spawning, "spawning"),
        (Category::Drops, "drops"),
        (Category::Updates, "updates"),
        (Category::Chat, "chat"),
        (Category::Misc, "misc"),
    ];
    for (c, name) in all {
        assert_eq!(c.as_str(), name);
    }
}

#[test]
fn change_listeners_see_their_rule_only() {
    let _g = fresh();
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    SEEN.lock().unwrap().clear();
    GameRules::on_change("t:count", |c| {
        SEEN.lock()
            .unwrap()
            .push(format!("{} {} {:?}->{:?}", c.name, c.world, c.old, c.new));
    })
    .unwrap();
    let fire = |name: &str, world: &str, old: RuleValueV1, new: RuleValueV1| {
        let mut p = GameRuleChangedPayload {
            size: size_of::<GameRuleChangedPayload>() as u32,
            reserved: 0,
            name: FishStr::new(name),
            world: FishStr::new(world),
            old,
            new_value: new,
        };
        // SAFETY: `ChangeContext` is `repr(transparent)` over the payload.
        let ctx: &mut ChangeContext = unsafe { &mut *(&raw mut p).cast() };
        crate::game_rule::CHANGED_EVENT.invoke(ctx).unwrap();
    };
    fire("t:count", "/w", RuleValueV1::int(3), RuleValueV1::int(4));
    fire("t:other", "/w", RuleValueV1::int(3), RuleValueV1::int(4));
    fire(
        "t:count",
        "/w",
        RuleValueV1::choice("a"),
        RuleValueV1::double(1.5),
    );
    fire(
        "t:count",
        "",
        RuleValueV1::boolean(false),
        RuleValueV1::boolean(true),
    );
    assert_eq!(
        *SEEN.lock().unwrap(),
        [
            "t:count /w Int(3)->Int(4)",
            "t:count /w Choice(\"a\")->Double(1.5)",
            "t:count  Bool(false)->Bool(true)",
        ]
    );
}
