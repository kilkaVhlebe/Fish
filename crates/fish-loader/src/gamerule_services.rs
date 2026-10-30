//! Host side of `fish:game_rule`, and the loader's game rule operations the hooks use.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;

use fish_abi::client::StrSink;
use fish_abi::gamerules::{
    CHANGED, GameRuleChangedPayload, GameRuleServiceV1, RuleDefV1, RuleValueV1, kind,
};
use fish_abi::{FishServiceHeader, FishStatus, FishStr};
use fish_hooks::{GameRuleDef, RuleKind, RuleValue};

use crate::gamerules::{SetError, current, file_of, read_file, write_file};
use crate::inner::Inner;

/// The categories the game lists rules under.
pub(crate) const CATEGORIES: [&str; 7] = [
    "player", "mobs", "spawning", "drops", "updates", "chat", "misc",
];

/// Most choices an enum rule may have.
const MAX_CHOICES: usize = 64;

fn guard(f: impl FnOnce() -> FishStatus) -> FishStatus {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(FishStatus::PANICKED)
}

/// A choice is typed in a command: letters, digits and `_ - .` only.
fn valid_choice(c: &str) -> bool {
    !c.is_empty()
        && c.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

/// A value as the ABI carries it; text is borrowed from `value`.
pub(crate) fn to_abi(value: &RuleValue) -> RuleValueV1 {
    match value {
        RuleValue::Bool(b) => RuleValueV1::boolean(*b),
        RuleValue::Int(i) => RuleValueV1::int(*i),
        RuleValue::Double(d) => RuleValueV1::double(*d),
        RuleValue::Text(t) => RuleValueV1::choice(t),
    }
}

/// Reads a value from the ABI; `None` for an unknown kind or text that is not UTF-8.
///
/// # Safety
/// The text of an enum value must be valid for the call.
unsafe fn from_abi(v: &RuleValueV1) -> Option<RuleValue> {
    Some(match v.kind {
        kind::BOOL => RuleValue::Bool(v.boolean != 0),
        kind::INT => RuleValue::Int(v.int),
        kind::DOUBLE => RuleValue::Double(v.double),
        // SAFETY: forwarded to the caller.
        kind::ENUM => RuleValue::Text(unsafe { v.text.as_str() }.ok()?.to_string()),
        _ => return None,
    })
}

/// Builds a rule from what a mod registered; `Err` is the reason.
///
/// # Safety
/// `def` must be valid, with `choice_count` readable strings at `choices`, for the call.
unsafe fn read_def(name: &str, category: &str, def: &RuleDefV1) -> Result<GameRuleDef, String> {
    if (def.size as usize) < size_of::<RuleDefV1>() {
        return Err("its definition table is too small".to_string());
    }
    // SAFETY: forwarded to the caller.
    let default = unsafe { from_abi(&def.default_value) }
        .ok_or_else(|| "its default value is invalid".to_string())?;
    let kind = match def.kind {
        kind::BOOL => RuleKind::Bool,
        kind::INT => RuleKind::Int {
            min: def.min_int,
            max: def.max_int,
        },
        kind::DOUBLE => RuleKind::Double {
            min: def.min_double,
            max: def.max_double,
        },
        kind::ENUM => {
            if def.choices.is_null() || def.choice_count == 0 || def.choice_count > MAX_CHOICES {
                return Err(format!("it needs 1 to {MAX_CHOICES} choices"));
            }
            // SAFETY: forwarded to the caller.
            let raw = unsafe { std::slice::from_raw_parts(def.choices, def.choice_count) };
            let mut choices: Vec<String> = Vec::with_capacity(raw.len());
            for c in raw {
                // SAFETY: forwarded to the caller.
                let c = unsafe { c.as_str() }.map_err(|_| "a choice is not UTF-8".to_string())?;
                if !valid_choice(c) {
                    return Err(format!(
                        "the choice {c:?} must be made of letters, digits, _, - and ."
                    ));
                }
                if choices.iter().any(|x| x == c) {
                    return Err(format!("the choice {c} is listed twice"));
                }
                choices.push(c.to_string());
            }
            RuleKind::Enum(choices)
        }
        other => return Err(format!("its type {other} is unknown")),
    };
    match &kind {
        RuleKind::Int { min, max } if min > max => {
            return Err(format!("its bounds {min}..{max} are reversed"));
        }
        RuleKind::Double { min, max } if !(min.is_finite() && max.is_finite() && min <= max) => {
            return Err(format!(
                "its bounds {min}..{max} are not finite and ordered"
            ));
        }
        _ => {}
    }
    let rule = GameRuleDef {
        name: name.to_string(),
        category: category.to_string(),
        kind,
        default,
    };
    rule.check(&rule.default)
        .map_err(|why| format!("its default is wrong: {why}"))?;
    Ok(rule)
}

unsafe extern "C" fn register(
    host: *mut c_void,
    token: u64,
    name: FishStr,
    category: FishStr,
    def: *const RuleDefV1,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`.
        let inner = unsafe { &*host.cast::<Inner>() };
        // SAFETY: valid strings for the call.
        let (Ok(name), Ok(category)) = (unsafe { name.as_str() }, unsafe { category.as_str() })
        else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if def.is_null() || !fish_abi::ident::is_valid_identifier(name) {
            return FishStatus::INVALID_ARGUMENT;
        }
        let who = inner
            .record(token)
            .map_or_else(|| "a mod".to_string(), |r| r.manifest.name.clone());
        if name.starts_with("minecraft:") {
            inner.warn(format!(
                "{who} registered the game rule {name}: the minecraft namespace belongs to the game"
            ));
            return FishStatus::INVALID_ARGUMENT;
        }
        if !CATEGORIES.contains(&category) {
            inner.warn(format!(
                "{who} registered the game rule {name} in the unknown category {category:?} (use {})",
                CATEGORIES.join(", ")
            ));
            return FishStatus::INVALID_ARGUMENT;
        }
        if inner.net.frozen.load(Ordering::Acquire) {
            inner.warn(format!(
                "registries are frozen: the game rule {name} of {who} was not added"
            ));
            return FishStatus::INVALID_ARGUMENT;
        }
        // SAFETY: the table and its choices are valid for the call (ABI contract).
        let rule = match unsafe { read_def(name, category, &*def) } {
            Ok(r) => r,
            Err(why) => {
                inner.warn(format!(
                    "{who} registered the invalid game rule {name}: {why}"
                ));
                return FishStatus::INVALID_ARGUMENT;
            }
        };
        let added = inner
            .rules
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .register(rule, token);
        match added {
            Ok(()) => FishStatus::OK,
            Err(first) => {
                let first = inner
                    .record(first)
                    .map_or_else(|| "another mod".to_string(), |r| r.manifest.name.clone());
                inner.warn(format!(
                    "{who} registered the game rule {name}, which {first} already registered"
                ));
                FishStatus::ALREADY_EXISTS
            }
        }
    })
}

unsafe extern "C" fn get(
    host: *mut c_void,
    name: FishStr,
    out: *mut RuleValueV1,
    text: StrSink,
    user: *mut c_void,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let inner = unsafe { &*host.cast::<Inner>() };
        // SAFETY: valid string for the call.
        let Ok(name) = (unsafe { name.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if out.is_null() {
            return FishStatus::INVALID_ARGUMENT;
        }
        let world = current();
        let value = inner
            .rules
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(world.as_deref(), name);
        let Some(value) = value else {
            return FishStatus::NOT_FOUND;
        };
        let mut v = to_abi(&value);
        if let RuleValue::Text(t) = &value {
            // SAFETY: the sink and its user pointer come from the caller; the text lives through the call.
            unsafe { text(user, FishStr::new(t)) };
            v.text = FishStr::EMPTY;
        }
        // SAFETY: `out` checked non-null.
        unsafe { *out = v };
        FishStatus::OK
    })
}

unsafe extern "C" fn set(
    host: *mut c_void,
    name: FishStr,
    value: *const RuleValueV1,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let inner = unsafe { &*host.cast::<Inner>() };
        // SAFETY: valid string for the call.
        let Ok(name) = (unsafe { name.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if value.is_null() {
            return FishStatus::INVALID_ARGUMENT;
        }
        // SAFETY: `value` is valid for the call.
        let Some(value) = (unsafe { from_abi(&*value) }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let known = inner
            .rules
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(None, name)
            .is_some();
        if !known {
            return FishStatus::NOT_FOUND;
        }
        let Some(world) = current() else {
            return FishStatus::UNSUPPORTED;
        };
        match inner.rules_set(&world, name, &value) {
            Ok(_) => FishStatus::OK,
            Err(SetError::Unknown(_)) => FishStatus::NOT_FOUND,
            Err(SetError::Mismatch(_)) => FishStatus::TYPE_MISMATCH,
            Err(SetError::Invalid(_)) => FishStatus::INVALID_ARGUMENT,
        }
    })
}

impl Inner {
    /// Sets rule `name` in `world`, raises `fish:game_rule/changed` when the value changed, and
    /// returns the previous value.
    pub(crate) fn rules_set(
        &self,
        world: &str,
        name: &str,
        value: &RuleValue,
    ) -> Result<RuleValue, SetError> {
        let old = self
            .rules
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set(world, name, value)?;
        if old != *value {
            let (old_abi, new_abi) = (to_abi(&old), to_abi(value));
            let mut payload = GameRuleChangedPayload {
                size: size_of::<GameRuleChangedPayload>() as u32,
                reserved: 0,
                name: FishStr::new(name),
                world: FishStr::new(world),
                old: old_abi,
                new_value: new_abi,
            };
            // SAFETY: the payload matches the event's documented layout; `Notify` events take no result.
            unsafe {
                self.bus
                    .invoke_id(CHANGED, (&raw mut payload).cast(), std::ptr::null_mut())
            };
        }
        Ok(old)
    }

    /// Reads the saved rules of `world` (a world was opened).
    pub(crate) fn rules_load(&self, world: &str) {
        let saved = match read_file(world) {
            Ok(saved) => saved,
            Err(why) => {
                self.warn(why);
                None
            }
        };
        let warnings = self
            .rules
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .load(world, saved.as_ref());
        for w in warnings {
            self.warn(format!("{w} (world {world})"));
        }
    }

    /// Writes the rules of `world` beside the game's own (a world is being saved).
    pub(crate) fn rules_save(&self, world: &str) {
        let (seen, content) = {
            let rules = self.rules.lock().unwrap_or_else(|e| e.into_inner());
            (rules.has_world(world), rules.snapshot(world))
        };
        // A world no rule was ever read or set in keeps whatever file it has.
        if !seen {
            return;
        }
        let result = match content {
            Some(text) => write_file(world, &text),
            // Nothing to keep: a file left from before (all values back at their defaults) goes.
            None if file_of(world).is_file() => std::fs::remove_file(file_of(world))
                .map_err(|e| format!("could not remove {}: {e}", file_of(world).display())),
            None => Ok(()),
        };
        if let Err(why) = result {
            self.warn(why);
        }
    }
}

/// The `fish:game_rule` table.
pub(crate) static GAME_RULE: GameRuleServiceV1 = GameRuleServiceV1 {
    header: FishServiceHeader {
        size: size_of::<GameRuleServiceV1>(),
        version: 1,
    },
    register,
    get,
    set,
};

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use fish_abi::gamerules::RuleDefV1;
    use fish_metadata::Side;

    use super::*;
    use crate::boot::{BootConfig, Loader, boot};
    use crate::gamerules::enter;

    /// A booted loader without mods, its host pointer, and its game directory.
    fn host() -> (Loader, *mut c_void, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = BootConfig::new(Side::Client, dir.path());
        cfg.log_sink = Some(Arc::new(|_| {}));
        let loader = boot(cfg).unwrap();
        // Entrypoints are over, so the registries are frozen: open them for these tests.
        loader.inner.net.frozen.store(false, Ordering::Release);
        let ptr = Arc::as_ptr(&loader.inner).cast_mut().cast();
        (loader, ptr, dir)
    }

    fn def(kind: u8, default: RuleValueV1) -> RuleDefV1 {
        RuleDefV1 {
            size: size_of::<RuleDefV1>() as u32,
            kind,
            reserved: [0; 3],
            default_value: default,
            min_int: 0,
            max_int: 10,
            min_double: 0.0,
            max_double: 10.0,
            choices: std::ptr::null(),
            choice_count: 0,
        }
    }

    fn reg(host: *mut c_void, name: &str, category: &str, d: &RuleDefV1) -> FishStatus {
        // SAFETY: the host pointer is a live `Inner`; the strings and the table outlive the call.
        unsafe { register(host, 1, FishStr::new(name), FishStr::new(category), d) }
    }

    unsafe extern "C" fn collect(user: *mut c_void, text: FishStr) {
        // SAFETY: `user` is the `String` the tests pass.
        unsafe { (*user.cast::<String>()).push_str(text.as_str().unwrap()) };
    }

    fn read(host: *mut c_void, name: &str) -> (FishStatus, RuleValueV1, String) {
        let mut out = RuleValueV1::int(0);
        let mut text = String::new();
        // SAFETY: valid host and out pointers for the call.
        let st = unsafe {
            get(
                host,
                FishStr::new(name),
                &mut out,
                collect,
                (&raw mut text).cast(),
            )
        };
        (st, out, text)
    }

    fn write(host: *mut c_void, name: &str, v: &RuleValueV1) -> FishStatus {
        // SAFETY: valid host and value pointers for the call.
        unsafe { set(host, FishStr::new(name), v) }
    }

    #[test]
    fn bad_registrations_are_refused_with_a_warning_naming_the_rule() {
        let (loader, h, _dir) = host();
        let ok = def(kind::INT, RuleValueV1::int(3));
        let check = |name: &str, category: &str, d: &RuleDefV1, why: &str| {
            assert_eq!(
                reg(h, name, category, d),
                FishStatus::INVALID_ARGUMENT,
                "{name}"
            );
            let warnings = loader.inner.warnings.lock().unwrap().clone();
            assert!(
                warnings.iter().any(|w| w.contains(why)),
                "{why} not in {warnings:?}"
            );
        };
        assert_eq!(reg(h, "nocolon", "misc", &ok), FishStatus::INVALID_ARGUMENT);
        // SAFETY: the host pointer is live; a null table is checked for.
        let null = unsafe {
            register(
                h,
                1,
                FishStr::new("a:b"),
                FishStr::new("misc"),
                std::ptr::null(),
            )
        };
        assert_eq!(null, FishStatus::INVALID_ARGUMENT);
        check("minecraft:mine", "misc", &ok, "belongs to the game");
        check("a:cat", "nonsense", &ok, "unknown category");
        let mut small = ok;
        small.size = 4;
        check("a:small", "misc", &small, "too small");
        let mut bad_default = ok;
        bad_default.default_value = RuleValueV1 {
            kind: 9,
            ..RuleValueV1::int(0)
        };
        check(
            "a:default",
            "misc",
            &bad_default,
            "default value is invalid",
        );
        let mut bad_kind = ok;
        bad_kind.kind = 9;
        check("a:kind", "misc", &bad_kind, "type 9 is unknown");
        let mut reversed = ok;
        reversed.min_int = 5;
        reversed.max_int = 1;
        check("a:rev", "misc", &reversed, "reversed");
        let mut outside = ok;
        outside.default_value = RuleValueV1::int(99);
        check("a:out", "misc", &outside, "default is wrong");
        let mut nan = def(kind::DOUBLE, RuleValueV1::double(1.0));
        nan.min_double = f64::NAN;
        check("a:nan", "misc", &nan, "not finite and ordered");
        let mut wrong_type = ok;
        wrong_type.default_value = RuleValueV1::boolean(true);
        check("a:type", "misc", &wrong_type, "default is wrong");

        let choices = |list: &[&'static str]| -> Vec<FishStr> {
            list.iter().map(|c| FishStr::from_static(c)).collect()
        };
        let enum_def = |list: &[FishStr], default: &str| {
            let mut d = def(kind::ENUM, RuleValueV1::choice(default));
            d.choices = list.as_ptr();
            d.choice_count = list.len();
            d
        };
        let none = def(kind::ENUM, RuleValueV1::choice("a"));
        check("a:nochoice", "misc", &none, "1 to 64 choices");
        let spaced = choices(&["a b"]);
        check(
            "a:space",
            "misc",
            &enum_def(&spaced, "a b"),
            "must be made of",
        );
        let twice = choices(&["a", "a"]);
        check("a:twice", "misc", &enum_def(&twice, "a"), "listed twice");
        let ab = choices(&["a", "b"]);
        check("a:missing", "misc", &enum_def(&ab, "c"), "default is wrong");
        let invalid = [FishStr {
            ptr: [0xff_u8].as_ptr(),
            len: 1,
        }];
        check("a:utf8", "misc", &enum_def(&invalid, "a"), "not UTF-8");
        assert_eq!(
            reg(h, "a:fine", "misc", &enum_def(&ab, "b")),
            FishStatus::OK
        );

        // a second registration of the name keeps the first
        assert_eq!(reg(h, "a:fine", "misc", &ok), FishStatus::ALREADY_EXISTS);
        assert!(
            loader
                .inner
                .warnings
                .lock()
                .unwrap()
                .iter()
                .any(|w| w.contains("a:fine") && w.contains("already registered"))
        );
        // after the freeze nothing registers
        loader.inner.net.frozen.store(true, Ordering::Release);
        assert_eq!(reg(h, "a:late", "misc", &ok), FishStatus::INVALID_ARGUMENT);
        assert!(
            loader
                .inner
                .warnings
                .lock()
                .unwrap()
                .iter()
                .any(|w| w.contains("registries are frozen") && w.contains("a:late"))
        );
        assert_eq!(loader.inner.rules.lock().unwrap().defs().len(), 1);
    }

    #[test]
    fn reading_and_writing_follow_the_threads_world() {
        let (_loader, h, _dir) = host();
        let int = def(kind::INT, RuleValueV1::int(3));
        assert_eq!(reg(h, "a:n", "mobs", &int), FishStatus::OK);
        let words = [FishStr::from_static("x"), FishStr::from_static("y")];
        let mut en = def(kind::ENUM, RuleValueV1::choice("x"));
        en.choices = words.as_ptr();
        en.choice_count = 2;
        assert_eq!(reg(h, "a:e", "misc", &en), FishStatus::OK);
        assert_eq!(
            reg(
                h,
                "a:b",
                "misc",
                &def(kind::BOOL, RuleValueV1::boolean(true))
            ),
            FishStatus::OK
        );

        // outside of a world: the default can be read, nothing can be written
        let (st, v, _) = read(h, "a:n");
        assert_eq!((st, v.int), (FishStatus::OK, 3));
        assert_eq!(
            write(h, "a:n", &RuleValueV1::int(4)),
            FishStatus::UNSUPPORTED
        );
        assert_eq!(
            write(h, "a:zz", &RuleValueV1::int(4)),
            FishStatus::NOT_FOUND
        );
        assert_eq!(read(h, "a:zz").0, FishStatus::NOT_FOUND);

        let _world = enter(Some("w".to_string()));
        assert_eq!(write(h, "a:n", &RuleValueV1::int(4)), FishStatus::OK);
        assert_eq!(read(h, "a:n").1.int, 4);
        assert_eq!(
            write(h, "a:n", &RuleValueV1::int(40)),
            FishStatus::INVALID_ARGUMENT
        );
        assert_eq!(
            write(h, "a:n", &RuleValueV1::boolean(true)),
            FishStatus::TYPE_MISMATCH
        );
        assert_eq!(
            write(h, "a:b", &RuleValueV1::boolean(false)),
            FishStatus::OK
        );
        assert_eq!(read(h, "a:b").1.boolean, 0);
        assert_eq!(write(h, "a:e", &RuleValueV1::choice("y")), FishStatus::OK);
        let (st, v, text) = read(h, "a:e");
        assert_eq!(
            (st, v.kind, text.as_str()),
            (FishStatus::OK, kind::ENUM, "y")
        );
        assert_eq!(
            write(h, "a:e", &RuleValueV1::choice("z")),
            FishStatus::INVALID_ARGUMENT
        );
        let junk = RuleValueV1 {
            kind: 9,
            ..RuleValueV1::int(0)
        };
        assert_eq!(write(h, "a:n", &junk), FishStatus::INVALID_ARGUMENT);
        // SAFETY: a null value is checked for.
        let st = unsafe { set(h, FishStr::new("a:n"), std::ptr::null()) };
        assert_eq!(st, FishStatus::INVALID_ARGUMENT);
        // SAFETY: a null out pointer is checked for.
        let st = unsafe {
            get(
                h,
                FishStr::new("a:n"),
                std::ptr::null_mut(),
                collect,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(st, FishStatus::INVALID_ARGUMENT);
    }

    #[test]
    fn rules_are_saved_beside_the_games_and_read_back() {
        let (loader, h, dir) = host();
        let inner = &loader.inner;
        assert_eq!(
            reg(h, "a:n", "misc", &def(kind::INT, RuleValueV1::int(3))),
            FishStatus::OK
        );
        let world = dir.path().join("saves/w");
        let id = world.to_string_lossy().into_owned();
        let file = world.join("data/fish/game_rules.json");

        // a world nobody touched keeps whatever it has
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{\"schema\":1,\"rules\":{\"a:n\":6}}").unwrap();
        inner.rules_save(&id);
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("\"a:n\":6")
        );

        inner.rules_load(&id);
        assert_eq!(
            inner.rules.lock().unwrap().get(Some(&id), "a:n"),
            Some(RuleValue::Int(6))
        );
        assert_eq!(
            inner.rules_set(&id, "a:n", &RuleValue::Int(7)),
            Ok(RuleValue::Int(6))
        );
        inner.rules_save(&id);
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("\"a:n\": 7")
        );
        // back to the default: the file goes
        inner.rules_set(&id, "a:n", &RuleValue::Int(3)).unwrap();
        inner.rules_save(&id);
        assert!(!file.exists());
        inner.rules_save(&id);

        // a broken file is reported and ignored
        std::fs::write(&file, "{").unwrap();
        inner.rules_load(&id);
        assert!(
            inner
                .warnings
                .lock()
                .unwrap()
                .iter()
                .any(|w| w.contains("is not valid JSON"))
        );
        assert_eq!(
            inner.rules.lock().unwrap().get(Some(&id), "a:n"),
            Some(RuleValue::Int(3))
        );
        // a value of the wrong type is reported with the world
        std::fs::write(&file, "{\"rules\":{\"a:n\":\"x\"}}").unwrap();
        inner.rules_load(&id);
        assert!(
            inner
                .warnings
                .lock()
                .unwrap()
                .iter()
                .any(|w| w.contains("a:n") && w.contains(&id))
        );
        // a write that cannot happen is a warning, not a crash
        inner.rules_set(&id, "a:n", &RuleValue::Int(8)).unwrap();
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir_all(&file).unwrap();
        inner.rules_save(&id);
        assert!(
            inner
                .warnings
                .lock()
                .unwrap()
                .iter()
                .any(|w| w.contains("could not write"))
        );
    }

    #[test]
    fn a_change_is_announced_once_to_listeners_of_the_event() {
        use fish_abi::gamerules::GameRuleChangedPayload;
        static SEEN: Mutex<Vec<(String, i32, i32)>> = Mutex::new(Vec::new());
        unsafe extern "C" fn listener(
            _user: *mut c_void,
            payload: *mut c_void,
            _out: *mut c_void,
        ) -> FishStatus {
            // SAFETY: the loader passes a `GameRuleChangedPayload`.
            let p = unsafe { &*payload.cast::<GameRuleChangedPayload>() };
            SEEN.lock().unwrap().push((
                // SAFETY: valid strings for the callback.
                unsafe { p.name.as_str() }.unwrap().to_string(),
                p.old.int,
                p.new_value.int,
            ));
            FishStatus::OK
        }
        let (loader, h, _dir) = host();
        assert_eq!(
            reg(h, "a:n", "misc", &def(kind::INT, RuleValueV1::int(3))),
            FishStatus::OK
        );
        let status = loader.inner.bus.register(crate::bus::Registration {
            id: CHANGED,
            owner: 1,
            failed: Arc::default(),
            phase: crate::bus::DEFAULT_PHASE,
            func: listener,
            user: std::ptr::null_mut(),
            drop_user: None,
        });
        assert!(status.is_ok(), "{status:?}");
        loader
            .inner
            .rules_set("w", "a:n", &RuleValue::Int(5))
            .unwrap();
        loader
            .inner
            .rules_set("w", "a:n", &RuleValue::Int(5))
            .unwrap();
        loader
            .inner
            .rules_set("x", "a:n", &RuleValue::Int(3))
            .unwrap();
        assert_eq!(*SEEN.lock().unwrap(), [("a:n".to_string(), 3, 5)]);
    }
}
