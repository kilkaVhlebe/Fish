//! Fish items in the simulation: registration, use results, panicking handlers, the inventory
//! tick, tooltips, eating, creative tabs and the script keys' failure reports.

use fish_hooks::Outcome;
use fish_loader::ModState;
use fish_test_host::script::{self, ScriptFailure};
use fish_test_host::{TestHost, build_example};

const RUBY: &str = "content_demo:ruby";
const SNACK: &str = "content_demo:ruby_snack";

fn host() -> TestHost {
    TestHost::new()
        .add_mod(build_example("item-edges"))
        .add_mod(build_example("content-demo"))
        .boot()
        .unwrap()
}

fn only_content_demo() -> TestHost {
    TestHost::new()
        .add_mod(build_example("content-demo"))
        .boot()
        .unwrap()
}

fn count(h: &TestHost, mod_id: &str, needle: &str) -> usize {
    h.mod_log(mod_id)
        .iter()
        .filter(|m| m.contains(needle))
        .count()
}

fn run(text: &str) -> Result<(), ScriptFailure> {
    let mut h = only_content_demo();
    script::run(&mut h, &script::parse(text).unwrap())
}

#[test]
fn items_are_declared_with_their_settings() {
    let h = host();
    let mut defs = h.item_defs();
    defs.sort_by(|a, b| a.id.cmp(&b.id));
    let ids: Vec<&str> = defs.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            RUBY,
            SNACK,
            "item_edges:consumer",
            "item_edges:entity_user",
            "item_edges:finisher",
            "item_edges:panicky",
            "item_edges:passer",
            "item_edges:refuser",
            "item_edges:stacked",
            "item_edges:ticker",
            "item_edges:tipper",
        ]
    );
    let ruby = defs.iter().find(|d| d.id == RUBY).unwrap();
    assert_eq!(ruby.template, "minecraft:diamond");
    assert_eq!(ruby.max_stack, Some(16));
    assert_eq!(ruby.rarity.as_deref(), Some("rare"));
    let stacked = defs.iter().find(|d| d.id == "item_edges:stacked").unwrap();
    assert_eq!(
        (stacked.max_stack, stacked.rarity.as_deref(), stacked.events),
        (Some(1), Some("epic"), 0)
    );
    let tipper = defs.iter().find(|d| d.id == "item_edges:tipper").unwrap();
    assert_eq!(tipper.max_stack, None);
    assert_eq!(tipper.events, 1 << 5);
    assert_eq!(h.block_content_dirs().len(), 2);
    assert!(h.is_fish_item(RUBY) && !h.is_fish_item("minecraft:stick"));
}

#[test]
fn a_use_ends_in_the_items_result() {
    let mut h = host();
    assert_eq!(h.use_item("item_edges:consumer", 1, 0), Outcome::Handled);
    assert_eq!(h.sim.last_use, "handled");
    assert_eq!(count(&h, "item_edges", "consumer used"), 1);

    assert_eq!(h.use_item("item_edges:refuser", 1, 0), Outcome::Cancel);
    assert_eq!(h.sim.last_use, "cancel");
    assert_eq!(count(&h, "item_edges", "refuser used"), 1);

    assert_eq!(h.use_item("item_edges:passer", 1, 1), Outcome::Pass);
    assert_eq!(h.sim.last_use, "pass");
    assert_eq!(count(&h, "item_edges", "passer used"), 1);

    // an item that only has a tooltip or a tick does not decide
    assert_eq!(h.use_item("item_edges:tipper", 1, 0), Outcome::Pass);
    assert_eq!(h.use_item("item_edges:stacked", 1, 0), Outcome::Pass);
    assert!(h.sim.events.iter().all(|e| !e.contains("tipper")));
}

#[test]
fn vanilla_items_raise_no_item_event() {
    let mut h = host();
    assert_eq!(h.use_item("minecraft:stick", 1, 0), Outcome::Pass);
    assert_eq!(
        h.use_item_on_entity(5, "minecraft:cow", "minecraft:bucket", 0),
        Outcome::Pass
    );
    h.finish_using("minecraft:apple", 1, 0);
    assert!(h.item_tooltip("minecraft:stick", 1).is_empty());
    assert!(h.sim.events.iter().all(|e| !e.starts_with("item/use")));
    assert_eq!(count(&h, "item_edges", "used"), 0);
}

#[test]
fn a_panicking_handler_stops_only_its_mod() {
    let mut h = host();
    h.use_item("item_edges:panicky", 1, 0);
    assert!(
        matches!(h.state("item_edges"), Some(ModState::Failed(_))),
        "{:?}",
        h.state("item_edges")
    );
    assert_eq!(h.state("content_demo"), Some(ModState::Ok));

    // the failed mod's other items no longer decide; the other mod's item still does
    assert_eq!(h.use_item("item_edges:consumer", 1, 0), Outcome::Pass);
    assert_eq!(count(&h, "item_edges", "consumer used"), 0);
    assert_eq!(h.use_item(RUBY, 1, 0), Outcome::Handled);
    assert_eq!(count(&h, "content_demo", "ruby used by Steve"), 1);
    assert!(h.item_defs().iter().all(|d| d.mod_id == "content_demo"));
    assert!(h.item_tooltip("item_edges:tipper", 1).is_empty());
}

#[test]
fn entities_and_blocks_are_offered_the_item_first() {
    let mut h = host();
    assert_eq!(
        h.use_item_on_entity(77, "minecraft:pig", "item_edges:entity_user", 0),
        Outcome::Handled
    );
    assert_eq!(
        count(
            &h,
            "item_edges",
            "entity_user used on 77 (minecraft:pig) by Steve"
        ),
        1
    );
    // the entity decision passes: the item's own use runs next, and it has none
    assert_eq!(
        h.use_item_on_entity(78, "minecraft:pig", "item_edges:passer", 0),
        Outcome::Pass
    );
    assert_eq!(count(&h, "item_edges", "passer used"), 1);

    assert_eq!(
        h.use_block([1, 64, 1], "minecraft:stone", RUBY),
        Outcome::Handled
    );
    assert_eq!(
        count(
            &h,
            "content_demo",
            "ruby used on minecraft:stone at 1 64 1 face Some(Up)"
        ),
        1
    );
}

#[test]
fn inventory_ticks_carry_the_slot_and_the_selection() {
    let mut h = host();
    h.inventory_tick("item_edges:ticker", 3, 0, true);
    h.inventory_tick("item_edges:ticker", 1, 38, false);
    h.inventory_tick("item_edges:ticker", 1, 40, true);
    let log = h.mod_log("item_edges");
    for line in [
        "ticker in slot Some(Inventory(0)) selected true count 3",
        "ticker in slot Some(Armor(2)) selected false count 1",
        "ticker in slot Some(OffHand) selected true count 1",
    ] {
        assert!(log.iter().any(|l| l.contains(line)), "{line}: {log:?}");
    }
}

#[test]
fn tooltips_come_from_the_items_mod_only() {
    let mut h = host();
    assert_eq!(
        h.item_tooltip("item_edges:tipper", 1),
        ["§6first line", "second line"]
    );
    assert_eq!(h.item_tooltip(RUBY, 1), ["§7A gem of content_demo"]);
    assert!(h.item_tooltip("item_edges:consumer", 1).is_empty());
    assert!(h.item_tooltip("minecraft:stick", 1).is_empty());
}

#[test]
fn eating_reports_the_player() {
    let mut h = host();
    h.finish_using("item_edges:finisher", 1, 0);
    h.finish_using(SNACK, 1, 0);
    assert_eq!(count(&h, "item_edges", "finisher eaten by Steve"), 1);
    assert_eq!(count(&h, "content_demo", "ruby snack eaten by Steve"), 1);
}

#[test]
fn creative_tabs_are_added_and_entries_reach_vanilla_tabs_and_search() {
    let h = host();
    let tabs = h.creative_tabs();
    let find = |id: &str| {
        tabs.iter()
            .find(|t| t.id == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    // free places are given in id order
    let (a, b) = (find("content_demo:ruby_tab"), find("item_edges:tab"));
    assert_eq!((a.row.as_str(), a.column), ("TOP", 1));
    assert_eq!((b.row.as_str(), b.column), ("TOP", 2));
    assert!(a.visible && b.visible);
    assert_eq!((a.title.as_str(), a.icon.as_str()), ("Ruby", RUBY));
    assert_eq!(b.title, "Item Edges");
    assert_eq!(
        a.entries,
        [
            RUBY,
            SNACK,
            "content_demo:ruby_block",
            "content_demo:raw_ruby"
        ]
    );
    assert_eq!(
        b.entries,
        [
            "item_edges:consumer",
            "item_edges:refuser",
            "item_edges:passer",
            "item_edges:stacked"
        ]
    );
    assert_eq!(
        find("minecraft:building_blocks").entries,
        ["minecraft:stone", "item_edges:consumer"]
    );
    assert_eq!(
        find("minecraft:ingredients").entries,
        ["minecraft:iron_ingot", RUBY]
    );
    // the search tab lists everything once
    let search = find("minecraft:search");
    for id in a.entries.iter().chain(&b.entries) {
        assert_eq!(
            search.entries.iter().filter(|e| *e == id).count(),
            1,
            "{id}"
        );
    }
    assert!(h.mod_log("item_edges").iter().all(|l| !l.contains("error")));
}

#[test]
fn a_stopped_mods_tabs_and_entries_go_with_it() {
    let mut h = host();
    h.use_item("item_edges:panicky", 1, 0);
    h.build_creative();
    let tabs = h.creative_tabs();
    assert!(tabs.iter().all(|t| t.id != "item_edges:tab"));
    assert!(tabs.iter().any(|t| t.id == "content_demo:ruby_tab"));
    let building = tabs
        .iter()
        .find(|t| t.id == "minecraft:building_blocks")
        .unwrap();
    assert!(
        !building
            .entries
            .iter()
            .any(|e| e.starts_with("item_edges:"))
    );
}

#[test]
fn registering_after_the_freeze_is_refused() {
    let mut h = only_content_demo();
    h.start_game();
    assert!(h.logged("late item refused: invalid argument: registries are frozen"));
    assert!(h.item_defs().iter().all(|d| d.id != "content_demo:late"));
}

#[test]
fn script_keys_drive_items() {
    run(r#"
[[step]]
use_item = { item = "content_demo:ruby", hand = "off" }
[step.expect]
use_result = "success"
logs = ["ruby used by Steve hand Off"]
"#)
    .unwrap();
}

#[test]
fn script_failures_name_what_was_seen() {
    let f = run("[[step]]\n[step.expect]\nitems = [\"content_demo:nope\"]").unwrap_err();
    assert_eq!(f.what, "expect.items");
    assert_eq!(f.expected, "item content_demo:nope registered");
    assert!(f.actual.contains("content_demo:ruby"), "{}", f.actual);

    let f = run("[[step]]\n[step.expect]\ntooltips = { \"content_demo:ruby\" = [\"other\"] }")
        .unwrap_err();
    assert_eq!(f.what, "expect.tooltips");
    assert_eq!(f.actual, "[\"§7A gem of content_demo\"]");

    let f = run("[[step]]\n[step.expect]\ncreative_tabs = [\"content_demo:nowhere\"]").unwrap_err();
    assert_eq!(f.what, "expect.creative_tabs");

    let f = run(
        "[[step]]\n[step.expect]\ncreative_entries = { \"content_demo:ruby_tab\" = [\"content_demo:raw_ruby\", \"content_demo:ruby\"] }",
    )
    .unwrap_err();
    assert_eq!(f.what, "expect.creative_entries");

    let f = run(
        "[[step]]\n[step.expect]\ncreative_entries = { \"nobody:tab\" = [\"content_demo:ruby\"] }",
    )
    .unwrap_err();
    assert_eq!(f.expected, "creative tab nobody:tab");

    let f = run(
        "[[step]]\n[step.expect]\nno_creative_entries = { \"minecraft:ingredients\" = [\"content_demo:ruby\"] }",
    )
    .unwrap_err();
    assert_eq!(f.what, "expect.no_creative_entries");

    let f = run("[[step]]\nuse_item = { item = \"content_demo:ruby\" }\n[step.expect]\nuse_result = \"fail\"")
        .unwrap_err();
    assert_eq!(f.what, "expect.use_result");
}
