//! An in-memory server and clients, wired through the loader's real networking.
//!
//! Each side is a booted [`TestHost`] with its own loader; [`Network`] is their wire. Packets a
//! side sends are queued and delivered by [`Network::pump`] to the other side's
//! [`Dispatch::net_receive`](fish_hooks::Dispatch::net_receive), so the handshake, registry
//! synchronisation, limits and receivers run exactly as they do in the game.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use fish_hooks::{Dispatch, GameEvent, Kind, NetOut, NetPacket, NetPhase};

use crate::TestHost;
use crate::events::OVERWORLD;

/// One payload in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Flight {
    player: String,
    phase: NetPhase,
    channel: String,
    data: Vec<u8>,
}

#[derive(Default)]
struct Wire {
    to_clients: Mutex<VecDeque<Flight>>,
    to_server: Mutex<VecDeque<Flight>>,
    /// Pending disconnects: `(player, reason)` (`None` reason side is the client leaving).
    kicks: Mutex<Vec<(String, String)>>,
    config: Mutex<Vec<String>>,
    play: Mutex<Vec<String>>,
}

struct ServerWire(Arc<Wire>);

impl NetOut for ServerWire {
    fn to_server(&self, _: NetPhase, _: &str, _: &[u8]) -> Result<(), String> {
        Err("this is the server".to_string())
    }

    fn to_player(
        &self,
        phase: NetPhase,
        player: Option<&str>,
        channel: &str,
        data: &[u8],
    ) -> Result<(), String> {
        let list = match phase {
            NetPhase::Configuration => &self.0.config,
            NetPhase::Play => &self.0.play,
        };
        let names: Vec<String> = match player {
            Some(p) => vec![p.to_string()],
            None => list.lock().unwrap().clone(),
        };
        for name in names {
            if !list.lock().unwrap().contains(&name) {
                return Err(format!("{name} is not connected in {phase:?}"));
            }
            self.0.to_clients.lock().unwrap().push_back(Flight {
                player: name,
                phase,
                channel: channel.to_string(),
                data: data.to_vec(),
            });
        }
        Ok(())
    }

    fn disconnect(&self, player: Option<&str>, reason: &str) {
        if let Some(p) = player {
            self.0
                .kicks
                .lock()
                .unwrap()
                .push((p.to_string(), reason.to_string()));
        }
    }

    fn players(&self, phase: NetPhase) -> Vec<String> {
        match phase {
            NetPhase::Configuration => self.0.config.lock().unwrap().clone(),
            NetPhase::Play => self.0.play.lock().unwrap().clone(),
        }
    }
}

struct ClientWire {
    wire: Arc<Wire>,
    name: String,
}

impl NetOut for ClientWire {
    fn to_server(&self, phase: NetPhase, channel: &str, data: &[u8]) -> Result<(), String> {
        self.wire.to_server.lock().unwrap().push_back(Flight {
            player: self.name.clone(),
            phase,
            channel: channel.to_string(),
            data: data.to_vec(),
        });
        Ok(())
    }

    fn to_player(&self, _: NetPhase, _: Option<&str>, _: &str, _: &[u8]) -> Result<(), String> {
        Err("this is a client".to_string())
    }

    fn disconnect(&self, _: Option<&str>, reason: &str) {
        self.wire
            .kicks
            .lock()
            .unwrap()
            .push((self.name.clone(), reason.to_string()));
    }

    fn players(&self, _: NetPhase) -> Vec<String> {
        Vec::new()
    }
}

fn uuid_of(name: &str) -> [u8; 16] {
    let mut h: u128 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h = (h ^ u128::from(b)).wrapping_mul(0x0000_0100_0000_01b3_0000_0000_0000_013b);
    }
    (h | 1).to_be_bytes()
}

/// A dedicated server and its clients in one process.
pub struct Network {
    /// The server (boot it with `.side(Side::Server)`).
    pub server: TestHost,
    /// The connected or connecting clients, by player name.
    pub clients: BTreeMap<String, TestHost>,
    wire: Arc<Wire>,
    vanilla_server: bool,
    vanilla: std::collections::BTreeSet<String>,
    /// Why each player was disconnected, in order: `(player, reason)`.
    pub disconnects: Vec<(String, String)>,
}

impl Network {
    /// A network around a booted server.
    pub fn new(server: TestHost) -> Network {
        let wire = Arc::new(Wire::default());
        server
            .loader()
            .attach_net(Arc::new(ServerWire(wire.clone())));
        Network {
            server,
            clients: BTreeMap::new(),
            wire,
            vanilla_server: false,
            vanilla: Default::default(),
            disconnects: Vec::new(),
        }
    }

    /// Adds a booted client for player `name` (not connected yet).
    pub fn add_client(&mut self, name: &str, client: TestHost) {
        client.loader().attach_net(Arc::new(ClientWire {
            wire: self.wire.clone(),
            name: name.to_string(),
        }));
        self.clients.insert(name.to_string(), client);
    }

    /// Adds a player whose game does not run Fish: it never answers a payload.
    pub fn add_vanilla_client(&mut self, name: &str) {
        self.vanilla.insert(name.to_string());
    }

    /// Makes the server behave as one that does not run Fish: it never sends or answers a payload.
    pub fn set_vanilla_server(&mut self, vanilla: bool) {
        self.vanilla_server = vanilla;
    }

    fn server_event(&mut self, kind: Kind, name: &str, reason: &str) {
        let mut e = GameEvent::new(kind);
        e.actor = name;
        e.actor_uuid = uuid_of(name);
        e.dimension = OVERWORLD;
        e.subject = reason;
        self.server.game_event(&e);
    }

    fn client_event(&mut self, kind: Kind, name: &str, reason: &str) {
        let mut e = GameEvent::new(kind);
        e.subject = reason;
        if let Some(c) = self.clients.get_mut(name) {
            c.game_event(&e);
        }
    }

    /// Connects `name`: the configuration phase (handshake, registry sync), then the play phase,
    /// unless the handshake got the player disconnected. Returns true when the player is in play.
    pub fn connect(&mut self, name: &str) -> bool {
        self.wire.config.lock().unwrap().push(name.to_string());
        self.server_event(Kind::NetServerInit, name, "");
        self.client_event(Kind::NetClientInit, name, "");
        self.pump();
        if !self.wire.config.lock().unwrap().contains(&name.to_string()) {
            return false;
        }
        self.wire.config.lock().unwrap().retain(|n| n != name);
        self.wire.play.lock().unwrap().push(name.to_string());
        self.server_event(Kind::NetServerJoin, name, "");
        self.client_event(Kind::NetClientJoin, name, "");
        self.pump();
        self.wire.play.lock().unwrap().contains(&name.to_string())
    }

    /// The player leaves of their own accord.
    pub fn leave(&mut self, name: &str) {
        self.drop_player(name, "disconnected");
    }

    fn drop_player(&mut self, name: &str, reason: &str) {
        self.wire.config.lock().unwrap().retain(|n| n != name);
        self.wire.play.lock().unwrap().retain(|n| n != name);
        self.wire
            .to_clients
            .lock()
            .unwrap()
            .retain(|f| f.player != name);
        self.wire
            .to_server
            .lock()
            .unwrap()
            .retain(|f| f.player != name);
        self.disconnects
            .push((name.to_string(), reason.to_string()));
        self.server_event(Kind::NetServerDisconnect, name, reason);
        self.client_event(Kind::NetClientDisconnect, name, reason);
    }

    /// Delivers queued payloads in both directions until nothing is left, then applies
    /// disconnects (kicks by the server, or a client leaving because of a mismatch).
    pub fn pump(&mut self) {
        for _ in 0..1000 {
            let kick = self.wire.kicks.lock().unwrap().pop();
            if let Some((player, reason)) = kick {
                self.drop_player(&player, &reason);
                continue;
            }
            let down = self.wire.to_clients.lock().unwrap().pop_front();
            if let Some(f) = down {
                if self.vanilla_server {
                    continue;
                }
                if self.vanilla.contains(&f.player) {
                    continue;
                }
                if let Some(client) = self.clients.get_mut(&f.player) {
                    client.net_receive(&NetPacket {
                        phase: f.phase,
                        channel: &f.channel,
                        data: &f.data,
                        from: None,
                    });
                }
                continue;
            }
            let up = self.wire.to_server.lock().unwrap().pop_front();
            if let Some(f) = up {
                if self.vanilla_server {
                    continue;
                }
                self.server.net_receive(&NetPacket {
                    phase: f.phase,
                    channel: &f.channel,
                    data: &f.data,
                    from: Some((&f.player, uuid_of(&f.player))),
                });
                continue;
            }
            return;
        }
        panic!("the simulated network did not settle (a mod keeps answering itself)");
    }

    /// Payloads the server has queued for `name` and that were not delivered yet.
    pub fn pending_for(&self, name: &str) -> usize {
        self.wire
            .to_clients
            .lock()
            .unwrap()
            .iter()
            .filter(|f| f.player == name)
            .count()
    }
}

impl TestHost {
    /// Delivers a custom payload to this side as the game would; true when Fish consumed it.
    pub fn net_receive(&mut self, packet: &NetPacket<'_>) -> bool {
        let role = self.game_role();
        let (mut view, dispatch) = self.view(role);
        let consumed = dispatch.net_receive(&mut view, packet);
        self.drain_trace();
        consumed
    }
}
