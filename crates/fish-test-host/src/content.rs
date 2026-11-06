//! Content tables, sound events and particle types in the simulation.
//!
//! The simulated game keeps small stand-ins for the tables Horizon builds while it loads its
//! resources (the fuel and composting components of items, the block transformer files of the
//! axe, hoe and shovel, the fire odds and pathfinding types of blocks, the vibration frequencies of
//! game events) and applies the entries mods registered to them with the same functions the
//! edition's bridge uses ([`fish_loader::tables`]). The sounds a mod can play are the events of
//! the `sounds.json` files in the content folders the loader wrote, plus a few vanilla ones.

use std::collections::{BTreeMap, BTreeSet};

use fish_hooks::{ContentTables, ParticleDef, SoundDef, TransformDef, TransformKind};
use fish_loader::tables;
use serde_json::{Value, json};

use crate::TestHost;
use crate::script::{Expect, ScriptFailure};

/// The vanilla sound events the simulated game knows besides the mods' own.
pub const VANILLA_SOUNDS: [&str; 3] = [
    "minecraft:entity.experience_orb.pickup",
    "minecraft:block.note_block.bell",
    "minecraft:item.axe.strip",
];

/// A few vanilla vibration frequencies (`VibrationSystem.getGameEventFrequency`).
const VANILLA_FREQUENCIES: [(&str, u8); 4] = [
    ("minecraft:step", 1),
    ("minecraft:item_interact_finish", 3),
    ("minecraft:eat", 8),
    ("minecraft:block_place", 13),
];

/// What the simulated game's own tables hold once the mods' entries are applied.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContentSim {
    /// Item id to its components: `fuel` (the burn time) and `compostable` (the layers provider).
    pub items: BTreeMap<String, Value>,
    /// Tool (`axe`, `hoe`, `shovel`) to the entries of its block transformer file.
    pub transformers: BTreeMap<String, Value>,
    /// Block id to its fire odds (ignite, burn); absent blocks do not burn.
    pub flammables: BTreeMap<String, (i32, i32)>,
    /// Block id to its pathfinding type; absent blocks are `BLOCKED`.
    pub path_types: BTreeMap<String, String>,
    /// The sound events the game knows (`namespace:path`).
    pub known_sounds: BTreeSet<String>,
    /// Sounds mods played, as `id volume v pitch p` or `id at x y z volume v pitch p`.
    pub played_sounds: Vec<String>,
    /// The block the last `use_tool` left (`none` when the tool did nothing).
    pub tool_result: String,
}

fn vanilla_transform(kind: TransformKind, from: &str, to: &str) -> Value {
    tables::transform_entry(&TransformDef {
        kind,
        from: from.to_string(),
        to: to.to_string(),
        mod_id: "minecraft".to_string(),
    })
}

/// The vanilla tables the simulation starts from.
fn vanilla() -> ContentSim {
    let mut sim = ContentSim::default();
    for (item, ticks) in [
        ("minecraft:coal", 1600),
        ("minecraft:stick", 100),
        ("minecraft:oak_planks", 300),
    ] {
        sim.items.insert(
            item.to_string(),
            json!({ "fuel": tables::fuel_components(ticks).0 }),
        );
    }
    for (item, chance) in [("minecraft:apple", 0.65), ("minecraft:wheat_seeds", 0.3)] {
        sim.items.insert(
            item.to_string(),
            json!({ "compostable": tables::compost_layers(chance) }),
        );
    }
    for item in [
        "minecraft:diamond",
        "minecraft:stone",
        "minecraft:iron_block",
    ] {
        sim.items.insert(item.to_string(), json!({}));
    }
    sim.transformers.insert(
        "axe".to_string(),
        json!([
            vanilla_transform(
                TransformKind::Strip,
                "minecraft:oak_log",
                "minecraft:stripped_oak_log"
            ),
            vanilla_transform(
                TransformKind::Wax,
                "minecraft:copper_block",
                "minecraft:waxed_copper_block"
            ),
        ]),
    );
    sim.transformers.insert(
        "hoe".to_string(),
        json!([vanilla_transform(
            TransformKind::Till,
            "minecraft:dirt",
            "minecraft:farmland"
        )]),
    );
    sim.transformers.insert(
        "shovel".to_string(),
        json!([vanilla_transform(
            TransformKind::Flatten,
            "minecraft:grass_block",
            "minecraft:dirt_path"
        )]),
    );
    sim.flammables
        .insert("minecraft:oak_planks".to_string(), (5, 20));
    sim.flammables
        .insert("minecraft:oak_log".to_string(), (5, 5));
    sim.path_types
        .insert("minecraft:cactus".to_string(), "DAMAGING".to_string());
    sim.known_sounds = VANILLA_SOUNDS.iter().map(|s| (*s).to_string()).collect();
    sim
}

impl ContentSim {
    /// Applies the entries mods registered, as the bridge does where Horizon loads each table.
    pub fn apply(&mut self, t: &ContentTables) {
        for f in &t.fuels {
            let item = self
                .items
                .entry(f.item.clone())
                .or_insert_with(|| json!({}));
            if f.burn_ticks == 0 {
                item.as_object_mut().map(|o| o.remove("fuel"));
            } else {
                item["fuel"] = tables::fuel_components(f.burn_ticks).0;
            }
        }
        for c in &t.compostables {
            let item = self
                .items
                .entry(c.item.clone())
                .or_insert_with(|| json!({}));
            if c.chance <= 0.0 {
                item.as_object_mut().map(|o| o.remove("compostable"));
            } else {
                item["compostable"] = tables::compost_layers(c.chance);
            }
        }
        for f in &t.flammables {
            if f.ignite_odds == 0 && f.burn_odds == 0 {
                self.flammables.remove(&f.block);
            } else {
                self.flammables
                    .insert(f.block.clone(), (f.ignite_odds, f.burn_odds));
            }
        }
        for (tool, list) in &mut self.transformers {
            tables::patch_transformers(tool, list, t);
        }
        for p in &t.path_types {
            self.path_types.insert(p.block.clone(), p.path_type.clone());
        }
    }

    /// The burn time of `item` in a furnace, zero when it is no fuel.
    pub fn fuel_ticks(&self, item: &str) -> u32 {
        self.items
            .get(item)
            .and_then(|i| i["fuel"].as_u64())
            .map_or(0, |t| u32::try_from(t).unwrap_or(u32::MAX))
    }

    /// The chance that `item` adds a layer to a composter that already has one, zero when it is
    /// not compostable.
    pub fn compost_chance(&self, item: &str) -> f32 {
        let Some(layers) = self.items.get(item).map(|i| &i["compostable"]) else {
            return 0.0;
        };
        match layers {
            Value::Number(n) => n.as_f64().map_or(0.0, |n| n.min(1.0) as f32),
            Value::Object(_) => layers["default"]["distribution"][0]["weight"]
                .as_f64()
                .map_or(0.0, |w| (w / 100.0) as f32),
            _ => 0.0,
        }
    }

    /// The block `tool` turns `block` into, when the block above is air: `axe`, `hoe` or `shovel`
    /// read their transformer file (the first entry that gives a block wins, like Horizon's
    /// `Transformers::apply`), `honeycomb` reads the axe's wax-off entries backwards
    /// (`HoneycombItem.getWaxed`).
    pub fn use_tool(&self, tool: &str, block: &str) -> Option<String> {
        if tool == "honeycomb" {
            return self.waxed(block);
        }
        let list = self.transformers.get(tool)?.as_array()?;
        list.iter()
            .find_map(|entry| provide(&entry["block_state_provider"], block))
    }

    fn waxed(&self, block: &str) -> Option<String> {
        let list = self.transformers.get("axe")?.as_array()?;
        for entry in list.iter().filter(|e| e["particle"] == "wax_off") {
            for rule in entry["block_state_provider"]["rules"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let then = &rule["then"];
                if then["type"] == "minecraft:copy_properties" && then["source"]["id"] == block {
                    return first_block(&rule["if_true"]["blocks"]);
                }
            }
        }
        None
    }
}

fn first_block(blocks: &Value) -> Option<String> {
    match blocks {
        Value::String(s) => Some(s.clone()),
        Value::Array(a) => a.first()?.as_str().map(str::to_string),
        _ => None,
    }
}

/// A predicate of a transformer rule for `block`, with air above it.
fn test(predicate: &Value, block: &str) -> bool {
    match predicate["type"].as_str() {
        Some("minecraft:matching_blocks") => match &predicate["blocks"] {
            Value::String(s) => s == block,
            Value::Array(a) => a.iter().any(|b| b == block),
            _ => false,
        },
        Some("minecraft:matching_block_tag") => predicate["tag"] == "minecraft:air",
        Some("minecraft:all_of") => predicate["predicates"]
            .as_array()
            .is_some_and(|all| all.iter().all(|p| test(p, block))),
        Some("minecraft:any_of") => predicate["predicates"]
            .as_array()
            .is_some_and(|any| any.iter().any(|p| test(p, block))),
        _ => false,
    }
}

/// The block a state provider gives for `block`.
fn provide(provider: &Value, block: &str) -> Option<String> {
    match provider["type"].as_str() {
        Some("minecraft:rule_based") => provider["rules"]
            .as_array()?
            .iter()
            .find(|r| test(&r["if_true"], block))
            .and_then(|r| provide(&r["then"], block)),
        Some("minecraft:copy_properties") => provide(&provider["source"], block),
        _ => provider["id"]
            .as_str()
            .or_else(|| provider["Name"].as_str())
            .map(str::to_string),
    }
}

/// The events a `sounds.json` names, in `namespace`.
fn sound_events(namespace: &str, text: &str) -> Vec<String> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()))
        .unwrap_or_default()
        .into_iter()
        .map(|k| format!("{namespace}:{k}"))
        .collect()
}

impl TestHost {
    /// Builds the simulated game's tables from the vanilla stand-ins and the entries mods
    /// registered, and reads the sound events of the content folders. Runs at the end of boot,
    /// after the content folders are written.
    pub fn build_content(&mut self) {
        let mut sim = vanilla();
        sim.apply(&self.content_tables());
        for dir in &self.sim.block_dirs {
            let Ok(namespaces) = std::fs::read_dir(dir.join("assets")) else {
                continue;
            };
            for ns in namespaces.flatten() {
                let Some(name) = ns.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                if let Ok(text) = std::fs::read_to_string(ns.path().join("sounds.json")) {
                    sim.known_sounds.extend(sound_events(&name, &text));
                }
            }
        }
        self.sim.content = sim;
    }

    /// The content table entries of the mods that are on.
    pub fn content_tables(&self) -> ContentTables {
        self.rules_dispatch().content_tables()
    }

    /// The vibration frequency of the game event `event`: a mod's, else the game's own.
    pub fn sculk_frequency(&self, event: &str) -> u8 {
        self.rules_dispatch()
            .sculk_frequency(event)
            .unwrap_or_else(|| {
                VANILLA_FREQUENCIES
                    .iter()
                    .find(|(e, _)| *e == event)
                    .map_or(0, |(_, f)| *f)
            })
    }

    /// The sound events mods registered.
    pub fn sound_defs(&self) -> Vec<SoundDef> {
        let mut out = Vec::new();
        self.rules_dispatch().sound_events(&mut out);
        out
    }

    /// The particle types mods registered.
    pub fn particle_defs(&self) -> Vec<ParticleDef> {
        let mut out = Vec::new();
        self.rules_dispatch().particle_types(&mut out);
        out
    }

    /// Uses `tool` (`axe`, `hoe`, `shovel` or `honeycomb`) on `block` and keeps what it became.
    pub fn use_tool(&mut self, tool: &str, block: &str) -> Option<String> {
        let out = self.sim.content.use_tool(tool, block);
        self.sim.content.tool_result = out.clone().unwrap_or_else(|| "none".to_string());
        out
    }

    /// Checks the content expectations of a script step.
    pub(crate) fn check_content(&self, step: usize, expect: &Expect) -> Option<ScriptFailure> {
        let fail = |what: &str, expected: String, actual: String| {
            Some(ScriptFailure {
                step,
                what: what.to_string(),
                expected,
                actual,
            })
        };
        let content = &self.sim.content;
        for (item, want) in &expect.fuels {
            let actual = content.fuel_ticks(item);
            if actual != *want {
                return fail(
                    "expect.fuels",
                    format!("{item} burns {want}"),
                    actual.to_string(),
                );
            }
        }
        for (item, want) in &expect.compostables {
            let actual = content.compost_chance(item);
            if (actual - *want).abs() > 0.005 {
                return fail(
                    "expect.compostables",
                    format!("{item} composts with {want}"),
                    actual.to_string(),
                );
            }
        }
        for (block, want) in &expect.flammables {
            let actual = content.flammables.get(block).copied().unwrap_or((0, 0));
            if [actual.0, actual.1] != *want {
                return fail(
                    "expect.flammables",
                    format!("{block} odds {want:?}"),
                    format!("{actual:?}"),
                );
            }
        }
        for (block, want) in &expect.path_types {
            let actual = content
                .path_types
                .get(block)
                .map_or("BLOCKED", String::as_str);
            if actual != want {
                return fail(
                    "expect.path_types",
                    format!("{block} is {want}"),
                    actual.to_string(),
                );
            }
        }
        for (event, want) in &expect.sculk_frequencies {
            let actual = self.sculk_frequency(event);
            if actual != *want {
                return fail(
                    "expect.sculk_frequencies",
                    format!("{event} vibrates at {want}"),
                    actual.to_string(),
                );
            }
        }
        if !expect.sounds.is_empty() {
            let ids: Vec<String> = self.sound_defs().into_iter().map(|d| d.id).collect();
            if let Some(want) = expect.sounds.iter().find(|w| !ids.contains(w)) {
                return fail(
                    "expect.sounds",
                    format!("sound event {want} registered"),
                    format!("{ids:?}"),
                );
            }
        }
        if !expect.particles.is_empty() {
            let ids: Vec<String> = self.particle_defs().into_iter().map(|d| d.id).collect();
            if let Some(want) = expect.particles.iter().find(|w| !ids.contains(w)) {
                return fail(
                    "expect.particles",
                    format!("particle type {want} registered"),
                    format!("{ids:?}"),
                );
            }
        }
        if let Some(want) = expect
            .played_sounds
            .iter()
            .find(|w| !content.played_sounds.iter().any(|p| p.contains(w.as_str())))
        {
            return fail(
                "expect.played_sounds",
                format!("a played sound containing {want:?}"),
                format!("{:?}", content.played_sounds),
            );
        }
        if let Some(want) = &expect.tool_result
            && &content.tool_result != want
        {
            return fail(
                "expect.tool_result",
                want.clone(),
                content.tool_result.clone(),
            );
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fish_hooks::{CompostDef, FlammableDef, FuelDef, PathTypeDef};

    fn tables() -> ContentTables {
        let def = |kind, from: &str, to: &str| TransformDef {
            kind,
            from: from.into(),
            to: to.into(),
            mod_id: "m".into(),
        };
        ContentTables {
            fuels: vec![
                FuelDef {
                    item: "m:ruby".into(),
                    mod_id: "m".into(),
                    burn_ticks: 2400,
                },
                FuelDef {
                    item: "minecraft:stick".into(),
                    mod_id: "m".into(),
                    burn_ticks: 0,
                },
            ],
            compostables: vec![CompostDef {
                item: "m:snack".into(),
                mod_id: "m".into(),
                chance: 0.5,
            }],
            flammables: vec![
                FlammableDef {
                    block: "m:log".into(),
                    mod_id: "m".into(),
                    ignite_odds: 5,
                    burn_odds: 5,
                },
                FlammableDef {
                    block: "minecraft:oak_planks".into(),
                    mod_id: "m".into(),
                    ignite_odds: 0,
                    burn_odds: 0,
                },
            ],
            transforms: vec![
                def(TransformKind::Strip, "m:log", "m:stripped_log"),
                def(TransformKind::Wax, "m:block", "m:waxed_block"),
                def(TransformKind::Till, "m:soil", "minecraft:farmland"),
            ],
            path_types: vec![PathTypeDef {
                block: "m:thorns".into(),
                mod_id: "m".into(),
                path_type: "DAMAGING".into(),
            }],
            sculk_frequencies: Vec::new(),
        }
    }

    #[test]
    fn the_vanilla_tables_answer_like_the_game() {
        let sim = vanilla();
        assert_eq!(sim.fuel_ticks("minecraft:coal"), 1600);
        assert_eq!(sim.fuel_ticks("minecraft:diamond"), 0);
        assert!((sim.compost_chance("minecraft:apple") - 0.65).abs() < 0.001);
        assert_eq!(sim.compost_chance("minecraft:stone"), 0.0);
        assert_eq!(
            sim.use_tool("axe", "minecraft:oak_log").as_deref(),
            Some("minecraft:stripped_oak_log")
        );
        assert_eq!(
            sim.use_tool("axe", "minecraft:waxed_copper_block")
                .as_deref(),
            Some("minecraft:copper_block")
        );
        assert_eq!(
            sim.use_tool("honeycomb", "minecraft:copper_block")
                .as_deref(),
            Some("minecraft:waxed_copper_block")
        );
        assert_eq!(
            sim.use_tool("hoe", "minecraft:dirt").as_deref(),
            Some("minecraft:farmland")
        );
        assert_eq!(sim.use_tool("pickaxe", "minecraft:dirt"), None);
        assert_eq!(sim.use_tool("axe", "minecraft:stone"), None);
    }

    #[test]
    fn the_entries_of_mods_change_the_tables() {
        let mut sim = vanilla();
        sim.apply(&tables());
        assert_eq!(sim.fuel_ticks("m:ruby"), 2400);
        assert_eq!(sim.fuel_ticks("minecraft:stick"), 0);
        assert!((sim.compost_chance("m:snack") - 0.5).abs() < 0.001);
        assert_eq!(sim.flammables.get("m:log"), Some(&(5, 5)));
        assert!(!sim.flammables.contains_key("minecraft:oak_planks"));
        assert_eq!(
            sim.use_tool("axe", "m:log").as_deref(),
            Some("m:stripped_log")
        );
        assert_eq!(
            sim.use_tool("honeycomb", "m:block").as_deref(),
            Some("m:waxed_block")
        );
        assert_eq!(
            sim.use_tool("axe", "m:waxed_block").as_deref(),
            Some("m:block")
        );
        assert_eq!(
            sim.use_tool("hoe", "m:soil").as_deref(),
            Some("minecraft:farmland")
        );
        assert_eq!(
            sim.path_types.get("m:thorns").map(String::as_str),
            Some("DAMAGING")
        );
        // The vanilla entries stay first.
        assert_eq!(
            sim.use_tool("axe", "minecraft:oak_log").as_deref(),
            Some("minecraft:stripped_oak_log")
        );
    }

    #[test]
    fn sounds_json_names_its_events_in_the_namespace() {
        assert_eq!(
            sound_events("m", r#"{"a": {"sounds": []}, "b.c": {}}"#),
            ["m:a", "m:b.c"]
        );
        assert!(sound_events("m", "not json").is_empty());
    }
}
