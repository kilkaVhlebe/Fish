//! Resource conditions (Fabric's `ResourceConditions`): a JSON file under `data/` of a mod may
//! carry `"fish:load_conditions": [ ... ]`; the loader copies it into the content folder only
//! when every condition holds, without the key, and leaves it out otherwise.
//!
//! Conditions are objects with a `condition` name (`fish:` or `fabric:` prefix):
//!
//! | condition | holds when |
//! | --- | --- |
//! | `all_mods_loaded` | every id of `values` is a loaded mod (`minecraft` and `fish` always are) |
//! | `any_mods_loaded` | one id of `values` is |
//! | `not` | the condition `value` does not hold |
//! | `and`, `or` | every / one condition of `values` holds |
//! | `true` | always |

use serde_json::Value;

/// The key of the conditions in a JSON file.
pub const KEY: &str = "fish:load_conditions";

/// What happens to a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Filtered {
    /// Copy these bytes (the file without its conditions, or untouched).
    Keep(Vec<u8>),
    /// Its conditions do not hold: leave it out.
    Skip,
    /// Its conditions cannot be read: leave it out and say why.
    Invalid(String),
}

/// Applies the conditions of `bytes`, the file `path` of a mod; `loaded` says whether a mod id is
/// loaded. Only `data/**.json` files that are JSON objects with [`KEY`] are touched.
pub(crate) fn filter(path: &str, bytes: Vec<u8>, loaded: &dyn Fn(&str) -> bool) -> Filtered {
    if !(path.starts_with("data/") && path.ends_with(".json")) {
        return Filtered::Keep(bytes);
    }
    // A cheap look first: most files have no conditions.
    if !bytes.windows(KEY.len()).any(|w| w == KEY.as_bytes()) {
        return Filtered::Keep(bytes);
    }
    let Ok(Value::Object(mut doc)) = serde_json::from_slice::<Value>(&bytes) else {
        return Filtered::Keep(bytes);
    };
    let Some(conditions) = doc.remove(KEY) else {
        return Filtered::Keep(bytes);
    };
    let Value::Array(list) = conditions else {
        return Filtered::Invalid(format!("{KEY} is not a list"));
    };
    let mut all = true;
    for c in &list {
        match holds(c, loaded) {
            Ok(true) => {}
            Ok(false) => all = false,
            Err(why) => return Filtered::Invalid(why),
        }
    }
    if !all {
        return Filtered::Skip;
    }
    match serde_json::to_vec_pretty(&Value::Object(doc)) {
        Ok(bytes) => Filtered::Keep(bytes),
        Err(e) => Filtered::Invalid(e.to_string()),
    }
}

fn mods(c: &Value) -> Result<Vec<&str>, String> {
    c["values"]
        .as_array()
        .ok_or("the condition has no list of mod ids in `values`")?
        .iter()
        .map(|v| v.as_str().ok_or("a mod id is not a string".to_string()))
        .collect()
}

fn conditions(c: &Value) -> Result<&Vec<Value>, String> {
    c["values"]
        .as_array()
        .ok_or_else(|| "the condition has no list of conditions in `values`".to_string())
}

/// Whether condition `c` holds.
pub(crate) fn holds(c: &Value, loaded: &dyn Fn(&str) -> bool) -> Result<bool, String> {
    let name = c["condition"]
        .as_str()
        .ok_or("a condition has no `condition` name")?;
    let short = name
        .strip_prefix("fish:")
        .or_else(|| name.strip_prefix("fabric:"))
        .unwrap_or("");
    let is_loaded = |id: &str| matches!(id, "minecraft" | "fish") || loaded(id);
    match short {
        "true" => Ok(true),
        "all_mods_loaded" => Ok(mods(c)?.into_iter().all(is_loaded)),
        "any_mods_loaded" => Ok(mods(c)?.into_iter().any(is_loaded)),
        "not" => Ok(!holds(&c["value"], loaded)?),
        "and" => {
            let mut all = true;
            for c in conditions(c)? {
                all &= holds(c, loaded)?;
            }
            Ok(all)
        }
        "or" => {
            let mut any = false;
            for c in conditions(c)? {
                any |= holds(c, loaded)?;
            }
            Ok(any)
        }
        _ => Err(format!("unknown condition {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn loaded(id: &str) -> bool {
        id == "ruby"
    }

    fn run(doc: &Value) -> Filtered {
        filter(
            "data/m/recipe/x.json",
            serde_json::to_vec(doc).unwrap(),
            &loaded,
        )
    }

    #[test]
    fn conditions_decide_whether_a_file_is_copied() {
        let keep = run(
            &json!({"type": "x", KEY: [{"condition": "fish:all_mods_loaded", "values": ["ruby", "minecraft"]}]}),
        );
        let Filtered::Keep(bytes) = keep else {
            panic!("{keep:?}")
        };
        let doc: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(doc, json!({"type": "x"}));
        assert_eq!(
            run(&json!({KEY: [{"condition": "fish:all_mods_loaded", "values": ["ruby", "gem"]}]})),
            Filtered::Skip
        );
        assert!(matches!(
            run(
                &json!({KEY: [{"condition": "fabric:any_mods_loaded", "values": ["gem", "ruby"]}]})
            ),
            Filtered::Keep(_)
        ));
        assert_eq!(
            run(&json!({KEY: [{"condition": "fish:not", "value": {"condition": "fish:true"}}]})),
            Filtered::Skip
        );
        assert!(matches!(
            run(&json!({KEY: [{"condition": "fish:or", "values": [
                {"condition": "fish:any_mods_loaded", "values": ["gem"]},
                {"condition": "fish:and", "values": [{"condition": "fish:true"}, {"condition": "fish:all_mods_loaded", "values": ["fish"]}]}
            ]}]})),
            Filtered::Keep(_)
        ));
    }

    #[test]
    fn broken_conditions_are_reported_and_other_files_are_untouched() {
        for (doc, why) in [
            (json!({KEY: {}}), "not a list"),
            (json!({KEY: [{"values": []}]}), "no `condition`"),
            (
                json!({KEY: [{"condition": "fish:nope"}]}),
                "unknown condition fish:nope",
            ),
            (
                json!({KEY: [{"condition": "fish:all_mods_loaded"}]}),
                "list of mod ids",
            ),
            (
                json!({KEY: [{"condition": "fish:all_mods_loaded", "values": [1]}]}),
                "not a string",
            ),
            (
                json!({KEY: [{"condition": "fish:and", "values": 3}]}),
                "list of conditions",
            ),
        ] {
            match run(&doc) {
                Filtered::Invalid(e) => assert!(e.contains(why), "{e} / {why}"),
                other => panic!("{doc}: {other:?}"),
            }
        }
        // not data, not JSON, not an object, no key: untouched
        let raw = br#"{"fish:load_conditions": [{"condition": "fish:nope"}]}"#.to_vec();
        assert_eq!(
            filter("assets/m/x.json", raw.clone(), &loaded),
            Filtered::Keep(raw.clone())
        );
        assert_eq!(
            filter("data/m/x.txt", raw.clone(), &loaded),
            Filtered::Keep(raw)
        );
        let text = b"[1, 2] fish:load_conditions".to_vec();
        assert_eq!(
            filter("data/m/x.json", text.clone(), &loaded),
            Filtered::Keep(text)
        );
        let plain = br#"{"a": 1}"#.to_vec();
        assert_eq!(
            filter("data/m/x.json", plain.clone(), &loaded),
            Filtered::Keep(plain)
        );
        let array = br#"["fish:load_conditions"]"#.to_vec();
        assert_eq!(
            filter("data/m/x.json", array.clone(), &loaded),
            Filtered::Keep(array)
        );
    }
}
