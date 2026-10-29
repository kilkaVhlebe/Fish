//! The simulated game events: what the loader derives, orders and filters.

use fish_hooks::{GameEvent, Kind, Outcome};
use fish_test_host::{TestHost, build_example};

fn host() -> TestHost {
    TestHost::new()
        .add_mod(build_example("events-demo"))
        .boot()
        .unwrap()
}

fn trace(host: &TestHost) -> Vec<String> {
    host.sim
        .events
        .iter()
        .map(|l| l.split(' ').next().unwrap().to_string())
        .collect()
}

#[test]
fn starting_and_stopping_fire_the_lifecycle_in_order_and_derive_the_rest() {
    let mut h = host();
    h.start_game();
    assert_eq!(
        trace(&h),
        [
            "lifecycle/server_starting",
            "lifecycle/world_load",
            "lifecycle/server_started",
            "lifecycle/client_started"
        ]
    );
    // started events fire once: later ticks and frames add nothing
    h.tick_integrated_server(5);
    h.frame(3);
    assert_eq!(h.sim.events.len(), 4);
    // a second level loads on demand; both unload when the server stops
    let mut nether = GameEvent::new(Kind::WorldLoad);
    nether.dimension = "minecraft:the_nether";
    h.game_event(&nether);
    h.game_event(&nether);
    h.sim.events.clear();
    h.stop_game();
    assert_eq!(
        trace(&h),
        [
            "lifecycle/client_stopping",
            "lifecycle/server_stopping",
            "lifecycle/world_unload",
            "lifecycle/world_unload",
            "lifecycle/server_stopped",
            "lifecycle/client_stopped"
        ]
    );
    assert!(h.logged("world unloading: minecraft:overworld"));
    assert!(h.logged("world unloading: minecraft:the_nether"));
}

#[test]
fn breaks_fire_after_on_the_next_server_tick_and_canceled_instead_when_vetoed() {
    let mut h = host();
    assert!(!h.break_block([1, 1, 1], "minecraft:bedrock"));
    assert!(h.break_block([2, 2, 2], "minecraft:dirt"));
    assert!(!h.logged("broke at"), "not before the tick");
    h.tick(1);
    assert!(
        !h.logged("broke at"),
        "the window's tick is not the server's"
    );
    h.tick_integrated_server(1);
    assert!(h.logged("minecraft:dirt broke at 2 2 2"));
    assert!(!h.logged("bedrock broke"));
    assert!(h.logged("break canceled at 1 1 1"));
    h.tick_integrated_server(3);
    assert_eq!(
        h.logs()
            .iter()
            .filter(|r| r.message.contains("broke at"))
            .count(),
        1
    );
}

#[test]
fn damage_that_is_vetoed_is_not_followed_by_after_damage() {
    let mut h = host();
    assert!(!h.damage(1, "minecraft:zombie", 2.0, "minecraft:fall"));
    assert!(h.damage(1, "minecraft:zombie", 2.0, "minecraft:lava"));
    let t = trace(&h);
    assert_eq!(
        t,
        [
            "entity/allow_damage",
            "entity/allow_damage",
            "entity/after_damage"
        ]
    );
}

#[test]
fn chat_send_is_allowed_then_rewritten_then_sent() {
    let mut h = host();
    assert_eq!(h.chat_out("plain"), Some("plain".to_string()));
    assert_eq!(h.chat_out("a :fish: b"), Some("a ><> b".to_string()));
    assert_eq!(
        trace(&h),
        [
            "message/allow_send",
            "message/modify_send",
            "message/send",
            "message/allow_send",
            "message/modify_send",
            "message/send"
        ]
    );
    assert_eq!(h.sim.sent_chat, ["plain", "a ><> b"]);
    // the rewrite is only offered by modify_send
    let mut e = GameEvent::new(Kind::Send);
    e.subject = "a :fish: b";
    assert_eq!(h.game_event(&e), Outcome::Pass);
}

#[test]
fn reload_ends_on_the_next_server_tick_only() {
    let mut h = host();
    h.reload();
    h.tick(2);
    assert!(!h.logged("reload finished"));
    h.tick_integrated_server(1);
    assert!(h.logged("reload finished"));
    h.tick_integrated_server(1);
    assert_eq!(
        h.logs()
            .iter()
            .filter(|r| r.message == "reload finished")
            .count(),
        1
    );
}

#[test]
fn detached_events_have_no_game_and_hot_hooks_can_ask_first() {
    let mut h = host();
    let mut c = GameEvent::new(Kind::ChunkLoad);
    c.pos = [3, 0, -4];
    h.game_event(&c);
    assert!(h.logs().iter().any(|r| r.message == "chunk loaded: 3 -4"));
    assert!(h.wants(Kind::ChunkLoad));
    assert!(!h.wants(Kind::ChunkUnload), "nobody listens to unloads");
    let bare = TestHost::new()
        .add_mod(build_example("hello"))
        .boot()
        .unwrap();
    let mut bare = bare;
    assert!(!bare.wants(Kind::UseBlock));
    assert_eq!(
        bare.game_event(&GameEvent::new(Kind::UseBlock)),
        Outcome::Pass
    );
}

#[test]
fn f3_lines_start_with_fish_and_collect_mod_lines() {
    let mut h = host();
    let lines = h.debug_lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("[Fish] 0.1.0 - 1 mod(s)"), "{lines:?}");
    assert_eq!(lines[1], "events-demo is listening");
}

#[test]
fn players_join_and_leave_through_the_server_events() {
    let mut h = host();
    h.join("Alex");
    h.leave("Alex");
    assert_eq!(trace(&h), ["entity/player_join", "entity/player_leave"]);
    assert!(h.sim.players.is_empty());
}
