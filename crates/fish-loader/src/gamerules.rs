//! Fish game rules: what mods registered, every world's values, and their persistence.
//!
//! The game keeps its own rules in `data/minecraft/game_rules.dat`, a file vanilla refuses when it
//! holds a key it does not know. Fish rules therefore live beside it, under Fish's own namespace:
//! `<world>/data/fish/game_rules.json` (`{"schema": 1, "rules": {"mymod:rule": value}}`). Only
//! values that differ from the default are written, and values of rules no mod registers (a mod
//! was removed) are kept as they were, so removing a mod for a while loses nothing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fish_hooks::{GameRuleDef, RuleKind, RuleValue};
use serde_json::{Map, Value};

/// Name of the saved file below a world's directory.
const FILE: &str = "data/fish/game_rules.json";

/// Why a rule could not be set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetError {
    /// No mod registered the rule.
    Unknown(String),
    /// The value is of another type.
    Mismatch(String),
    /// The value is out of bounds.
    Invalid(String),
}

impl SetError {
    /// The message for the player.
    pub(crate) fn message(&self) -> &str {
        match self {
            SetError::Unknown(m) | SetError::Mismatch(m) | SetError::Invalid(m) => m,
        }
    }
}

struct Entry {
    def: GameRuleDef,
    owner: u64,
}

/// The values of one world.
#[derive(Default)]
struct WorldRules {
    /// Values that differ from the default.
    values: HashMap<String, RuleValue>,
    /// Saved values of rules nobody registers, written back unchanged.
    extra: Map<String, Value>,
}

/// Registered rules and the values of every world seen so far.
#[derive(Default)]
pub(crate) struct RuleStore {
    entries: Vec<Entry>,
    worlds: HashMap<String, WorldRules>,
}

fn same_kind(kind: &RuleKind, value: &RuleValue) -> bool {
    matches!(
        (kind, value),
        (RuleKind::Bool, RuleValue::Bool(_))
            | (RuleKind::Int { .. }, RuleValue::Int(_))
            | (RuleKind::Double { .. }, RuleValue::Double(_))
            | (RuleKind::Enum(_), RuleValue::Text(_))
    )
}

impl RuleStore {
    /// Adds a rule. `Err` holds the owner of the rule of that name.
    pub(crate) fn register(&mut self, def: GameRuleDef, owner: u64) -> Result<(), u64> {
        if let Some(e) = self.entries.iter().find(|e| e.def.name == def.name) {
            return Err(e.owner);
        }
        self.entries.push(Entry { def, owner });
        Ok(())
    }

    /// Every rule, in registration order.
    pub(crate) fn defs(&self) -> Vec<GameRuleDef> {
        self.entries.iter().map(|e| e.def.clone()).collect()
    }

    fn def(&self, name: &str) -> Option<&GameRuleDef> {
        self.entries.iter().map(|e| &e.def).find(|d| d.name == name)
    }

    /// The value of `name` in `world`; the default without a world or without a stored value.
    pub(crate) fn get(&self, world: Option<&str>, name: &str) -> Option<RuleValue> {
        let def = self.def(name)?;
        Some(
            world
                .and_then(|w| self.worlds.get(w))
                .and_then(|w| w.values.get(name))
                .unwrap_or(&def.default)
                .clone(),
        )
    }

    /// Sets `name` in `world`; returns the previous value.
    pub(crate) fn set(
        &mut self,
        world: &str,
        name: &str,
        value: &RuleValue,
    ) -> Result<RuleValue, SetError> {
        let def = self
            .def(name)
            .ok_or_else(|| SetError::Unknown(format!("no game rule {name}")))?
            .clone();
        if !same_kind(&def.kind, value) {
            return Err(SetError::Mismatch(
                def.check(value).expect_err("a mismatch is an error"),
            ));
        }
        def.check(value).map_err(SetError::Invalid)?;
        let rules = self.worlds.entry(world.to_string()).or_default();
        let old = rules.values.get(name).unwrap_or(&def.default).clone();
        if *value == def.default {
            rules.values.remove(name);
        } else {
            rules.values.insert(name.to_string(), value.clone());
        }
        Ok(old)
    }

    /// Replaces the values of `world` with `saved` (the file's parsed content); returns what to
    /// warn about.
    pub(crate) fn load(&mut self, world: &str, saved: Option<&Value>) -> Vec<String> {
        let mut warnings = Vec::new();
        let mut rules = WorldRules::default();
        if let Some(Value::Object(root)) = saved
            && let Some(Value::Object(map)) = root.get("rules")
        {
            for (name, raw) in map {
                let Some(def) = self.def(name) else {
                    rules.extra.insert(name.clone(), raw.clone());
                    continue;
                };
                let value = match (&def.kind, raw) {
                    (RuleKind::Bool, Value::Bool(b)) => Some(RuleValue::Bool(*b)),
                    (RuleKind::Int { .. }, Value::Number(n)) => n
                        .as_i64()
                        .and_then(|i| i32::try_from(i).ok())
                        .map(RuleValue::Int),
                    (RuleKind::Double { .. }, Value::Number(n)) => {
                        n.as_f64().map(RuleValue::Double)
                    }
                    (RuleKind::Enum(_), Value::String(s)) => Some(RuleValue::Text(s.clone())),
                    _ => None,
                };
                match value.map(|v| def.check(&v).map(|()| v)) {
                    Some(Ok(v)) => {
                        if v != def.default {
                            rules.values.insert(name.clone(), v);
                        }
                    }
                    Some(Err(why)) => warnings.push(format!(
                        "ignored the saved value of game rule {name}: {why}"
                    )),
                    None => warnings.push(format!(
                        "ignored the saved value of game rule {name}: it is not {}",
                        match &def.kind {
                            RuleKind::Bool => "true or false",
                            RuleKind::Int { .. } => "a whole number",
                            RuleKind::Double { .. } => "a number",
                            RuleKind::Enum(_) => "a text",
                        }
                    )),
                }
            }
        }
        self.worlds.insert(world.to_string(), rules);
        warnings
    }

    /// True when `world` was loaded or had a rule set.
    pub(crate) fn has_world(&self, world: &str) -> bool {
        self.worlds.contains_key(world)
    }

    /// The file content of `world`; `None` when the world has nothing to write.
    pub(crate) fn snapshot(&self, world: &str) -> Option<String> {
        let rules = self.worlds.get(world)?;
        let mut out = rules.extra.clone();
        for e in &self.entries {
            if let Some(v) = rules.values.get(&e.def.name) {
                out.insert(
                    e.def.name.clone(),
                    match v {
                        RuleValue::Bool(b) => Value::Bool(*b),
                        RuleValue::Int(i) => Value::from(*i),
                        RuleValue::Double(d) => Value::from(*d),
                        RuleValue::Text(t) => Value::String(t.clone()),
                    },
                );
            }
        }
        if out.is_empty() {
            return None;
        }
        let root = serde_json::json!({ "schema": 1, "rules": out });
        Some(serde_json::to_string_pretty(&root).expect("a JSON value serialises"))
    }
}

/// `<world>/data/fish/game_rules.json`.
pub(crate) fn file_of(world: &str) -> PathBuf {
    Path::new(world).join(FILE)
}

/// Reads and parses a world's saved rules; `Err` is a warning, `Ok(None)` means no file.
pub(crate) fn read_file(world: &str) -> Result<Option<Value>, String> {
    let path = file_of(world);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    serde_json::from_str(&text).map(Some).map_err(|e| {
        format!(
            "{} is not valid JSON ({e}); its rules are ignored",
            path.display()
        )
    })
}

/// Writes `content` to a world's file (through a temporary file, so a crash never leaves half of it).
pub(crate) fn write_file(world: &str, content: &str) -> Result<(), String> {
    let path = file_of(world);
    let fail = |e: std::io::Error| format!("could not write {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content).map_err(fail)?;
    std::fs::rename(&tmp, &path).map_err(fail)
}

thread_local! {
    /// The world of the game running on this thread, while one of its events runs.
    static WORLD: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Restores the previous world of the thread when dropped.
pub(crate) struct WorldGuard(Option<String>);

impl Drop for WorldGuard {
    fn drop(&mut self) {
        WORLD.with(|w| *w.borrow_mut() = self.0.take());
    }
}

/// Makes `world` the thread's world until the guard drops.
pub(crate) fn enter(world: Option<String>) -> WorldGuard {
    WorldGuard(WORLD.with(|w| w.replace(world)))
}

/// The world of the game running on this thread, if an event of one is running.
pub(crate) fn current() -> Option<String> {
    WORLD.with(|w| w.borrow().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, kind: RuleKind, default: RuleValue) -> GameRuleDef {
        GameRuleDef {
            name: name.to_string(),
            category: "misc".to_string(),
            kind,
            default,
        }
    }

    fn store() -> RuleStore {
        let mut s = RuleStore::default();
        s.register(def("m:flag", RuleKind::Bool, RuleValue::Bool(false)), 1)
            .unwrap();
        s.register(
            def(
                "m:count",
                RuleKind::Int { min: 0, max: 9 },
                RuleValue::Int(3),
            ),
            1,
        )
        .unwrap();
        s.register(
            def(
                "m:ratio",
                RuleKind::Double { min: 0.0, max: 2.0 },
                RuleValue::Double(0.5),
            ),
            2,
        )
        .unwrap();
        s.register(
            def(
                "m:mode",
                RuleKind::Enum(vec!["easy".into(), "hard".into()]),
                RuleValue::Text("easy".into()),
            ),
            2,
        )
        .unwrap();
        s
    }

    #[test]
    fn values_are_per_world_and_default_without_one() {
        let mut s = store();
        assert_eq!(
            s.register(def("m:flag", RuleKind::Bool, RuleValue::Bool(true)), 9),
            Err(1)
        );
        assert_eq!(s.defs().len(), 4);
        assert_eq!(s.get(None, "m:count"), Some(RuleValue::Int(3)));
        assert_eq!(s.get(Some("a"), "nope"), None);
        assert_eq!(
            s.set("a", "m:count", &RuleValue::Int(7)),
            Ok(RuleValue::Int(3))
        );
        assert_eq!(s.get(Some("a"), "m:count"), Some(RuleValue::Int(7)));
        assert_eq!(s.get(Some("b"), "m:count"), Some(RuleValue::Int(3)));
        assert_eq!(s.get(None, "m:count"), Some(RuleValue::Int(3)));
        // setting the default again forgets the value
        assert_eq!(
            s.set("a", "m:count", &RuleValue::Int(3)),
            Ok(RuleValue::Int(7))
        );
        assert_eq!(s.snapshot("a"), None);
    }

    #[test]
    fn a_wrong_value_is_refused_with_its_reason() {
        let mut s = store();
        assert!(matches!(
            s.set("a", "m:nope", &RuleValue::Bool(true)),
            Err(SetError::Unknown(m)) if m == "no game rule m:nope"
        ));
        let e = s.set("a", "m:flag", &RuleValue::Int(1)).unwrap_err();
        assert!(matches!(e, SetError::Mismatch(_)));
        assert_eq!(e.message(), "m:flag expects a boolean, got an integer");
        let e = s.set("a", "m:count", &RuleValue::Int(10)).unwrap_err();
        assert!(matches!(e, SetError::Invalid(_)));
        assert_eq!(e.message(), "m:count must be between 0 and 9, not 10");
        assert!(
            s.set("a", "m:mode", &RuleValue::Text("odd".into()))
                .is_err()
        );
        assert!(s.set("a", "m:ratio", &RuleValue::Double(f64::NAN)).is_err());
        assert_eq!(s.get(Some("a"), "m:count"), Some(RuleValue::Int(3)));
    }

    #[test]
    fn snapshots_hold_only_non_default_values_and_round_trip() {
        let mut s = store();
        s.set("w", "m:flag", &RuleValue::Bool(true)).unwrap();
        s.set("w", "m:ratio", &RuleValue::Double(1.25)).unwrap();
        s.set("w", "m:mode", &RuleValue::Text("hard".into()))
            .unwrap();
        let text = s.snapshot("w").unwrap();
        let json: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["schema"], 1);
        assert_eq!(json["rules"]["m:flag"], true);
        assert_eq!(json["rules"]["m:ratio"], 1.25);
        assert_eq!(json["rules"]["m:mode"], "hard");
        assert!(json["rules"].get("m:count").is_none());
        assert_eq!(s.snapshot("never seen"), None);
        assert!(s.has_world("w") && !s.has_world("never seen"));

        let mut t = store();
        assert!(t.load("w", Some(&json)).is_empty());
        assert_eq!(t.get(Some("w"), "m:flag"), Some(RuleValue::Bool(true)));
        assert_eq!(t.get(Some("w"), "m:ratio"), Some(RuleValue::Double(1.25)));
        assert_eq!(
            t.get(Some("w"), "m:mode"),
            Some(RuleValue::Text("hard".into()))
        );
        assert_eq!(t.get(Some("w"), "m:count"), Some(RuleValue::Int(3)));
    }

    #[test]
    fn loading_ignores_bad_values_keeps_unknown_rules_and_replaces_the_world() {
        let mut s = store();
        s.set("w", "m:flag", &RuleValue::Bool(true)).unwrap();
        let saved = serde_json::json!({"schema": 1, "rules": {
            "m:flag": 1, "m:count": 99, "m:ratio": "x", "m:mode": "weird",
            "m:fits": 4, "gone:rule": {"a": [1, 2]}, "m:big": 5000000000i64
        }});
        let warnings = s.load("w", Some(&saved));
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        let has = |a: &str, b: &str| warnings.iter().any(|w| w.contains(a) && w.contains(b));
        assert!(has("m:flag", "true or false"));
        assert!(has("m:count", "between 0 and 9"));
        assert!(has("m:ratio", "a number"));
        assert!(has("m:mode", "one of easy, hard"));
        // the old in-memory value is gone: the world was reloaded
        assert_eq!(s.get(Some("w"), "m:flag"), Some(RuleValue::Bool(false)));
        // a rule nobody registers survives a save
        let text = s.snapshot("w").unwrap();
        let json: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["rules"]["gone:rule"]["a"][1], 2);
        // a world without a file, or with junk, starts from defaults
        assert!(s.load("x", None).is_empty());
        assert!(s.load("x", Some(&Value::from(3))).is_empty());
        assert_eq!(s.get(Some("x"), "m:count"), Some(RuleValue::Int(3)));
        let junk = serde_json::json!({"rules": 5});
        assert!(s.load("x", Some(&junk)).is_empty());
        // an integer rule given an out-of-range number is ignored too
        let big = serde_json::json!({"rules": {"m:count": 5000000000i64}});
        assert_eq!(s.load("x", Some(&big)).len(), 1);
    }

    #[test]
    fn the_file_is_written_atomically_under_the_fish_namespace() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().to_str().unwrap();
        assert_eq!(read_file(world), Ok(None));
        write_file(world, "{\"schema\":1,\"rules\":{\"m:flag\":true}}").unwrap();
        assert!(dir.path().join("data/fish/game_rules.json").is_file());
        assert!(!dir.path().join("data/fish/game_rules.json.tmp").exists());
        write_file(world, "{\"schema\":1,\"rules\":{}}").unwrap();
        let back = read_file(world).unwrap().unwrap();
        assert_eq!(back["schema"], 1);
        std::fs::write(file_of(world), "not json").unwrap();
        assert!(read_file(world).unwrap_err().contains("is not valid JSON"));
        // a directory where the file should be: reading and writing fail with a message
        let blocked = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(blocked.path().join("data/fish/game_rules.json")).unwrap();
        let blocked = blocked.path().to_str().unwrap();
        assert!(read_file(blocked).unwrap_err().contains("could not read"));
        assert!(
            write_file(blocked, "{}")
                .unwrap_err()
                .contains("could not write")
        );
    }

    #[test]
    fn the_threads_world_is_scoped_and_restored() {
        assert_eq!(current(), None);
        {
            let _outer = enter(Some("a".into()));
            assert_eq!(current().as_deref(), Some("a"));
            {
                let _inner = enter(None);
                assert_eq!(current(), None);
            }
            assert_eq!(current().as_deref(), Some("a"));
        }
        assert_eq!(current(), None);
    }
}
