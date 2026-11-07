//! The content tables of mods as the data of the game's own tables.
//!
//! Horizon keeps fuel and composting as components of its item table, tool actions as the rule
//! files of its block transformers (`data/minecraft/block_transformer/<tool>.json`). The edition's
//! bridge calls these functions where the game loads those tables, and the test host calls them on
//! fixtures that stand in for them, so both build exactly the same data.

use fish_hooks::{ContentTables, TransformDef, TransformKind};
use serde_json::{Value, json};

/// The `cooking_fuel` component of an item that burns `burn_ticks` ticks: the burn time and the
/// speed multiplier, as the references and numbers Horizon's providers read.
pub fn fuel_components(burn_ticks: u32) -> (Value, Value) {
    (json!(burn_ticks), json!("minecraft:cooking/speed_default"))
}

/// The `compostable` layers provider of an item a composter takes with probability `chance`
/// (whole percent, `0.01..=1`): the first item into an empty composter always adds a layer, as
/// with the vanilla items; after that the chance decides.
pub fn compost_layers(chance: f32) -> Value {
    let percent = (f64::from(chance) * 100.0).round().clamp(1.0, 100.0) as u32;
    if percent >= 100 {
        return json!(1);
    }
    json!({
        "type": "minecraft:number_dispatcher",
        "cases": [{
            "condition": {
                "type": "minecraft:match_block",
                "blocks": "minecraft:composter",
                "state": { "level": "0" }
            },
            "value": 1
        }],
        "default": {
            "type": "minecraft:weighted_list",
            "distribution": [
                { "data": 1, "weight": percent },
                { "data": 0, "weight": 100 - percent }
            ]
        }
    })
}

/// One entry of a block transformer file (`axe.json`, `hoe.json`, `shovel.json`) for `def`: a
/// transform with a single rule, so a block that does not exist drops this entry only.
pub fn transform_entry(def: &TransformDef) -> Value {
    let air_above = json!({
        "type": "minecraft:matching_block_tag",
        "offset": [0, 1, 0],
        "tag": "minecraft:air"
    });
    let matching = |block: &str| json!({ "type": "minecraft:matching_blocks", "blocks": block });
    // The state the tool leaves, taking the properties both blocks have from the old one.
    let copying = |source: &str| {
        json!({
            "type": "minecraft:copy_properties",
            "source": { "id": source }
        })
    };
    let (rule, sound, particle, neighbours, faces) = match def.kind {
        TransformKind::Strip => (
            json!({ "if_true": matching(&def.from), "then": copying(&def.to) }),
            "minecraft:item.axe.strip",
            None,
            true,
            false,
        ),
        TransformKind::Till => (
            json!({
                "if_true": { "type": "minecraft:all_of", "predicates": [matching(&def.from), air_above] },
                "then": { "id": def.to }
            }),
            "minecraft:item.hoe.till",
            None,
            true,
            true,
        ),
        TransformKind::Flatten => (
            json!({
                "if_true": { "type": "minecraft:all_of", "predicates": [matching(&def.from), air_above] },
                "then": { "id": def.to }
            }),
            "minecraft:item.shovel.flatten",
            None,
            true,
            true,
        ),
        // The axe takes the wax off: waxed block to unwaxed. Honeycomb reads the same rule backwards.
        TransformKind::Wax => (
            json!({ "if_true": matching(&def.to), "then": copying(&def.from) }),
            "minecraft:item.axe.wax_off",
            Some("wax_off"),
            false,
            false,
        ),
        TransformKind::Oxidize => (
            json!({ "if_true": matching(&def.to), "then": copying(&def.from) }),
            "minecraft:item.axe.scrape",
            Some("scrape"),
            false,
            false,
        ),
    };
    let mut entry = json!({
        "block_state_provider": { "type": "minecraft:rule_based", "rules": [rule] },
        "item_damage_per_use": 1,
        "sound": sound
    });
    if faces {
        entry["disallowed_faces"] = json!(["down"]);
    }
    if let Some(particle) = particle {
        entry["particle"] = json!(particle);
    }
    if !neighbours {
        entry["update_from_neighbors"] = json!(false);
    }
    entry
}

/// Adds the tool actions of `tables` for `tool` (`axe`, `hoe` or `shovel`) to the parsed text of
/// that tool's block transformer file, after the vanilla entries (so the vanilla actions win
/// where both match). Returns how many were added.
pub fn patch_transformers(tool: &str, list: &mut Value, tables: &ContentTables) -> usize {
    let Some(entries) = list.as_array_mut() else {
        return 0;
    };
    let before = entries.len();
    entries.extend(
        tables
            .transforms
            .iter()
            .filter(|t| t.kind.tool() == tool)
            .map(transform_entry),
    );
    entries.len() - before
}

/// What [`apply_loot`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LootApplied {
    /// Changes applied.
    pub applied: usize,
    /// The tables changes named that the game does not have (each once).
    pub missing: Vec<String>,
}

/// Applies the loot table changes of mods to the game's tables (`id` to the parsed table):
/// every replacement first, then the added pools in registration order. A change of a table the
/// game does not have is skipped.
pub fn apply_loot(
    tables: &mut std::collections::HashMap<String, Value>,
    changes: &[fish_hooks::LootChange],
) -> LootApplied {
    use fish_hooks::LootChangeKind;
    let mut done = LootApplied::default();
    let miss = |table: &str, done: &mut LootApplied| {
        if !done.missing.iter().any(|m| m == table) {
            done.missing.push(table.to_string());
        }
    };
    for c in changes.iter().filter(|c| c.kind == LootChangeKind::Replace) {
        match (
            tables.get_mut(&c.table),
            serde_json::from_str::<Value>(&c.json),
        ) {
            (Some(slot), Ok(table)) => {
                *slot = table;
                done.applied += 1;
            }
            (None, _) => miss(&c.table, &mut done),
            (Some(_), Err(_)) => {}
        }
    }
    for c in changes
        .iter()
        .filter(|c| c.kind == LootChangeKind::AddPools)
    {
        let Some(table) = tables.get_mut(&c.table) else {
            miss(&c.table, &mut done);
            continue;
        };
        let pools = match serde_json::from_str::<Value>(&c.json) {
            Ok(Value::Array(pools)) => pools,
            Ok(pool @ Value::Object(_)) => vec![pool],
            _ => continue,
        };
        if !table["pools"].is_array() {
            table["pools"] = json!([]);
        }
        if let Some(list) = table["pools"].as_array_mut() {
            list.extend(pools);
            done.applied += 1;
        }
    }
    done
}
/// One line saying what `tables`, `sounds` sound events and `particles` particle types hold.
pub fn summary(tables: &ContentTables, sounds: usize, particles: usize) -> String {
    format!(
        "{} fuel(s), {} compostable item(s), {} flammable block(s), {} tool action(s), {} path type(s), {} sculk frequency(ies), {} sound event(s), {} particle type(s)",
        tables.fuels.len(),
        tables.compostables.len(),
        tables.flammables.len(),
        tables.transforms.len(),
        tables.path_types.len(),
        tables.sculk_frequencies.len(),
        sounds,
        particles
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(kind: TransformKind, from: &str, to: &str) -> TransformDef {
        TransformDef {
            kind,
            from: from.into(),
            to: to.into(),
            mod_id: "m".into(),
        }
    }

    #[test]
    fn a_fuel_is_a_burn_time_and_the_default_speed() {
        let (time, speed) = fuel_components(1600);
        assert_eq!(time, json!(1600));
        assert_eq!(speed, json!("minecraft:cooking/speed_default"));
    }

    #[test]
    fn a_compost_chance_becomes_vanillas_weighted_layers() {
        assert_eq!(compost_layers(1.0), json!(1));
        assert_eq!(compost_layers(1.5), json!(1));
        let layers = compost_layers(0.3);
        assert_eq!(layers["type"], "minecraft:number_dispatcher");
        assert_eq!(layers["cases"][0]["value"], 1);
        let distribution = &layers["default"]["distribution"];
        assert_eq!(distribution[0], json!({ "data": 1, "weight": 30 }));
        assert_eq!(distribution[1], json!({ "data": 0, "weight": 70 }));
        // The smallest chance still has a weight.
        assert_eq!(
            compost_layers(0.001)["default"]["distribution"][0]["weight"],
            1
        );
    }

    #[test]
    fn a_strip_copies_the_properties_of_the_old_block() {
        let e = transform_entry(&def(TransformKind::Strip, "m:log", "m:stripped_log"));
        let rule = &e["block_state_provider"]["rules"][0];
        assert_eq!(rule["if_true"]["blocks"], "m:log");
        assert_eq!(rule["then"]["type"], "minecraft:copy_properties");
        assert_eq!(rule["then"]["source"]["id"], "m:stripped_log");
        assert_eq!(e["sound"], "minecraft:item.axe.strip");
        assert!(e.get("particle").is_none() && e.get("disallowed_faces").is_none());
        assert!(e.get("update_from_neighbors").is_none());
    }

    #[test]
    fn tilling_and_flattening_need_air_above_and_refuse_the_underside() {
        for (kind, sound) in [
            (TransformKind::Till, "minecraft:item.hoe.till"),
            (TransformKind::Flatten, "minecraft:item.shovel.flatten"),
        ] {
            let e = transform_entry(&def(kind, "m:soil", "m:field"));
            let rule = &e["block_state_provider"]["rules"][0];
            let all = &rule["if_true"]["predicates"];
            assert_eq!(all[0]["blocks"], "m:soil");
            assert_eq!(all[1]["offset"], json!([0, 1, 0]));
            assert_eq!(all[1]["tag"], "minecraft:air");
            assert_eq!(rule["then"], json!({ "id": "m:field" }));
            assert_eq!(e["disallowed_faces"], json!(["down"]));
            assert_eq!(e["sound"], sound);
        }
    }

    #[test]
    fn waxing_and_oxidizing_are_axe_rules_from_the_new_block_back_to_the_old() {
        let wax = transform_entry(&def(TransformKind::Wax, "m:copper", "m:waxed_copper"));
        let rule = &wax["block_state_provider"]["rules"][0];
        assert_eq!(rule["if_true"]["blocks"], "m:waxed_copper");
        assert_eq!(rule["then"]["source"]["id"], "m:copper");
        assert_eq!(wax["particle"], "wax_off");
        assert_eq!(wax["update_from_neighbors"], false);
        let scrape = transform_entry(&def(TransformKind::Oxidize, "m:copper", "m:exposed"));
        assert_eq!(scrape["particle"], "scrape");
        assert_eq!(scrape["sound"], "minecraft:item.axe.scrape");
        assert_eq!(
            scrape["block_state_provider"]["rules"][0]["if_true"]["blocks"],
            "m:exposed"
        );
    }

    #[test]
    fn transformer_files_get_only_their_tools_entries_after_the_vanilla_ones() {
        let tables = ContentTables {
            transforms: vec![
                def(TransformKind::Strip, "m:a", "m:b"),
                def(TransformKind::Till, "m:c", "m:d"),
                def(TransformKind::Wax, "m:e", "m:f"),
                def(TransformKind::Flatten, "m:g", "m:h"),
            ],
            ..ContentTables::default()
        };
        let mut axe = json!(["vanilla"]);
        assert_eq!(patch_transformers("axe", &mut axe, &tables), 2);
        assert_eq!(axe[0], "vanilla");
        assert_eq!(axe.as_array().unwrap().len(), 3);
        let mut hoe = json!([]);
        assert_eq!(patch_transformers("hoe", &mut hoe, &tables), 1);
        let mut shovel = json!([]);
        assert_eq!(patch_transformers("shovel", &mut shovel, &tables), 1);
        let mut other = json!([]);
        assert_eq!(patch_transformers("pickaxe", &mut other, &tables), 0);
        let mut not_a_list = json!({});
        assert_eq!(patch_transformers("axe", &mut not_a_list, &tables), 0);
    }

    #[test]
    fn loot_changes_replace_then_add_pools_and_skip_missing_tables() {
        use fish_hooks::{LootChange, LootChangeKind};
        let change = |table: &str, kind, json: &str| LootChange {
            table: table.into(),
            mod_id: "m".into(),
            kind,
            json: json.into(),
        };
        let mut tables: std::collections::HashMap<String, Value> = [
            (
                "minecraft:blocks/stone".to_string(),
                json!({"pools": [{"rolls": 1}]}),
            ),
            (
                "minecraft:blocks/dirt".to_string(),
                json!({"type": "minecraft:block"}),
            ),
        ]
        .into_iter()
        .collect();
        let done = apply_loot(
            &mut tables,
            &[
                change(
                    "minecraft:blocks/stone",
                    LootChangeKind::AddPools,
                    r#"{"rolls": 2}"#,
                ),
                change(
                    "minecraft:blocks/stone",
                    LootChangeKind::Replace,
                    r#"{"pools": []}"#,
                ),
                change(
                    "minecraft:blocks/dirt",
                    LootChangeKind::AddPools,
                    r#"[{"rolls": 3}, {"rolls": 4}]"#,
                ),
                change("minecraft:blocks/none", LootChangeKind::AddPools, r#"{}"#),
                change("minecraft:blocks/none", LootChangeKind::Replace, r#"{}"#),
            ],
        );
        assert_eq!(done.applied, 3);
        assert_eq!(done.missing, ["minecraft:blocks/none"]);
        // the replacement came first, then the pool was added to it
        assert_eq!(
            tables["minecraft:blocks/stone"],
            json!({"pools": [{"rolls": 2}]})
        );
        assert_eq!(
            tables["minecraft:blocks/dirt"]["pools"],
            json!([{"rolls": 3}, {"rolls": 4}])
        );
    }

    #[test]
    fn the_summary_counts_every_table() {
        let text = summary(&ContentTables::default(), 2, 3);
        assert!(text.starts_with("0 fuel(s), 0 compostable"));
        assert!(text.ends_with("2 sound event(s), 3 particle type(s)"));
    }
}
