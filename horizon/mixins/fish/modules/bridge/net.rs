// Custom payloads and connection events on the game thread (the connection threads' side is in
// the library module `fish_lib`).

use std::cell::RefCell;
use std::collections::HashSet;

use fish_hooks::{GameEvent, Kind, NetPacket, NetPhase};
use horizon_client::fish_lib::PlayOut;
use horizon_protocol::packets::{configuration as cfg, play};

use super::views::{GameView, role_of};
use crate::game::Game;
use crate::game::lan::LanHost;

thread_local! {
    // Connections whose disconnect was reported when the server stopped.
    static REPORTED: RefCell<HashSet<u32>> = RefCell::new(HashSet::new());
}

fn fire(game: &mut Game<'_>, e: &GameEvent<'_>) {
    if let Some(d) = fish_hooks::get() {
        d.game_event(Some(&mut GameView { game }), e);
    }
}

/// `handle_packet` (patched): a custom payload of the server reached the client.
pub(in crate::game) fn client_play(game: &mut Game<'_>, p: &cfg::CustomPayload) {
    if p.channel == "minecraft:brand" || p.channel == horizon_protocol::quiet::CHANNEL {
        return;
    }
    let Some(d) = fish_hooks::get() else {
        return;
    };
    let packet = NetPacket {
        phase: NetPhase::Play,
        channel: &p.channel,
        data: &p.data,
        from: None,
    };
    d.net_receive(&mut GameView { game }, &packet);
}

/// The client's game ticks: payloads that waited for the connection go out, a requested
/// disconnect is carried out.
pub(in crate::game) fn client_tick(game: &mut Game<'_>) {
    if role_of(game) != fish_hooks::Role::Window {
        return;
    }
    if let Some(reason) = horizon_client::fish_lib::client_poll()
        && game.remote.is_some()
    {
        let _ = game.leave_server(Some(reason));
    }
}

/// The client entered the play phase (`extras_joined`).
pub(in crate::game) fn client_joined() {
    horizon_client::fish_lib::client_joined();
}

/// `leave_server`: the connection ends.
pub(in crate::game) fn client_left(game: &Game<'_>, reason: Option<&str>) {
    if game.remote.is_some() {
        horizon_client::fish_lib::client_left(reason.unwrap_or(""));
    }
}

fn drain(host: &mut LanHost) {
    let out = horizon_client::fish_lib::take_play_out();
    if out.is_empty() {
        return;
    }
    let mut held = Vec::new();
    for item in out {
        match item {
            PlayOut::Payload {
                player,
                channel,
                data,
            } => {
                let packet = play::Clientbound::CustomPayload(cfg::CustomPayload { channel: channel.clone(), data: data.clone() });
                let mut wait = false;
                for g in host.hub.guests.iter() {
                    if player.as_deref().is_some_and(|p| p != g.name) {
                        continue;
                    }
                    if g.placed {
                        g.send(&packet);
                    } else if player.is_some() {
                        wait = true;
                    }
                }
                if wait {
                    held.push(PlayOut::Payload {
                        player,
                        channel,
                        data,
                    });
                }
            }
            PlayOut::Kick { player, reason } => {
                if let Some(conn) = host
                    .hub
                    .guests
                    .iter()
                    .find(|g| g.name == player)
                    .map(|g| g.conn)
                {
                    host.hub.kick(conn, horizon_client::fish_lib::reason_tag(&reason));
                }
            }
        }
    }
    horizon_client::fish_lib::requeue_play_out(held);
}

/// Every server tick: writes what mods queued for players in play.
pub(in crate::game) fn server_tick(host: &mut LanHost) {
    drain(host);
}

/// `lan_guest_packet` (patched): a custom payload of a player reached the server.
pub(in crate::game) fn server_play(game: &mut Game<'_>, host: &mut LanHost, conn: u32, p: &cfg::CustomPayload) {
    let Some(d) = fish_hooks::get() else {
        return;
    };
    let Some(g) = host.hub.guest(conn) else {
        return;
    };
    let (name, uuid) = (g.name.clone(), g.uuid.to_be_bytes());
    let received = NetPacket {
        phase: NetPhase::Play,
        channel: &p.channel,
        data: &p.data,
        from: Some((&name, uuid)),
    };
    d.net_receive(&mut GameView { game }, &received);
    drain(host);
}

/// A player entered the play phase (`load_guest_records`).
pub(in crate::game) fn server_joined(game: &mut Game<'_>, host: &LanHost, conn: u32) {
    let Some(g) = host.hub.guest(conn) else {
        return;
    };
    let (name, uuid) = (g.name.clone(), g.uuid.to_be_bytes());
    horizon_client::fish_lib::play_joined(&name);
    let mut e = GameEvent::new(Kind::NetServerJoin);
    e.role = role_of(game);
    e.actor = &name;
    e.actor_uuid = uuid;
    fire(game, &e);
}

/// A player's connection closed (`lan_crafting_left`).
pub(in crate::game) fn server_left(game: &mut Game<'_>, conn: u32, name: &str, uuid: u128, reason: &str) {
    if REPORTED.with(|r| r.borrow_mut().remove(&conn)) {
        return;
    }
    left(game, name, uuid, reason);
}

fn left(game: &mut Game<'_>, name: &str, uuid: u128, reason: &str) {
    horizon_client::fish_lib::play_left(name);
    let mut e = GameEvent::new(Kind::NetServerDisconnect);
    e.role = role_of(game);
    e.actor = name;
    e.actor_uuid = uuid.to_be_bytes();
    e.subject = reason;
    fire(game, &e);
}

/// The server drops every player as it stops, without a leave of their own.
pub(in crate::game) fn server_stopping(game: &mut Game<'_>) {
    let players: Vec<(u32, String, u128)> = game
        .lan
        .as_ref()
        .map(|h| {
            h.hub
                .guests
                .iter()
                .filter(|g| g.placed)
                .map(|g| (g.conn, g.name.clone(), g.uuid))
                .collect()
        })
        .unwrap_or_default();
    for (conn, name, uuid) in players {
        REPORTED.with(|r| r.borrow_mut().insert(conn));
        left(game, &name, uuid, "Server closed");
    }
}
