//! Game rules in the simulation: per-world values, saving, and the script keys.

use fish_hooks::{GameRuleReply, RuleValue};
use fish_test_host::script::{self, ScriptFailure};
use fish_test_host::{TestHost, build_example};

const LIMIT: &str = "gamerule_demo:sparkle_limit";
const STONE: &str = "gamerule_demo:allow_stone_break";

fn host() -> TestHost {
    TestHost::new()
        .add_mod(build_example("gamerule-demo"))
        .boot()
        .unwrap()
}

fn run(text: &str) -> Result<(), ScriptFailure> {
    let mut h = host();
    script::run(&mut h, &script::parse(text).unwrap())
}

#[test]
fn rules_are_listed_in_registration_order_with_their_categories() {
    let h = host();
    let defs = h.game_rule_defs();
    let names: Vec<(&str, &str)> = defs
        .iter()
        .map(|d| (d.name.as_str(), d.category.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("gamerule_demo:allow_stone_break", "player"),
            ("gamerule_demo:sparkle_limit", "mobs"),
            ("gamerule_demo:damage_scale", "drops"),
            ("gamerule_demo:difficulty_mode", "misc"),
        ]
    );
    assert_eq!(h.game_rule(LIMIT), Some(RuleValue::Int(8)));
    assert_eq!(h.game_rule("nobody:none"), None);
}

#[test]
fn values_are_saved_with_the_world_and_come_back_after_a_reboot() {
    let mut h = host();
    h.start_game();
    let reply = h.gamerule(LIMIT, Some("20")).unwrap();
    assert_eq!(
        reply,
        GameRuleReply::Set {
            name: LIMIT.into(),
            value: "20".into(),
            result: 20
        }
    );
    h.gamerule("gamerule_demo:difficulty_mode", Some("easy"))
        .unwrap();
    // the game saves the world: the rules go beside the game's own, under Fish's namespace
    h.stop_game();
    let file = h.game_dir().join("saves/world/data/fish/game_rules.json");
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["rules"][LIMIT], 20);
    assert_eq!(saved["rules"]["gamerule_demo:difficulty_mode"], "easy");
    assert!(
        saved["rules"].get(STONE).is_none(),
        "defaults are not saved"
    );

    // a new process: the world opens and reads them back
    let mut h = h.reboot().unwrap();
    assert_eq!(
        h.game_rule(LIMIT),
        Some(RuleValue::Int(8)),
        "not loaded yet"
    );
    h.start_game();
    assert_eq!(h.game_rule(LIMIT), Some(RuleValue::Int(20)));
    assert_eq!(
        h.game_rule("gamerule_demo:difficulty_mode"),
        Some(RuleValue::Text("easy".into()))
    );
    assert!(h.logged("rules at start: allow_stone_break=Ok(true) sparkle_limit=Ok(20)"));
}

#[test]
fn the_main_menu_has_no_world_to_write_to() {
    let mut h = host();
    h.close_world();
    assert_eq!(h.world_id(), None);
    assert_eq!(h.game_rule(LIMIT), Some(RuleValue::Int(8)));
    assert_eq!(h.gamerule(LIMIT, Some("3")), None);
    // the mod cannot write either: its command only logs the refusal
    assert!(h.command("rules cap"));
    assert!(h.logged("cannot cap the sparkle limit: setting a game rule failed: UNSUPPORTED"));
    h.switch_world("again");
    assert!(
        h.world_id()
            .unwrap()
            .replace('\\', "/")
            .ends_with("saves/again")
    );
    assert!(h.command("rules cap"));
    assert_eq!(h.game_rule(LIMIT), Some(RuleValue::Int(4)));
}

#[test]
fn a_rule_nobody_registered_is_left_to_the_game() {
    let mut h = host();
    assert_eq!(h.gamerule("minecraft:keep_inventory", Some("true")), None);
    assert_eq!(h.gamerule("nobody:none", None), None);
    assert!(h.sim.command_feedback.is_empty());
}

#[test]
fn refused_values_are_reported_and_change_nothing() {
    let mut h = host();
    for (name, value, why) in [
        (LIMIT, "many", "takes a whole number"),
        (LIMIT, "99", "must be between 0 and 64"),
        (STONE, "maybe", "takes true or false"),
        (
            "gamerule_demo:damage_scale",
            "-1",
            "must be between 0 and 10",
        ),
        (
            "gamerule_demo:difficulty_mode",
            "nightmare",
            "must be one of easy, normal, hard",
        ),
    ] {
        let Some(GameRuleReply::Failed(message)) = h.gamerule(name, Some(value)) else {
            panic!("{name} {value} was not refused");
        };
        assert!(message.contains(why), "{message}");
        assert_eq!(h.sim.command_ok, Some(false));
    }
    assert_eq!(h.game_rule(LIMIT), Some(RuleValue::Int(8)));
    assert_eq!(
        h.gamerule(LIMIT, None),
        Some(GameRuleReply::Query {
            name: LIMIT.into(),
            value: "8".into(),
            result: 8
        })
    );
    assert_eq!(
        h.sim.chat.last().unwrap(),
        "Gamerule gamerule_demo:sparkle_limit is currently set to: 8"
    );
}

#[test]
fn script_failures_name_the_rule_and_the_values() {
    let f =
        run("[[step]]\ngamerule = { name = \"gamerule_demo:nothing\", value = 1 }").unwrap_err();
    assert_eq!((f.step, f.what.as_str()), (1, "gamerule"));
    assert!(f.expected.contains("Fish rule named gamerule_demo:nothing"));
    assert!(
        f.actual.contains("gamerule_demo:sparkle_limit"),
        "{}",
        f.actual
    );

    let f = run("[[step]]\ngamerule = { name = \"gamerule_demo:sparkle_limit\", value = [1] }")
        .unwrap_err();
    assert_eq!(f.what, "gamerule");
    assert_eq!(f.expected, "a boolean, number or string value");

    let f = run("[[step]]\n[step.expect]\ngamerules = { \"gamerule_demo:sparkle_limit\" = 9 }")
        .unwrap_err();
    assert_eq!(f.what, "expect.gamerules");
    assert_eq!(f.expected, "gamerule_demo:sparkle_limit = 9");
    assert_eq!(f.actual, "gamerule_demo:sparkle_limit = 8");

    let f = run("[[step]]\n[step.expect]\ngamerules = { \"x:y\" = 1 }").unwrap_err();
    assert_eq!(f.expected, "a game rule named x:y");

    let f = run("[[step]]\n[step.expect]\ngamerules = { \"gamerule_demo:sparkle_limit\" = [1] }")
        .unwrap_err();
    assert!(f.expected.contains("to be a boolean, number or string"));

    let f =
        run("[[step]]\n[step.expect]\ngamerules = { \"gamerule_demo:sparkle_limit\" = \"lots\" }")
            .unwrap_err();
    assert_eq!(f.expected, "gamerule_demo:sparkle_limit = lots");
    assert!(f.actual.contains("takes a whole number"), "{}", f.actual);

    // a decimal rule accepts an integer literal
    run("[[step]]\n[step.expect]\ngamerules = { \"gamerule_demo:damage_scale\" = 1 }").unwrap();
    // switching to a world and querying it works as a step
    run("[[step]]\nworld = \"w2\"\ngamerule = { name = \"gamerule_demo:sparkle_limit\" }\n[step.expect]\ncommand_ok = true")
        .unwrap();
}
