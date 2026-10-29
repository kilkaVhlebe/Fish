//! Example mod: a greeting from the server, a reply from the client, and a few edge cases.
//!
//! * `net_demo:hello` (server to client, play): a greeting sent when a player joins.
//! * `net_demo:reply` (client to server, play): the client answers with its own name.
//! * `net_demo:cfg` (server to client, configuration): sent while the player configures.
//! * `net_demo:small` (client to server, play, 8 bytes at most) and `net_demo:boom` (a receiver that panics).

use fish_api::Error;
use fish_api::event::networking;
use fish_api::networking::{
    ClientConfigurationNetworking, ClientPlayNetworking, PacketBuf, PacketReader,
    PayloadTypeRegistry, ServerConfigurationNetworking, ServerPlayNetworking,
};

fn init() -> Result<(), Error> {
    PayloadTypeRegistry::play_s2c().register("net_demo:hello", 1024)?;
    PayloadTypeRegistry::play_c2s().register("net_demo:reply", 1024)?;
    PayloadTypeRegistry::play_c2s().register("net_demo:small", 8)?;
    PayloadTypeRegistry::play_c2s().register("net_demo:boom", 8)?;
    PayloadTypeRegistry::configuration_s2c().register("net_demo:cfg", 64)?;

    ClientPlayNetworking::register_global_receiver("net_demo:hello", |ctx| {
        let mut r = PacketReader::new(ctx.data());
        let text = r.read_string().unwrap_or_default();
        let n = r.read_i32().unwrap_or(-1);
        log::info!("client got hello: {text} #{n}");
        let reply = PacketBuf::new().string("client says hi");
        if let Err(e) = ClientPlayNetworking::send("net_demo:reply", reply.as_bytes()) {
            log::error!("cannot reply: {e}");
        }
    })?;
    ServerPlayNetworking::register_global_receiver("net_demo:reply", |ctx| {
        let text = PacketReader::new(ctx.data())
            .read_string()
            .unwrap_or_default();
        log::info!("server got reply from {}: {text}", ctx.player_name());
    })?;
    ServerPlayNetworking::register_global_receiver("net_demo:small", |ctx| {
        log::info!("server got {} byte(s) on small", ctx.data().len());
    })?;
    ServerPlayNetworking::register_global_receiver("net_demo:boom", |_| {
        panic!("net_demo asked to panic")
    })?;
    ClientConfigurationNetworking::register_global_receiver("net_demo:cfg", |ctx| {
        log::info!("client got cfg: {}", String::from_utf8_lossy(ctx.data()));
    })?;

    networking::server::INIT.register(|c| {
        log::info!("server init: {}", c.actor());
        if let Err(e) = ServerConfigurationNetworking::send(c.actor(), "net_demo:cfg", b"configure")
        {
            log::error!("cannot send cfg: {e}");
        }
    })?;
    networking::server::JOIN.register(|c| {
        log::info!("server join: {}", c.actor());
        let hello = PacketBuf::new().string("welcome").i32(7);
        if let Err(e) = ServerPlayNetworking::send(c.actor(), "net_demo:hello", hello.as_bytes()) {
            log::info!("hello to {} was not sent: {e}", c.actor());
        }
    })?;
    networking::server::DISCONNECT
        .register(|c| log::info!("server disconnect: {} ({})", c.actor(), c.subject()))?;
    networking::client::INIT.register(|_| log::info!("client init"))?;
    networking::client::JOIN.register(|_| log::info!("client join"))?;
    networking::client::DISCONNECT
        .register(|c| log::info!("client disconnect ({})", c.subject()))?;
    Ok(())
}

fish_api::export_mod! {
    main: [init],
}
