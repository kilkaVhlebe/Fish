//! Fish items in the simulation: the events the game raises for using an item, eating it, holding
//! it in the inventory and building its tooltip, and the creative inventory the loader builds from
//! the tabs mods registered.

use fish_hooks::{Dispatch, ItemDef, ItemEvent, ItemEventKind, Kind, Outcome};
use serde_json::{Value, json};

use crate::TestHost;
use crate::events::{OVERWORLD, PLAYER, outcome_name, uuid_of};
use crate::script::{Expect, ScriptFailure};

/// One tab of the creative inventory the simulated game shows (or hides: see `visible`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreativeTab {
    /// The tab's id, `namespace:path`.
    pub id: String,
    /// The text on the tab.
    pub title: String,
    /// The id of the item shown on the tab.
    pub icon: String,
    /// `TOP` or `BOTTOM`.
    pub row: String,
    /// The tab's column in its row.
    pub column: u32,
    /// False for tabs the screen does not show (the hotbar tab and tabs marked `visible: false`).
    pub visible: bool,
    /// The ids of the items the tab lists, in order.
    pub entries: Vec<String>,
}

/// The item id of a stack in the creative inventory: `{count:1,id:"minecraft:stone"}` or a bare id.
fn stack_id(text: &str) -> String {
    let text = text.trim();
    if !text.starts_with('{') {
        return text.to_string();
    }
    let Some(at) = text.find("id:") else {
        return text.to_string();
    };
    let rest = text[at + 3..].trim_start();
    match rest.chars().next() {
        Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or("").to_string(),
        _ => rest
            .split([',', '}'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
    }
}

fn stack_text(id: &str) -> String {
    format!("{{count:1,id:\"{id}\"}}")
}

/// A small stand-in for the vanilla `creative-inventory.json` the game's resources hold.
fn vanilla_creative() -> Value {
    let tab = |id: &str, title: &str, icon: &str, row: &str, column: u32, items: &[&str]| {
        let stacks: Vec<String> = items.iter().map(|i| stack_text(i)).collect();
        json!({
            "id": id,
            "title": title,
            "icon": stack_text(icon),
            "type": "CATEGORY",
            "row": row,
            "column": column,
            "items": stacks,
        })
    };
    let mut search = tab(
        "minecraft:search",
        "Search Items",
        "minecraft:compass",
        "TOP",
        6,
        &["minecraft:stone", "minecraft:iron_ingot"],
    );
    search["type"] = json!("SEARCH");
    search["lines"] = json!([[], []]);
    search["tags"] = json!([[], ["minecraft:iron_ingots"]]);
    let mut hotbar = tab(
        "minecraft:hotbar",
        "Saved Hotbars",
        "minecraft:bookshelf",
        "TOP",
        5,
        &[],
    );
    hotbar["type"] = json!("HOTBAR");
    let mut op = tab(
        "minecraft:op_blocks",
        "Operator Utilities",
        "minecraft:command_block",
        "BOTTOM",
        5,
        &[],
    );
    op["visible"] = json!(false);
    json!({
        "tabs": [
            tab(
                "minecraft:building_blocks",
                "Building Blocks",
                "minecraft:bricks",
                "TOP",
                0,
                &["minecraft:stone"],
            ),
            tab(
                "minecraft:ingredients",
                "Ingredients",
                "minecraft:iron_ingot",
                "BOTTOM",
                3,
                &["minecraft:iron_ingot"],
            ),
            search,
            hotbar,
            op,
        ]
    })
}

impl TestHost {
    /// Asks the loader to patch a vanilla creative inventory fixture and keeps the result. Runs at
    /// the end of boot, and again when a test wants to see the effect of a mod that stopped.
    pub fn build_creative(&mut self) {
        let fixture = vanilla_creative().to_string();
        let patched = self.rules_dispatch().creative_inventory(&fixture);
        self.sim.creative_json = patched.unwrap_or(fixture);
    }

    /// Every item a mod registered.
    pub fn item_defs(&self) -> Vec<ItemDef> {
        let mut defs = Vec::new();
        self.rules_dispatch().items(&mut defs);
        defs
    }

    /// The creative inventory the loader built (`creative-inventory.json`).
    ///
    /// # Panics
    /// Never in practice: the text is produced by the loader and the fixture at boot.
    pub fn creative(&self) -> Value {
        serde_json::from_str(&self.sim.creative_json).expect("the creative inventory is JSON")
    }

    /// The tabs of the creative inventory, in file order.
    pub fn creative_tabs(&self) -> Vec<CreativeTab> {
        let json = self.creative();
        let text = |t: &Value, key: &str| t[key].as_str().unwrap_or("").to_string();
        json["tabs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| CreativeTab {
                id: text(t, "id"),
                title: text(t, "title"),
                icon: stack_id(&text(t, "icon")),
                row: text(t, "row"),
                column: t["column"]
                    .as_u64()
                    .and_then(|c| u32::try_from(c).ok())
                    .unwrap_or(0),
                visible: t["visible"] != false && t["type"] != "HOTBAR",
                entries: t["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(stack_id)
                    .collect(),
            })
            .collect()
    }

    /// Raises one item event in the server's game (the integrated one on a client host) and
    /// records it in `sim.events` as `item/<name> <outcome>`.
    pub fn item_event(&mut self, mut event: ItemEvent<'_>) -> Outcome {
        event.role = self.server_role();
        let role = event.role;
        let (mut view, dispatch) = self.view(role);
        let outcome = dispatch.item_event(Some(&mut view), &event);
        self.sim.events.push(format!(
            "item/{} {}",
            event.kind.name(),
            outcome_name(&outcome)
        ));
        self.drain_trace();
        outcome
    }

    /// True when `item` is an item a mod registered.
    pub fn is_fish_item(&self, item: &str) -> bool {
        self.rules_dispatch().is_item(item)
    }

    /// The player uses `count` of `item` in the air (hand `hand`: 0 main, 1 off). The generic
    /// `use_item` event comes first (a cancel ends it); then a Fish item's own `Use`. Returns the
    /// final outcome, also kept for `expect.use_result`.
    pub fn use_item(&mut self, item: &str, count: u32, hand: u8) -> Outcome {
        let mut e = Self::at(Kind::UseItem);
        e.item = item;
        e.hand = hand;
        let generic = self.game_event(&e);
        let outcome = if generic.cancelled() {
            generic
        } else if self.is_fish_item(item) {
            match self.item_event(player_event(ItemEventKind::Use, item, count, hand)) {
                Outcome::Pass => generic,
                decided => decided,
            }
        } else {
            generic
        };
        self.sim.last_use = outcome_name(&outcome);
        outcome
    }

    /// The player uses `item` (hand `hand`) on entity `entity_id` of type `entity_type`: the generic
    /// `use_entity` event, then the Fish item's `UseOnEntity`, then its `Use` when that passed.
    /// Returns the final outcome, also kept for `expect.use_result`.
    pub fn use_item_on_entity(
        &mut self,
        entity_id: i64,
        entity_type: &str,
        item: &str,
        hand: u8,
    ) -> Outcome {
        let mut e = Self::at(Kind::UseEntity);
        e.entity_id = entity_id;
        e.subject = entity_type;
        e.item = item;
        e.hand = hand;
        let generic = self.game_event(&e);
        let outcome = if generic.cancelled() {
            generic
        } else if self.is_fish_item(item) {
            let mut ev = player_event(ItemEventKind::UseOnEntity, item, 1, hand);
            ev.target = entity_type;
            ev.entity_id = entity_id;
            let mut outcome = self.item_event(ev);
            if outcome == Outcome::Pass {
                outcome = self.item_event(player_event(ItemEventKind::Use, item, 1, hand));
            }
            if outcome == Outcome::Pass {
                generic
            } else {
                outcome
            }
        } else {
            generic
        };
        self.sim.last_use = outcome_name(&outcome);
        outcome
    }

    /// The player finished eating or drinking `count` of `item`.
    pub fn finish_using(&mut self, item: &str, count: u32, hand: u8) -> Outcome {
        self.item_event(player_event(ItemEventKind::FinishUsing, item, count, hand))
    }

    /// One tick of `count` of `item` sitting in inventory slot `slot` (see `fish_abi::items::slot`).
    pub fn inventory_tick(&mut self, item: &str, count: u32, slot: i32, selected: bool) -> Outcome {
        let mut ev = player_event(ItemEventKind::InventoryTick, item, count, 0);
        ev.slot = slot;
        ev.selected = selected;
        self.item_event(ev)
    }

    /// The extra tooltip lines the mod of `item` adds for a stack of `count` (client only); empty
    /// for items no mod registered.
    pub fn item_tooltip(&mut self, item: &str, count: u32) -> Vec<String> {
        let lines = self.tooltip_lines(item, count);
        if self.is_fish_item(item) {
            self.sim.events.push("item/append_tooltip pass".to_string());
        }
        self.drain_trace();
        lines
    }

    fn tooltip_lines(&self, item: &str, count: u32) -> Vec<String> {
        let mut lines = Vec::new();
        self.rules_dispatch().item_tooltip(item, count, &mut lines);
        lines
    }

    /// The first failed item expectation of `expect`, for step `step`.
    pub(crate) fn check_items(&self, step: usize, expect: &Expect) -> Option<ScriptFailure> {
        let fail = |what: &str, expected: String, actual: String| ScriptFailure {
            step,
            what: what.to_string(),
            expected,
            actual,
        };
        if !expect.items.is_empty() {
            let ids: Vec<String> = self.item_defs().into_iter().map(|d| d.id).collect();
            for want in &expect.items {
                if !ids.contains(want) {
                    return Some(fail(
                        "expect.items",
                        format!("item {want} registered"),
                        format!("items {ids:?}"),
                    ));
                }
            }
        }
        for (item, want) in &expect.tooltips {
            let actual = self.tooltip_lines(item, 1);
            if &actual != want {
                return Some(fail(
                    "expect.tooltips",
                    format!("{item} tooltip {want:?}"),
                    format!("{actual:?}"),
                ));
            }
        }
        let tabs = self.creative_tabs();
        for want in &expect.creative_tabs {
            if !tabs.iter().any(|t| &t.id == want) {
                let ids: Vec<&str> = tabs.iter().map(|t| t.id.as_str()).collect();
                return Some(fail(
                    "expect.creative_tabs",
                    format!("creative tab {want}"),
                    format!("tabs {ids:?}"),
                ));
            }
        }
        for (tab, want) in &expect.creative_entries {
            let Some(found) = tabs.iter().find(|t| &t.id == tab) else {
                return Some(fail(
                    "expect.creative_entries",
                    format!("creative tab {tab}"),
                    format!(
                        "tabs {:?}",
                        tabs.iter().map(|t| t.id.as_str()).collect::<Vec<_>>()
                    ),
                ));
            };
            let mut from = 0;
            for id in want {
                match found.entries[from..].iter().position(|e| e == id) {
                    Some(at) => from += at + 1,
                    None => {
                        return Some(fail(
                            "expect.creative_entries",
                            format!("{tab} entries containing {want:?} in that order"),
                            format!("{:?}", found.entries),
                        ));
                    }
                }
            }
        }
        for (tab, unwanted) in &expect.no_creative_entries {
            let Some(found) = tabs.iter().find(|t| &t.id == tab) else {
                continue;
            };
            if let Some(id) = unwanted.iter().find(|id| found.entries.contains(id)) {
                return Some(fail(
                    "expect.no_creative_entries",
                    format!("no {id} in {tab}"),
                    format!("{:?}", found.entries),
                ));
            }
        }
        None
    }
}

/// An event of `kind` for `count` of `item`, caused by the player.
pub(crate) fn player_event(kind: ItemEventKind, item: &str, count: u32, hand: u8) -> ItemEvent<'_> {
    let mut e = ItemEvent::new(kind, item, count);
    e.hand = hand;
    e.actor = PLAYER;
    e.actor_uuid = uuid_of(PLAYER);
    e.dimension = OVERWORLD;
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_ids_are_read_from_snbt_and_bare_ids() {
        assert_eq!(
            stack_id("{count:1,id:\"minecraft:stone\"}"),
            "minecraft:stone"
        );
        assert_eq!(stack_id("{id:\"a:b\",count:3}"), "a:b");
        assert_eq!(stack_id("{id:'a:b'}"), "a:b");
        assert_eq!(stack_id("{id:a:b,count:3}"), "a:b");
        assert_eq!(stack_id(" a:b "), "a:b");
        assert_eq!(stack_id("{count:1}"), "{count:1}");
    }

    #[test]
    fn the_fixture_holds_the_vanilla_tabs() {
        let v = vanilla_creative();
        let ids: Vec<&str> = v["tabs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "minecraft:building_blocks",
                "minecraft:ingredients",
                "minecraft:search",
                "minecraft:hotbar",
                "minecraft:op_blocks"
            ]
        );
    }
}
