//! The items and creative tabs mods registered, the Horizon declarations the loader writes for the
//! items, and the rewrite of the game's creative inventory.

use std::collections::HashMap;
use std::sync::Arc;

use fish_abi::items::ItemHandlerFn;
use fish_hooks::{ItemDef, ItemGroupAddition, ItemGroupDef};
use serde::Serialize;
use serde_json::{Value, json};

use crate::blocks::{UserBox, check_minecraft};

/// One registered item.
pub(crate) struct ItemEntry {
    /// What the item is.
    pub def: ItemDef,
    /// Token of the registering mod.
    pub owner: u64,
    /// The mod's handler, if it wants events.
    pub handler: Option<ItemHandlerFn>,
    /// The handler's user pointer.
    pub user: UserBox,
}

/// Every item a loader knows.
#[derive(Default)]
pub(crate) struct ItemStore {
    items: Vec<Arc<ItemEntry>>,
    by_id: HashMap<String, Arc<ItemEntry>>,
    /// The union of the events of every item.
    mask: u32,
}

impl ItemStore {
    /// Adds an item; `Err` holds the token of the mod that registered the id first.
    pub(crate) fn register(&mut self, entry: ItemEntry) -> Result<(), u64> {
        if let Some(first) = self.by_id.get(&entry.def.id) {
            return Err(first.owner);
        }
        self.mask |= entry.def.events;
        let entry = Arc::new(entry);
        self.by_id.insert(entry.def.id.clone(), entry.clone());
        self.items.push(entry);
        Ok(())
    }

    /// The items in registration order.
    pub(crate) fn entries(&self) -> Vec<Arc<ItemEntry>> {
        self.items.clone()
    }

    /// The item `id`.
    pub(crate) fn find(&self, id: &str) -> Option<Arc<ItemEntry>> {
        self.by_id.get(id).cloned()
    }

    /// True when an item `id` exists.
    pub(crate) fn has(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }

    /// True when `token` registered an item.
    pub(crate) fn has_items_of(&self, token: u64) -> bool {
        self.items.iter().any(|i| i.owner == token)
    }

    /// The union of the `1 << event` bits of every item.
    pub(crate) fn event_mask(&self) -> u32 {
        self.mask
    }

    /// Drops every item (and so every user pointer).
    pub(crate) fn clear(&mut self) {
        self.by_id.clear();
        self.items.clear();
        self.mask = 0;
    }
}

/// Every creative tab and tab addition a loader knows.
#[derive(Default)]
pub(crate) struct GroupStore {
    groups: Vec<(ItemGroupDef, u64)>,
    additions: Vec<(ItemGroupAddition, u64)>,
}

impl GroupStore {
    /// Adds a tab; `Err` holds the token of the mod that registered the id first.
    pub(crate) fn register(&mut self, def: ItemGroupDef, owner: u64) -> Result<(), u64> {
        if let Some((_, first)) = self.groups.iter().find(|(g, _)| g.id == def.id) {
            return Err(*first);
        }
        self.groups.push((def, owner));
        Ok(())
    }

    /// Adds items to a tab.
    pub(crate) fn add(&mut self, addition: ItemGroupAddition, owner: u64) {
        self.additions.push((addition, owner));
    }

    /// The tabs and additions, in registration order, of the mods `live` accepts.
    pub(crate) fn live(
        &self,
        live: impl Fn(u64) -> bool,
    ) -> (Vec<ItemGroupDef>, Vec<ItemGroupAddition>) {
        (
            self.groups
                .iter()
                .filter(|(_, o)| live(*o))
                .map(|(g, _)| g.clone())
                .collect(),
            self.additions
                .iter()
                .filter(|(_, o)| live(*o))
                .map(|(a, _)| a.clone())
                .collect(),
        )
    }

    /// Drops every tab and addition.
    pub(crate) fn clear(&mut self) {
        self.groups.clear();
        self.additions.clear();
    }
}

/// Checks the template and the numbers of `def`; `Err` is the reason.
pub(crate) fn check_item_def(def: &ItemDef) -> Result<(), String> {
    check_minecraft("template", &def.template)?;
    if let Some(n) = def.max_stack
        && !(1..=99).contains(&n)
    {
        return Err(format!("the stack size {n} is not between 1 and 99"));
    }
    Ok(())
}

#[derive(Serialize)]
struct ItemJson<'a> {
    template: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_stack_size: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rarity: Option<&'a str>,
}

/// The bytes of Horizon's `horizon/item/<path>.json` for `def`.
pub(crate) fn item_json(def: &ItemDef) -> Vec<u8> {
    let json = ItemJson {
        template: &def.template,
        max_stack_size: def.max_stack,
        rarity: def.rarity.as_deref(),
    };
    let mut bytes = serde_json::to_vec_pretty(&json).expect("an item serializes");
    bytes.push(b'\n');
    bytes
}

/// True when `id` is `namespace:path` made of what Horizon's identifiers allow.
pub(crate) fn valid_item_ref(id: &str) -> bool {
    let Some((namespace, path)) = id.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && namespace
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-'))
        && !path.is_empty()
        && path
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'/' | b'-'))
}

/// The most characters a tab title may have.
pub(crate) const TITLE_MAX: usize = 64;

/// `ruby_tab` becomes `Ruby Tab`.
fn title_of(id: &str) -> String {
    let path = id.split_once(':').map_or(id, |(_, p)| p);
    path.split(['_', '/', '-', '.'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The stack text Horizon's creative export uses for an item.
fn stack_text(id: &str) -> String {
    format!("{{count:1,id:\"{id}\"}}")
}

/// The item id of a stack text of the export (`{count:1,id:"minecraft:stone"}` or a bare id).
fn stack_id(text: &str) -> &str {
    match text.split_once("id:\"") {
        Some((_, rest)) => rest.split('"').next().unwrap_or(rest),
        None => text,
    }
}

/// What [`patch_creative`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Patched {
    /// Fish tabs that got a place on the screen.
    pub shown: Vec<String>,
    /// Fish tabs that found no free place.
    pub hidden: Vec<String>,
    /// Additions that reached their tab.
    pub applied: usize,
    /// Tabs additions named that do not exist, with the mod that named them.
    pub unknown: Vec<(String, String)>,
    /// Entries left out because they name an item of a Fish mod that does not exist: (tab, item).
    pub skipped: Vec<(String, String)>,
}

const ROWS: [&str; 2] = ["TOP", "BOTTOM"];

/// The places on the creative screen no tab Horizon shows holds: (row, column).
fn free_slots(tabs: &[Value]) -> Vec<(&'static str, u64)> {
    let mut free = Vec::new();
    for row in ROWS {
        for column in 0..7 {
            let held = tabs.iter().any(|t| {
                t["row"] == row
                    && t["column"] == column
                    && t["type"] != "HOTBAR"
                    && t["visible"] != false
            });
            if !held {
                free.push((row, column));
            }
        }
    }
    free
}

/// Rewrites the text of `creative-inventory.json`: the tabs of `groups` take the free places of
/// the screen, `additions` append entries to existing tabs, and every entry reaches the search tab.
/// `known` says whether an item of a Fish mod exists (`Some(true)`), does not (`Some(false)`) or
/// belongs to a namespace no Fish mod owns (`None`).
pub(crate) fn patch_creative(
    json: &str,
    groups: &[ItemGroupDef],
    additions: &[ItemGroupAddition],
    known: impl Fn(&str) -> Option<bool>,
) -> Result<(String, Patched), String> {
    let mut doc: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let tabs = doc
        .get_mut("tabs")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "the document has no tabs".to_string())?;
    let mut report = Patched::default();
    let mut entered: Vec<String> = Vec::new();
    let keep = |tab: &str, entries: &[String], report: &mut Patched| -> Vec<String> {
        let mut kept = Vec::new();
        for e in entries {
            if known(e) == Some(false) {
                report.skipped.push((tab.to_string(), e.clone()));
            } else if !kept.contains(e) {
                kept.push(e.clone());
            }
        }
        kept
    };

    let mut sorted: Vec<&ItemGroupDef> = groups.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut free = free_slots(tabs).into_iter();
    let mut new_tabs = Vec::new();
    for g in sorted {
        let entries = keep(&g.id, &g.entries, &mut report);
        entered.extend(entries.iter().cloned());
        let Some((row, column)) = free.next() else {
            report.hidden.push(g.id.clone());
            continue;
        };
        let title = if g.title.is_empty() {
            title_of(&g.id)
        } else {
            g.title.clone()
        };
        new_tabs.push(json!({
            "id": g.id,
            "type": "CATEGORY",
            "row": row,
            "column": column,
            "title": title,
            "icon": stack_text(&g.icon),
            "visible": true,
            "items": entries.iter().map(|e| stack_text(e)).collect::<Vec<_>>(),
        }));
        report.shown.push(g.id.clone());
    }
    let at = tabs
        .iter()
        .position(|t| t["type"] == "INVENTORY")
        .unwrap_or(tabs.len());
    for (i, tab) in new_tabs.into_iter().enumerate() {
        tabs.insert(at + i, tab);
    }

    for a in additions {
        let entries = keep(&a.group, &a.entries, &mut report);
        let Some(tab) = tabs
            .iter_mut()
            .find(|t| t["id"] == a.group && t["type"] != "HOTBAR")
        else {
            report.unknown.push((a.group.clone(), a.mod_id.clone()));
            continue;
        };
        let Some(items) = tab.get_mut("items").and_then(Value::as_array_mut) else {
            report.unknown.push((a.group.clone(), a.mod_id.clone()));
            continue;
        };
        for e in &entries {
            if !items
                .iter()
                .any(|i| i.as_str().is_some_and(|s| stack_id(s) == e))
            {
                items.push(Value::String(stack_text(e)));
            }
        }
        entered.extend(entries);
        report.applied += 1;
    }

    // The search tab lists every item of every tab.
    if let Some(search) = tabs.iter_mut().find(|t| t["type"] == "SEARCH")
        && let Some(items) = search.get_mut("items").and_then(Value::as_array_mut)
    {
        for e in entered {
            if !items
                .iter()
                .any(|i| i.as_str().is_some_and(|s| stack_id(s) == e))
            {
                items.push(Value::String(stack_text(&e)));
            }
        }
    }
    Ok((doc.to_string(), report))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The creative inventory of Horizon: every place of the screen is taken but two (the hotbar's
    /// and the operator tab's, which is invisible).
    fn vanilla() -> String {
        let mut doc = json!({
            "schema": 1,
            "tabs": [
                {"id": "minecraft:building_blocks", "type": "CATEGORY", "row": "TOP", "column": 0,
                 "title": "Building Blocks", "icon": "{count:1,id:\"minecraft:bricks\"}",
                 "visible": true, "items": ["{count:1,id:\"minecraft:stone\"}"]},
                {"id": "minecraft:hotbar", "type": "HOTBAR", "row": "TOP", "column": 5,
                 "title": "Saved Hotbars", "icon": "{count:1,id:\"minecraft:bookshelf\"}",
                 "visible": true, "items": []},
                {"id": "minecraft:search", "type": "SEARCH", "row": "TOP", "column": 6,
                 "title": "Search Items", "icon": "{count:1,id:\"minecraft:compass\"}",
                 "visible": true, "items": ["{count:1,id:\"minecraft:stone\"}"],
                 "lines": [["Stone"]], "tags": [[]]},
                {"id": "minecraft:op_blocks", "type": "CATEGORY", "row": "BOTTOM", "column": 5,
                 "title": "Operator Utilities", "icon": "{count:1,id:\"minecraft:command_block\"}",
                 "visible": false, "items": []},
                {"id": "minecraft:inventory", "type": "INVENTORY", "row": "BOTTOM", "column": 6,
                 "title": "Survival Inventory", "icon": "{count:1,id:\"minecraft:chest\"}",
                 "visible": true, "items": []},
            ]
        });
        let tabs = doc["tabs"].as_array_mut().unwrap();
        let mut filler = Vec::new();
        for (row, columns) in [("TOP", 1..5), ("BOTTOM", 0..5)] {
            for column in columns {
                filler.push(json!({
                    "id": format!("minecraft:filler_{row}_{column}"), "type": "CATEGORY",
                    "row": row, "column": column, "title": "Filler",
                    "icon": "{count:1,id:\"minecraft:stone\"}", "visible": true, "items": []
                }));
            }
        }
        tabs.splice(1..1, filler);
        doc.to_string()
    }

    fn group(id: &str, entries: &[&str]) -> ItemGroupDef {
        ItemGroupDef {
            id: id.into(),
            mod_id: "ruby".into(),
            title: String::new(),
            icon: "ruby:gem".into(),
            entries: entries.iter().map(|e| (*e).to_string()).collect(),
        }
    }

    fn addition(group: &str, entries: &[&str]) -> ItemGroupAddition {
        ItemGroupAddition {
            group: group.into(),
            mod_id: "ruby".into(),
            entries: entries.iter().map(|e| (*e).to_string()).collect(),
        }
    }

    fn tabs(text: &str) -> Vec<Value> {
        serde_json::from_str::<Value>(text).unwrap()["tabs"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn ids(tab: &Value) -> Vec<String> {
        tab["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| stack_id(i.as_str().unwrap()).to_string())
            .collect()
    }

    fn all_known(_: &str) -> Option<bool> {
        None
    }

    #[test]
    fn stack_texts_and_ids_round_trip() {
        assert_eq!(stack_text("a:b"), "{count:1,id:\"a:b\"}");
        assert_eq!(stack_id(&stack_text("a:b")), "a:b");
        assert_eq!(stack_id("a:b"), "a:b");
        assert_eq!(title_of("ruby:ruby_tab"), "Ruby Tab");
        assert_eq!(title_of("ruby:a/b-c"), "A B C");
        assert_eq!(title_of("x:"), "");
    }

    #[test]
    fn item_refs_are_namespace_and_path() {
        for ok in ["minecraft:stone", "a.b-c:d/e_f", "ruby:ruby_block"] {
            assert!(valid_item_ref(ok), "{ok}");
        }
        for bad in ["stone", ":stone", "a:", "A:b", "a:B", "a b:c"] {
            assert!(!valid_item_ref(bad), "{bad}");
        }
    }

    #[test]
    fn free_slots_are_the_two_tabs_horizon_does_not_show() {
        let t = tabs(&vanilla());
        let free = free_slots(&t);
        assert!(free.contains(&("TOP", 5)) && free.contains(&("BOTTOM", 5)));
        assert!(!free.contains(&("TOP", 0)) && !free.contains(&("TOP", 6)));
        // the hotbar and the invisible operator tab hold nothing: TOP 5 comes before BOTTOM 5
        let pos = |s| free.iter().position(|f| *f == s).unwrap();
        assert!(pos(("TOP", 5)) < pos(("BOTTOM", 5)));
    }

    #[test]
    fn fish_tabs_take_free_places_in_id_order() {
        let groups = [
            group("ruby:b", &["ruby:gem", "ruby:gem", "minecraft:stone"]),
            group("ruby:a", &["ruby:rock"]),
            group("ruby:c", &["ruby:more"]),
        ];
        let (text, report) = patch_creative(&vanilla(), &groups, &[], all_known).unwrap();
        assert_eq!(report.shown, ["ruby:a", "ruby:b"]);
        assert_eq!(report.hidden, ["ruby:c"]);
        let t = tabs(&text);
        let a = t.iter().find(|t| t["id"] == "ruby:a").unwrap();
        let b = t.iter().find(|t| t["id"] == "ruby:b").unwrap();
        assert_eq!(
            (a["row"].as_str(), a["column"].as_u64()),
            (Some("TOP"), Some(5))
        );
        assert_eq!(
            (b["row"].as_str(), b["column"].as_u64()),
            (Some("BOTTOM"), Some(5))
        );
        assert_eq!(a["title"], "A");
        assert_eq!(a["icon"], "{count:1,id:\"ruby:gem\"}");
        assert_eq!(ids(b), ["ruby:gem", "minecraft:stone"]);
        // before the survival inventory
        assert_eq!(t.last().unwrap()["id"], "minecraft:inventory");
        // the search tab lists every entry once, the hidden tab's too
        let search = t.iter().find(|t| t["type"] == "SEARCH").unwrap();
        assert_eq!(
            ids(search),
            ["minecraft:stone", "ruby:rock", "ruby:gem", "ruby:more"]
        );
        assert!(t.iter().all(|t| t["id"] != "ruby:c"));
    }

    #[test]
    fn additions_extend_tabs_once_and_report_unknown_tabs() {
        let groups = [group("ruby:a", &["ruby:rock"])];
        let additions = [
            addition(
                "minecraft:building_blocks",
                &["ruby:gem", "minecraft:stone"],
            ),
            addition("ruby:a", &["ruby:rock", "ruby:more"]),
            addition("minecraft:nowhere", &["ruby:gem"]),
            addition("minecraft:hotbar", &["ruby:gem"]),
        ];
        let (text, report) = patch_creative(&vanilla(), &groups, &additions, all_known).unwrap();
        assert_eq!(report.applied, 2);
        assert_eq!(
            report.unknown,
            [
                ("minecraft:nowhere".to_string(), "ruby".to_string()),
                ("minecraft:hotbar".to_string(), "ruby".to_string())
            ]
        );
        let t = tabs(&text);
        let find = |id: &str| t.iter().find(|t| t["id"] == id).unwrap();
        assert_eq!(
            ids(find("minecraft:building_blocks")),
            ["minecraft:stone", "ruby:gem"]
        );
        assert_eq!(ids(find("ruby:a")), ["ruby:rock", "ruby:more"]);
        assert_eq!(
            ids(find("minecraft:search")),
            ["minecraft:stone", "ruby:rock", "ruby:gem", "ruby:more"]
        );
    }

    #[test]
    fn entries_naming_missing_fish_items_are_left_out() {
        let known = |id: &str| match id {
            "ruby:gem" => Some(true),
            "ruby:typo" => Some(false),
            _ => None,
        };
        let groups = [group(
            "ruby:a",
            &["ruby:gem", "ruby:typo", "minecraft:stone"],
        )];
        let additions = [addition("minecraft:building_blocks", &["ruby:typo"])];
        let (text, report) = patch_creative(&vanilla(), &groups, &additions, known).unwrap();
        assert_eq!(
            report.skipped,
            [
                ("ruby:a".to_string(), "ruby:typo".to_string()),
                (
                    "minecraft:building_blocks".to_string(),
                    "ruby:typo".to_string()
                )
            ]
        );
        let t = tabs(&text);
        assert!(t.iter().all(|t| !ids(t).contains(&"ruby:typo".to_string())));
    }

    #[test]
    fn broken_documents_are_errors() {
        assert!(patch_creative("nope", &[], &[], all_known).is_err());
        assert!(patch_creative("{}", &[], &[], all_known).is_err());
        // a tab without an item list cannot take additions
        let doc = json!({"tabs": [{"id": "a:b", "type": "CATEGORY"}]}).to_string();
        let (_, report) =
            patch_creative(&doc, &[], &[addition("a:b", &["x:y"])], all_known).unwrap();
        assert_eq!(report.unknown.len(), 1);
        // a document without a search tab still gets its tabs
        let (text, report) = patch_creative(&doc, &[group("ruby:a", &[])], &[], all_known).unwrap();
        assert_eq!(report.shown, ["ruby:a"]);
        assert_eq!(tabs(&text).len(), 2);
    }

    #[test]
    fn items_and_groups_register_once() {
        fn entry(id: &str, owner: u64, events: u32) -> ItemEntry {
            ItemEntry {
                def: ItemDef {
                    id: id.into(),
                    mod_id: "ruby".into(),
                    template: "minecraft:diamond".into(),
                    max_stack: None,
                    rarity: None,
                    events,
                },
                owner,
                handler: None,
                user: UserBox::new(std::ptr::null_mut(), None),
            }
        }
        let mut store = ItemStore::default();
        assert_eq!(store.event_mask(), 0);
        assert!(store.register(entry("ruby:a", 1, 0b101)).is_ok());
        assert!(store.register(entry("ruby:b", 2, 0b010)).is_ok());
        assert_eq!(store.register(entry("ruby:a", 3, 0)).unwrap_err(), 1);
        assert_eq!(store.event_mask(), 0b111);
        assert!(store.has("ruby:a") && !store.has("ruby:c"));
        assert!(store.has_items_of(2) && !store.has_items_of(3));
        assert_eq!(store.find("ruby:b").unwrap().owner, 2);
        assert_eq!(store.entries().len(), 2);
        store.clear();
        assert!(store.entries().is_empty() && store.event_mask() == 0 && !store.has("ruby:a"));

        let mut groups = GroupStore::default();
        assert!(groups.register(group("ruby:t", &[]), 1).is_ok());
        assert_eq!(groups.register(group("ruby:t", &[]), 2).unwrap_err(), 1);
        groups.add(addition("minecraft:combat", &["ruby:a"]), 1);
        groups.add(addition("minecraft:combat", &["ruby:b"]), 2);
        let (g, a) = groups.live(|o| o == 1);
        assert_eq!((g.len(), a.len()), (1, 1));
        let (g, a) = groups.live(|_| false);
        assert!(g.is_empty() && a.is_empty());
        groups.clear();
        assert!(groups.live(|_| true).0.is_empty());
    }

    #[test]
    fn item_definitions_are_checked_and_written() {
        let mut def = ItemDef {
            id: "ruby:gem".into(),
            mod_id: "ruby".into(),
            template: "minecraft:diamond".into(),
            max_stack: Some(16),
            rarity: Some("rare".into()),
            events: 0,
        };
        assert!(check_item_def(&def).is_ok());
        let json: Value = serde_json::from_slice(&item_json(&def)).unwrap();
        assert_eq!(
            json,
            json!({"template": "minecraft:diamond", "max_stack_size": 16, "rarity": "rare"})
        );
        def.max_stack = None;
        def.rarity = None;
        let json: Value = serde_json::from_slice(&item_json(&def)).unwrap();
        assert_eq!(json, json!({"template": "minecraft:diamond"}));
        def.max_stack = Some(100);
        assert!(check_item_def(&def).unwrap_err().contains("stack size"));
        def.max_stack = Some(0);
        assert!(check_item_def(&def).is_err());
        def.max_stack = None;
        def.template = "diamond".into();
        assert!(check_item_def(&def).unwrap_err().contains("template"));
    }
}
