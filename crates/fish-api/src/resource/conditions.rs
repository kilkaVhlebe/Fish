//! Resource conditions (Fabric's `ResourceConditions`): a JSON file under `data/` of a mod is
//! copied into the game only when its conditions hold.
//!
//! The file carries them under [`KEY`]; the loader evaluates them when it writes the mod's content
//! folder at start, copies the file without the key when every condition holds and leaves it out
//! otherwise (logged at info level). A condition it cannot read leaves the file out with a
//! warning.
//!
//! ```json
//! {
//!   "fish:load_conditions": [
//!     { "condition": "fish:all_mods_loaded", "values": ["gems"] }
//!   ],
//!   "type": "minecraft:crafting_shapeless",
//!   "ingredients": ["gems:raw_gem"],
//!   "result": { "id": "mymod:ruby" }
//! }
//! ```
//!
//! [`Condition`] writes the same JSON from Rust (for generated data files):
//!
//! ```
//! use fish_api::resource::conditions::Condition;
//!
//! let only_with_gems = Condition::all_mods_loaded(["gems"]);
//! let without_gems = Condition::not(only_with_gems.clone());
//! assert_eq!(
//!     Condition::list(&[only_with_gems]),
//!     r#"[{"condition":"fish:all_mods_loaded","values":["gems"]}]"#
//! );
//! assert!(without_gems.to_json().starts_with(r#"{"condition":"fish:not""#));
//! ```
//!
//! The `fabric:` names of the same conditions are accepted too, so data files ported from Fabric
//! mods keep working when they use `fabric:load_conditions`'s conditions under [`KEY`].

/// The key of the conditions in a JSON file.
pub const KEY: &str = "fish:load_conditions";

/// A resource condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// Always holds.
    True,
    /// Every mod id is loaded (`minecraft` and `fish` always are).
    AllModsLoaded(Vec<String>),
    /// One of the mod ids is loaded.
    AnyModsLoaded(Vec<String>),
    /// The condition does not hold.
    Not(Box<Condition>),
    /// Every condition holds.
    And(Vec<Condition>),
    /// One of the conditions holds.
    Or(Vec<Condition>),
}

fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Condition {
    /// Every mod of `ids` is loaded.
    pub fn all_mods_loaded<S: AsRef<str>>(ids: impl IntoIterator<Item = S>) -> Condition {
        Condition::AllModsLoaded(ids.into_iter().map(|s| s.as_ref().to_string()).collect())
    }

    /// One mod of `ids` is loaded.
    pub fn any_mods_loaded<S: AsRef<str>>(ids: impl IntoIterator<Item = S>) -> Condition {
        Condition::AnyModsLoaded(ids.into_iter().map(|s| s.as_ref().to_string()).collect())
    }

    /// `condition` does not hold.
    #[allow(clippy::should_implement_trait)]
    pub fn not(condition: Condition) -> Condition {
        Condition::Not(Box::new(condition))
    }

    /// The condition as a JSON object.
    pub fn to_json(&self) -> String {
        let ids = |ids: &[String]| ids.iter().map(|s| quoted(s)).collect::<Vec<_>>().join(",");
        let all = |cs: &[Condition]| {
            cs.iter()
                .map(Condition::to_json)
                .collect::<Vec<_>>()
                .join(",")
        };
        match self {
            Condition::True => r#"{"condition":"fish:true"}"#.to_string(),
            Condition::AllModsLoaded(v) => {
                format!(
                    r#"{{"condition":"fish:all_mods_loaded","values":[{}]}}"#,
                    ids(v)
                )
            }
            Condition::AnyModsLoaded(v) => {
                format!(
                    r#"{{"condition":"fish:any_mods_loaded","values":[{}]}}"#,
                    ids(v)
                )
            }
            Condition::Not(c) => format!(r#"{{"condition":"fish:not","value":{}}}"#, c.to_json()),
            Condition::And(cs) => format!(r#"{{"condition":"fish:and","values":[{}]}}"#, all(cs)),
            Condition::Or(cs) => format!(r#"{{"condition":"fish:or","values":[{}]}}"#, all(cs)),
        }
    }

    /// The JSON list of `conditions`, the value of [`KEY`].
    pub fn list(conditions: &[Condition]) -> String {
        let items: Vec<String> = conditions.iter().map(Condition::to_json).collect();
        format!("[{}]", items.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_condition_writes_its_json() {
        let c = Condition::Or(vec![
            Condition::True,
            Condition::And(vec![
                Condition::any_mods_loaded(["a", "b\"c"]),
                Condition::not(Condition::all_mods_loaded(Vec::<&str>::new())),
            ]),
        ]);
        assert_eq!(
            c.to_json(),
            concat!(
                r#"{"condition":"fish:or","values":[{"condition":"fish:true"},"#,
                r#"{"condition":"fish:and","values":[{"condition":"fish:any_mods_loaded","values":["a","b\"c"]},"#,
                r#"{"condition":"fish:not","value":{"condition":"fish:all_mods_loaded","values":[]}}]}]}"#
            )
        );
        assert_eq!(Condition::list(&[]), "[]");
        assert_eq!(quoted("a\\b\n"), r#""a\\b\u000a""#);
        assert_eq!(KEY, "fish:load_conditions");
    }
}
