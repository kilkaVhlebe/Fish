//! Fish blocks in the simulation: state ids, use results, panicking handlers, neighbours, ticks
//! and the script keys' failure reports.

use fish_hooks::Outcome;
use fish_loader::ModState;
use fish_test_host::blocks::FIRST_FISH_STATE;
use fish_test_host::script::{self, ScriptFailure};
use fish_test_host::{TestHost, build_example};

const RUBY: &str = "content_demo:ruby_block";
const RAW: &str = "content_demo:raw_ruby";

fn host() -> TestHost {
    TestHost::new()
        .add_mod(build_example("block-edges"))
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
fn states_follow_mod_and_block_order() {
    let h = host();
    let ids = [
        "block_edges:consumer",
        "block_edges:panicky",
        "block_edges:passer",
        "block_edges:refuser",
        "block_edges:ticker",
        RAW,
        RUBY,
    ];
    for (i, id) in ids.iter().enumerate() {
        let first = FIRST_FISH_STATE + u32::try_from(i).unwrap();
        assert_eq!(h.block_state_range(id), Some(first..first + 1), "{id}");
    }
    assert_eq!(h.block_state_range("content_demo:nope"), None);
    let mut defs: Vec<String> = h.block_defs().into_iter().map(|d| d.id).collect();
    defs.sort();
    let mut want: Vec<String> = ids.iter().map(|s| (*s).to_string()).collect();
    want.sort();
    assert_eq!(defs, want);
    assert_eq!(h.block_content_dirs().len(), 2);
}

#[test]
fn a_use_ends_in_the_blocks_result() {
    let mut h = host();
    h.place_block([1, 64, 0], "block_edges:consumer", "", "Steve");
    h.place_block([2, 64, 0], "block_edges:refuser", "", "Steve");
    h.place_block([3, 64, 0], "block_edges:passer", "", "Steve");

    assert_eq!(
        h.use_block([1, 64, 0], "minecraft:stone", ""),
        Outcome::Handled
    );
    assert_eq!(h.sim.last_use, "handled");
    assert_eq!(count(&h, "block_edges", "consumer used"), 1);
    assert_eq!(count(&h, "block_edges", "game use_block"), 0);

    assert_eq!(
        h.use_block([2, 64, 0], "minecraft:stone", ""),
        Outcome::Cancel
    );
    assert_eq!(h.sim.last_use, "cancel");
    assert_eq!(count(&h, "block_edges", "refuser used"), 1);
    assert_eq!(
        count(&h, "block_edges", "game use_block"),
        0,
        "a cancelled click stops"
    );

    assert_eq!(
        h.use_block([3, 64, 0], "minecraft:stone", ""),
        Outcome::Pass
    );
    assert_eq!(h.sim.last_use, "pass");
    assert_eq!(count(&h, "block_edges", "passer used"), 1);
    assert!(h.logged("game use_block at 3 64 0 on block_edges:passer"));

    // Nothing in the world: the game's flow runs for the named block.
    assert_eq!(h.use_block([9, 9, 9], "minecraft:dirt", ""), Outcome::Pass);
    assert!(h.logged("game use_block at 9 9 9 on minecraft:dirt"));
}

#[test]
fn a_panicking_handler_stops_only_its_mod() {
    let mut h = host();
    h.place_block([1, 64, 0], "block_edges:panicky", "", "Steve");
    h.place_block([2, 64, 0], "block_edges:consumer", "", "Steve");
    h.place_block([3, 64, 0], RUBY, "", "Steve");

    h.use_block([1, 64, 0], "minecraft:stone", "");
    assert!(
        matches!(h.state("block_edges"), Some(ModState::Failed(_))),
        "{:?}",
        h.state("block_edges")
    );
    assert_eq!(h.state("content_demo"), Some(ModState::Ok));

    // The failed mod's other blocks no longer decide; the other mod's block still does.
    assert_eq!(
        h.use_block([2, 64, 0], "minecraft:stone", ""),
        Outcome::Pass
    );
    assert_eq!(count(&h, "block_edges", "consumer used"), 0);
    assert_eq!(
        h.use_block([3, 64, 0], "minecraft:stone", ""),
        Outcome::Handled
    );
    assert_eq!(count(&h, "content_demo", "ruby block used at 3 64 0"), 1);
    assert!(h.block_defs().iter().all(|d| d.mod_id == "content_demo"));
}

#[test]
fn neighbours_are_told_when_a_fish_block_is_placed_or_broken() {
    let mut h = only_content_demo();
    h.place_block([5, 64, 5], RUBY, "", "Steve");
    assert_eq!(count(&h, "content_demo", "neighbor changed"), 0);
    for pos in [
        [5, 63, 5],
        [5, 65, 5],
        [5, 64, 4],
        [5, 64, 6],
        [4, 64, 5],
        [6, 64, 5],
    ] {
        h.place_block(pos, RUBY, "minecraft:stick", "Alex");
    }
    assert_eq!(count(&h, "content_demo", "neighbor changed"), 6);
    assert_eq!(
        count(&h, "content_demo", "neighbor changed at 5 64 5 from 5 65 5"),
        1
    );
    assert!(h.break_block([5, 65, 5], RUBY));
    assert_eq!(h.block_at([5, 65, 5]), None);
    assert_eq!(count(&h, "content_demo", "ruby block broken at 5 65 5"), 1);
    assert_eq!(
        count(&h, "content_demo", "neighbor changed at 5 64 5 from 5 65 5"),
        2
    );
    // A vanilla neighbour is not told anything.
    h.place_block([20, 64, 20], "minecraft:stone", "", "Steve");
    h.place_block([21, 64, 20], RAW, "", "Steve");
    assert_eq!(count(&h, "content_demo", "neighbor changed"), 7);
}

#[test]
fn only_fish_blocks_tick() {
    let mut h = host();
    h.place_block([1, 64, 0], "block_edges:ticker", "", "Steve");
    h.place_block([2, 64, 0], RAW, "", "Steve");
    h.place_block([3, 64, 0], "minecraft:stone", "", "Steve");
    assert!(h.random_tick([1, 64, 0]));
    assert_eq!(count(&h, "block_edges", "ticker ticked at 1 64 0"), 1);
    // A block without a scheduled-tick handler is skipped by the loader.
    assert!(h.scheduled_tick([1, 64, 0]));
    assert!(h.random_tick([2, 64, 0]));
    assert!(!h.random_tick([3, 64, 0]));
    assert!(!h.scheduled_tick([4, 64, 0]));
    assert_eq!(count(&h, "block_edges", "ticker ticked"), 1);
}

#[test]
fn breaking_vanilla_blocks_raises_no_block_event() {
    let mut h = only_content_demo();
    assert!(h.break_block([1, 1, 1], "minecraft:dirt"));
    h.place_block([2, 2, 2], "minecraft:stone", "", "Steve");
    assert!(h.break_block([2, 2, 2], "minecraft:stone"));
    assert_eq!(h.block_at([2, 2, 2]), None);
    assert!(h.sim.events.iter().all(|e| !e.starts_with("block/")));
    assert_eq!(h.fish_state([2, 2, 2]), None);
}

#[test]
fn the_script_picks_face_and_hand() {
    run(r#"
[[step]]
place_block = { pos = [1, 64, 1], block = "content_demo:ruby_block" }
use_block = { pos = [1, 64, 1], face = "west", hand = "off" }
[step.expect]
use_result = "consume"
logs = ["ruby block used at 1 64 1 by Steve face Some(West)"]
"#)
    .unwrap();
}

#[test]
#[should_panic(expected = "unknown face")]
fn an_unknown_face_is_a_mistake_in_the_script() {
    let _ = run(r#"
[[step]]
use_block = { pos = [1, 64, 1], face = "sideways" }
"#);
}

#[test]
fn script_failures_name_what_was_seen() {
    let f = run(r#"
[[step]]
place_block = { pos = [1, 64, 1], block = "content_demo:ruby_block" }
[step.expect]
blocks = { "1 64 1" = "minecraft:stone" }
"#)
    .unwrap_err();
    assert_eq!(f.what, "expect.blocks");
    assert_eq!(f.expected, "minecraft:stone at 1 64 1");
    assert_eq!(f.actual, "content_demo:ruby_block at 1 64 1");

    let f = run("[[step]]\n[step.expect]\nblocks = { \"1 64 1\" = \"content_demo:raw_ruby\" }")
        .unwrap_err();
    assert_eq!(f.actual, "air at 1 64 1");

    let f = run("[[step]]\n[step.expect]\nblocks = { \"up here\" = \"air\" }").unwrap_err();
    assert_eq!(f.expected, "a position `x y z`");

    let f =
        run("[[step]]\n[step.expect]\nblock_states = { \"nobody:none\" = [1, 1] }").unwrap_err();
    assert_eq!(f.what, "expect.block_states");
    assert_eq!(f.actual, "nobody:none has no states");

    let f = run("[[step]]\n[step.expect]\nblock_states = { \"content_demo:ruby_block\" = [1, 1] }")
        .unwrap_err();
    assert_eq!(f.expected, "content_demo:ruby_block states [1, 1]");
    assert_eq!(f.actual, "content_demo:ruby_block states [35724, 1]");

    let f = run("[[step]]\nuse_block = { pos = [1, 64, 1] }\n[step.expect]\nuse_result = \"fail\"")
        .unwrap_err();
    assert_eq!(f.what, "expect.use_result");
    assert_eq!(f.expected, "fail (cancel)");
    assert_eq!(f.actual, "\"pass\"");

    let f = run("[[step]]\n[step.expect]\nuse_result = \"maybe\"").unwrap_err();
    assert_eq!(f.expected, "success, consume, fail or pass");

    let f = run("[[step]]\n[step.expect]\ncontent_files = [\"nope.json\"]").unwrap_err();
    assert_eq!(f.what, "expect.content_files");
    assert_eq!(f.expected, "nope.json in a content folder");

    let f = run("[[step]]\n[step.expect]\nno_content_files = [\"horizon-mod.toml\"]").unwrap_err();
    assert_eq!(f.what, "expect.no_content_files");
}

#[test]
fn use_results_of_the_script_keys() {
    let mut h = host();
    let text = r#"
[[step]]
place_block = { pos = [1, 64, 0], block = "block_edges:consumer" }
place_block_unused = 1
"#;
    assert!(script::parse(text).is_err(), "unknown keys are errors");
    for (block, result) in [
        ("block_edges:consumer", "consume"),
        ("block_edges:refuser", "fail"),
        ("block_edges:passer", "pass"),
    ] {
        let text = format!(
            "[[step]]\nplace_block = {{ pos = [1, 64, 0], block = \"{block}\" }}\nuse_block = {{ pos = [1, 64, 0] }}\n[step.expect]\nuse_result = \"{result}\"\n"
        );
        script::run(&mut h, &script::parse(&text).unwrap()).unwrap();
    }
}
