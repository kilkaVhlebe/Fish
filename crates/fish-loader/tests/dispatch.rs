//! The hook dispatcher: Horizon's hook points become events, services and mod rows.

use fish_hooks::Dispatch;
use fish_loader::{LoaderDispatch, Side};
use fish_test_host::{TestHost, build_example};

fn ticks_host(side: Side) -> TestHost {
    TestHost::new()
        .side(side)
        .add_mod(build_example("ticks"))
        .boot()
        .unwrap()
}

fn mod_log(host: &TestHost) -> Vec<String> {
    host.mod_log("ticks")
}

#[test]
fn window_ticks_fire_client_events_and_are_numbered() {
    let mut host = ticks_host(Side::Client);
    host.tick(3);
    let log = mod_log(&host);
    assert!(log.iter().any(|m| m == "client start 1 Window"), "{log:?}");
    assert!(log.iter().any(|m| m == "client end 3"), "{log:?}");
    assert!(!log.iter().any(|m| m.starts_with("server")), "{log:?}");
    // start precedes end within a tick
    let pos = |needle: &str| log.iter().position(|m| m == needle).unwrap();
    assert!(pos("client start 1 Window") < pos("client end 1"));
    assert!(pos("client end 1") < pos("client start 2 Window"));
}

#[test]
fn integrated_and_dedicated_servers_fire_server_events() {
    let mut host = ticks_host(Side::Client);
    host.tick_integrated_server(2);
    let log = mod_log(&host);
    assert!(
        log.iter().any(|m| m == "server start 1 IntegratedServer"),
        "{log:?}"
    );
    assert!(log.iter().any(|m| m == "server end 2"), "{log:?}");
    assert!(!log.iter().any(|m| m.starts_with("client")), "{log:?}");

    let mut server = ticks_host(Side::Server);
    server.tick(1);
    let log = mod_log(&server);
    assert!(
        log.iter().any(|m| m == "server start 1 DedicatedServer"),
        "{log:?}"
    );
}

#[test]
fn views_answer_through_the_client_service_only_during_the_callback() {
    let mut host = ticks_host(Side::Client);
    host.tick(2);
    let log = mod_log(&host);
    assert!(log.iter().any(|m| m == "role Ok(Window)"), "{log:?}");
    assert!(
        log.iter()
            .any(|m| m.contains("player Ok(Some(Player { x: 0.5, y: 64.0, z: 0.5")),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|m| m.contains("dimension: \"minecraft:overworld\"")),
        "{log:?}"
    );
    assert!(log.iter().any(|m| m == "game dir Ok(true)"), "{log:?}");
    assert!(
        log.iter().any(|m| m == "stale: Err(Some(FishStatus(-5)))"),
        "a handle used after its callback is STALE_HANDLE: {log:?}"
    );

    // outside a world there is no player
    let mut menu = ticks_host(Side::Client);
    menu.sim.player = None;
    menu.tick(1);
    assert!(mod_log(&menu).iter().any(|m| m == "player Ok(None)"));
    assert!(
        host.sim.chat.is_empty(),
        "the stale chat line was never shown"
    );
}

#[test]
fn frames_hud_and_keys_reach_mods() {
    let mut host = ticks_host(Side::Client);
    host.frame(1);
    host.key("key.keyboard.z", true);
    host.key("key.keyboard.z", false);
    let log = mod_log(&host);
    assert!(log.iter().any(|m| m == "frame 0.017"), "{log:?}");
    assert!(
        log.iter()
            .any(|m| m == "hud size Ok([427.0, 240.0]) width Ok(18.0)"),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|m| m == "key key.keyboard.z pressed=true down=Ok(true)"),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|m| m == "key key.keyboard.z pressed=false down=Ok(false)"),
        "{log:?}"
    );
}

#[test]
fn commands_are_first_non_pass() {
    let mut host = ticks_host(Side::Client);
    assert!(
        !host.command("ticks pass"),
        "Pass leaves the command to the game"
    );
    assert!(!host.command("anything else"), "Fail is not Success");
    assert_eq!(host.sim.commands, ["ticks pass", "anything else"]);
    let hello = TestHost::new()
        .add_mod(build_example("hello"))
        .boot()
        .unwrap();
    let mut hello = hello;
    assert!(hello.command("fish hello"));
    assert!(!hello.command("fish other"));
}

#[test]
fn mods_are_listed_for_options_mods() {
    let host = TestHost::new()
        .add_mod(build_example("hello"))
        .add_mod(build_example("abi_mismatch"))
        .boot()
        .unwrap();
    let dispatch = LoaderDispatch::new(host.loader().clone());
    let mut rows = Vec::new();
    dispatch.list_mods(&mut rows);
    assert_eq!(rows.len(), 1, "failed mods are not listed");
    assert_eq!(rows[0].id, "hello");
    assert_eq!(rows[0].title, "Hello 0.1.0 [Fish]");
    assert!(rows[0].description.contains("Fish example mod"));
    let mut buttons = Vec::new();
    dispatch.settings_buttons("hello", &mut buttons);
    assert!(buttons.is_empty());
}

#[test]
fn a_panicking_mod_listener_does_not_break_the_hook() {
    let mut host = TestHost::new()
        .add_mod(build_example("hello"))
        .add_mod(build_example("panicky"))
        .boot()
        .unwrap();
    host.tick(5);
    host.frame(2);
    assert_eq!(host.sim.hud, ["Hello from Fish"]);
}
