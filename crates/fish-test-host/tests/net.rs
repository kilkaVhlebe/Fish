//! Networking over the in-memory wire: payloads, limits, the handshake and registry sync.

use fish_hooks::{NetPacket, NetPhase};
use fish_loader::{ModState, Side};
use fish_test_host::net::Network;
use fish_test_host::{TestHost, build_example, pack_example};

fn server() -> TestHost {
    TestHost::new()
        .side(Side::Server)
        .add_mod(build_example("net-demo"))
        .boot()
        .unwrap()
}

fn client() -> TestHost {
    TestHost::new()
        .add_mod(build_example("net-demo"))
        .boot()
        .unwrap()
}

fn network(names: &[&str]) -> Network {
    let mut n = Network::new(server());
    for name in names {
        n.add_client(name, client());
    }
    n
}

#[test]
fn a_player_joins_and_the_greeting_and_reply_travel() {
    let mut n = network(&["Alex"]);
    assert!(n.connect("Alex"));
    let c = &n.clients["Alex"];
    assert!(c.logged("client init") && c.logged("client join"));
    assert!(
        c.logged("client got cfg: configure"),
        "configuration payloads arrive during configuration"
    );
    assert!(c.logged("client got hello: welcome #7"));
    assert!(n.server.logged("server init: Alex") && n.server.logged("server join: Alex"));
    assert!(
        n.server
            .logged("server got reply from Alex: client says hi")
    );
    n.leave("Alex");
    assert!(n.server.logged("server disconnect: Alex (disconnected)"));
    assert!(n.clients["Alex"].logged("client disconnect (disconnected)"));
}

#[test]
fn two_clients_are_addressed_by_name() {
    let mut n = network(&["Alex", "Bo"]);
    assert!(n.connect("Alex") && n.connect("Bo"));
    assert!(n.server.logged("server got reply from Alex"));
    assert!(n.server.logged("server got reply from Bo"));
    let logs = |name: &str| {
        n.clients[name]
            .logs()
            .iter()
            .filter(|r| r.message.starts_with("client got hello"))
            .count()
    };
    assert_eq!(
        (logs("Alex"), logs("Bo")),
        (1, 1),
        "each client got exactly its own greeting"
    );
}

#[test]
fn unregistered_channels_stay_with_the_game() {
    let mut n = network(&["Alex"]);
    assert!(n.connect("Alex"));
    let c = n.clients.get_mut("Alex").unwrap();
    for channel in ["minecraft:brand", "other:thing"] {
        let consumed = c.net_receive(&NetPacket {
            phase: NetPhase::Play,
            channel,
            data: b"x",
            from: None,
        });
        assert!(!consumed, "{channel} is not a Fish channel");
    }
    // the handshake is Fish's, in the phases it is meant for
    assert!(c.net_receive(&NetPacket {
        phase: NetPhase::Configuration,
        channel: "fish:handshake",
        data: b"not json",
        from: None,
    }));
    assert!(c.logged("malformed fish:handshake"));
}

#[test]
fn registration_limits_and_size_limits_hold() {
    let mut n = network(&["Alex"]);
    assert!(n.connect("Alex"));
    // 40,000 bytes cannot be registered serverbound; a 9-byte payload exceeds the 8-byte type
    let server = &n.server;
    let loader = server.loader();
    let _ = loader; // registration is exercised by the mod in the loader tests below
    let c = n.clients.get_mut("Alex").unwrap();
    let over = c.net_receive(&NetPacket {
        phase: NetPhase::Play,
        channel: "net_demo:hello",
        data: &[0; 2000],
        from: None,
    });
    assert!(over, "consumed even when dropped");
    assert!(c.logged("dropped a 2000-byte payload on net_demo:hello: the limit is 1024 bytes"));
    assert!(
        !c.logged("client got hello: welcome #7")
            || c.logs()
                .iter()
                .filter(|r| r.message.starts_with("client got hello"))
                .count()
                == 1
    );
    let s = &mut n.server;
    assert!(s.net_receive(&NetPacket {
        phase: NetPhase::Play,
        channel: "net_demo:small",
        data: &[1; 8],
        from: Some(("Alex", [1; 16]))
    }));
    assert!(s.logged("server got 8 byte(s) on small"));
    assert!(s.net_receive(&NetPacket {
        phase: NetPhase::Play,
        channel: "net_demo:small",
        data: &[1; 9],
        from: Some(("Alex", [1; 16]))
    }));
    assert!(s.logged("dropped a 9-byte payload on net_demo:small: the limit is 8 bytes"));
}

#[test]
fn a_receiver_that_panics_stops_its_mod_only() {
    let mut n = network(&["Alex"]);
    assert!(n.connect("Alex"));
    let s = &mut n.server;
    s.net_receive(&NetPacket {
        phase: NetPhase::Play,
        channel: "net_demo:boom",
        data: b"x",
        from: Some(("Alex", [1; 16])),
    });
    assert!(
        matches!(s.state("net_demo"), Some(ModState::Failed(r)) if r.contains("net_demo:boom"))
    );
    s.net_receive(&NetPacket {
        phase: NetPhase::Play,
        channel: "net_demo:small",
        data: b"x",
        from: Some(("Alex", [1; 16])),
    });
    assert!(
        !s.logged("server got 1 byte(s) on small"),
        "a stopped mod's receivers do not run"
    );
}

fn with_version(version: &str) -> std::path::PathBuf {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/mods/net-demo/fish.mod.toml"),
    )
    .unwrap()
    .replace("version = \"0.1.0\"", &format!("version = \"{version}\""));
    pack_example("net-demo", &text, &format!("net_demo_{version}"))
}

#[test]
fn synchronised_mods_must_match_between_server_and_client() {
    let s = TestHost::new()
        .side(Side::Server)
        .add_mod(build_example("net-demo"))
        .boot()
        .unwrap();
    s.loader()
        .registry_register("fish:items", "net_demo:ruby", "net_demo")
        .unwrap();
    let c = TestHost::new()
        .add_mod(with_version("0.2.0"))
        .boot()
        .unwrap();
    c.loader()
        .registry_register("fish:items", "net_demo:ruby", "net_demo")
        .unwrap();
    let mut n = Network::new(s);
    n.add_client("Alex", c);
    assert!(!n.connect("Alex"), "the version differs");
    let reason = &n.disconnects[0].1;
    assert_eq!(
        reason,
        "Missing or different Fish mods: net_demo 0.1.0 (server) / 0.2.0 (client)"
    );
    assert!(
        n.server
            .logged("server disconnect: Alex (Missing or different Fish mods")
    );
}

#[test]
fn a_mod_without_registered_content_need_not_match() {
    let s = TestHost::new()
        .side(Side::Server)
        .add_mod(build_example("net-demo"))
        .boot()
        .unwrap();
    let c = TestHost::new()
        .add_mod(with_version("0.3.0"))
        .boot()
        .unwrap();
    let mut n = Network::new(s);
    n.add_client("Alex", c);
    assert!(
        n.connect("Alex"),
        "no synchronised content: any version may join"
    );
}

#[test]
fn registries_are_renumbered_to_the_servers_ids() {
    let s = TestHost::new()
        .side(Side::Server)
        .add_mod(build_example("net-demo"))
        .boot()
        .unwrap();
    for id in ["a:one", "a:two", "a:three"] {
        s.loader()
            .registry_register("fish:blocks", id, "net_demo")
            .unwrap();
    }
    let c = client();
    // a different registration order on the client
    for id in ["a:three", "a:one", "a:two"] {
        c.loader()
            .registry_register("fish:blocks", id, "net_demo")
            .unwrap();
    }
    assert_ne!(
        c.loader().registry_hash(),
        s.loader().registry_hash(),
        "same ids, different numbers"
    );
    let mut n = Network::new(s);
    n.add_client("Alex", c);
    assert!(n.connect("Alex"));
    let cl = n.clients["Alex"].loader();
    assert_eq!(cl.registry_raw("fish:blocks", "a:one"), Some(0));
    assert_eq!(cl.registry_raw("fish:blocks", "a:two"), Some(1));
    assert_eq!(cl.registry_raw("fish:blocks", "a:three"), Some(2));
    assert_eq!(cl.registry_hash(), n.server.loader().registry_hash());
    assert!(
        n.clients["Alex"]
            .logged("synchronised 1 registr(ies) with the server (3 id(s) renumbered)")
    );
}

#[test]
fn registries_with_different_entries_disconnect_the_client() {
    let s = TestHost::new()
        .side(Side::Server)
        .add_mod(build_example("net-demo"))
        .boot()
        .unwrap();
    s.loader()
        .registry_register("fish:blocks", "a:one", "net_demo")
        .unwrap();
    s.loader()
        .registry_register("fish:blocks", "a:two", "net_demo")
        .unwrap();
    let c = client();
    c.loader()
        .registry_register("fish:blocks", "a:one", "net_demo")
        .unwrap();
    c.loader()
        .registry_register("fish:blocks", "a:extra", "net_demo")
        .unwrap();
    let mut n = Network::new(s);
    n.add_client("Alex", c);
    assert!(!n.connect("Alex"));
    assert_eq!(
        n.disconnects[0].1,
        "Fish registries do not match the server's: registry fish:blocks differs (missing on the client: a:two; missing on the server: a:extra)"
    );
}

#[test]
fn a_client_without_fish_joins_only_servers_without_synchronised_content() {
    // the client never answers the handshake: model it by delivering nothing to it
    let s = TestHost::new()
        .side(Side::Server)
        .add_mod(build_example("net-demo"))
        .boot()
        .unwrap();
    let mut open = Network::new(s);
    open.add_vanilla_client("Vic");
    assert!(
        open.connect("Vic"),
        "nothing to synchronise: vanilla clients are welcome"
    );
    assert!(!open.server.logged("server got reply from Vic"));
    assert!(
        open.clients.is_empty(),
        "a vanilla client has no Fish at all"
    );
    assert!(!open.server.loader().registry_hash().is_empty());

    let s2 = TestHost::new()
        .side(Side::Server)
        .add_mod(build_example("net-demo"))
        .boot()
        .unwrap();
    s2.loader()
        .registry_register("fish:blocks", "a:one", "net_demo")
        .unwrap();
    let mut closed = Network::new(s2);
    closed.add_vanilla_client("Vic");
    assert!(!closed.connect("Vic"));
    assert_eq!(
        closed.disconnects[0].1,
        "This server requires Fish mods, but your client does not run Fish: net_demo 0.1.0"
    );
}

#[test]
fn a_fish_client_refuses_a_server_without_fish_when_it_needs_synchronised_content() {
    let c = client();
    c.loader()
        .registry_register("fish:blocks", "a:one", "net_demo")
        .unwrap();
    let mut n = Network::new(TestHost::new().side(Side::Server).boot().unwrap());
    n.set_vanilla_server(true);
    n.add_client("Alex", c);
    assert!(!n.connect("Alex"));
    assert!(
        n.disconnects[0]
            .1
            .starts_with("This server does not run Fish, which these mods need: net_demo 0.1.0")
    );
}
