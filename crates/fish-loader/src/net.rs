//! Networking: payload types, receivers, the Fish handshake and registry synchronisation.
//!
//! The wire is a [`NetOut`] (the edition's bridge, or an in-memory wire in tests); this module
//! owns everything above it. Custom payloads that mods registered are delivered to their
//! receivers; `fish:handshake` (configuration phase, both directions) tells each side which mods
//! and receivers the other has; `fish:registry_sync` (server to client) renumbers the client's
//! registries to the server's raw ids.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::c_void;
use std::sync::{Arc, Mutex, RwLock};

use fish_abi::networking::{
    C2S_CONFIG, C2S_PLAY, HANDSHAKE_CHANNEL, MAX_C2S, MAX_S2C, NetPayloadV1, NetRecvFn,
    REGISTRY_SYNC_CHANNEL, S2C_CONFIG, S2C_PLAY,
};
use fish_abi::{DropUserFn, FishHandle, FishStatus, FishStr};
use fish_hooks::{ClientView, NetOut, NetPacket, NetPhase, Role};
use fish_metadata::Environment;
use serde::{Deserialize, Serialize};

use crate::inner::Inner;

/// The phase a direction travels in.
pub(crate) fn phase_of(dir: u8) -> NetPhase {
    if dir == S2C_CONFIG || dir == C2S_CONFIG {
        NetPhase::Configuration
    } else {
        NetPhase::Play
    }
}

/// True for the two clientbound directions.
fn is_s2c(dir: u8) -> bool {
    dir == S2C_PLAY || dir == S2C_CONFIG
}

/// The hard size limit of a direction.
pub(crate) fn limit_of(dir: u8) -> usize {
    if is_s2c(dir) { MAX_S2C } else { MAX_C2S }
}

struct UserPtr(*mut c_void);

// SAFETY: handed back only to the registering mod's own callback, which is `Send + Sync`.
unsafe impl Send for UserPtr {}
// SAFETY: see above.
unsafe impl Sync for UserPtr {}

struct Receiver {
    func: NetRecvFn,
    user: UserPtr,
    drop_user: Option<DropUserFn>,
    owner: u64,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        if let Some(f) = self.drop_user {
            // SAFETY: handed over together with its destructor by `register_receiver`; runs once.
            unsafe { f(self.user.0) };
        }
    }
}

/// One mod as announced in the handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HelloMod {
    id: String,
    version: String,
    /// `*`, `client` or `server`.
    env: String,
    /// True when the mod registered content that both sides must agree on.
    sync: bool,
}

/// What one side tells the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Hello {
    v: u32,
    fish: String,
    abi: [u16; 2],
    mods: Vec<HelloMod>,
    hash: String,
    /// Direction number to the channels this side receives in it.
    channels: BTreeMap<u8, Vec<String>>,
}

impl Hello {
    fn syncs(&self) -> BTreeMap<&str, &str> {
        self.mods
            .iter()
            .filter(|m| m.sync)
            .map(|m| (m.id.as_str(), m.version.as_str()))
            .collect()
    }
}

/// What differs between the synchronised mods of a server and a client.
pub(crate) fn mismatch(server: &Hello, client: &Hello) -> Option<String> {
    let (s, c) = (server.syncs(), client.syncs());
    let ids: std::collections::BTreeSet<&str> = s.keys().chain(c.keys()).copied().collect();
    let parts: Vec<String> = ids
        .into_iter()
        .filter(|id| s.get(id) != c.get(id))
        .map(|id| {
            format!(
                "{id} {} (server) / {} (client)",
                s.get(id).unwrap_or(&"none"),
                c.get(id).unwrap_or(&"none")
            )
        })
        .collect();
    (!parts.is_empty()).then(|| format!("Missing or different Fish mods: {}", parts.join(", ")))
}

/// A registry entry.
#[derive(Debug, Clone)]
struct Entry {
    id: String,
    raw: u32,
    /// Both sides must agree on it (its owner runs on both).
    synced: bool,
}

/// The raw-id tables of every Fish registry.
#[derive(Debug, Default)]
pub(crate) struct RegistryTables {
    registries: BTreeMap<String, Vec<Entry>>,
    /// Tokens of mods that registered at least one synchronised entry.
    owners: HashSet<u64>,
}

impl RegistryTables {
    /// Adds an entry and returns its raw id (registration order within the registry).
    pub(crate) fn register(&mut self, registry: &str, id: &str, synced: bool, owner: u64) -> u32 {
        let entries = self.registries.entry(registry.to_string()).or_default();
        if let Some(e) = entries.iter().find(|e| e.id == id) {
            return e.raw;
        }
        let raw = entries.len() as u32;
        entries.push(Entry {
            id: id.to_string(),
            raw,
            synced,
        });
        if synced {
            self.owners.insert(owner);
        }
        raw
    }

    pub(crate) fn raw(&self, registry: &str, id: &str) -> Option<u32> {
        self.registries
            .get(registry)?
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.raw)
    }

    pub(crate) fn entries(&self, registry: &str) -> Vec<(String, u32)> {
        self.registries
            .get(registry)
            .map(|v| v.iter().map(|e| (e.id.clone(), e.raw)).collect())
            .unwrap_or_default()
    }

    /// The synchronised entries, by registry.
    fn snapshot(&self) -> BTreeMap<String, Vec<(String, u32)>> {
        self.registries
            .iter()
            .map(|(name, entries)| {
                (name.clone(), {
                    let mut v: Vec<(String, u32)> = entries
                        .iter()
                        .filter(|e| e.synced)
                        .map(|e| (e.id.clone(), e.raw))
                        .collect();
                    v.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
                    v
                })
            })
            .filter(|(_, v)| !v.is_empty())
            .collect()
    }

    /// A stable hash of the synchronised entries (FNV-1a, 64 bit, hex).
    pub(crate) fn hash(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |bytes: &[u8]| {
            for b in bytes {
                h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for (name, entries) in self.snapshot() {
            feed(name.as_bytes());
            feed(&[0]);
            for (id, raw) in entries {
                feed(id.as_bytes());
                feed(&[0]);
                feed(&raw.to_le_bytes());
            }
            feed(&[1]);
        }
        format!("{h:016x}")
    }

    /// Renumbers the local registries to the server's raw ids. Returns the number of entries
    /// remapped, or a message naming the entries one side lacks.
    fn apply(&mut self, server: &BTreeMap<String, Vec<(String, u32)>>) -> Result<usize, String> {
        let mut problems = Vec::new();
        for (name, theirs) in server {
            let ours = self.registries.get(name).map_or(&[][..], Vec::as_slice);
            let mine: Vec<&str> = ours
                .iter()
                .filter(|e| e.synced)
                .map(|e| e.id.as_str())
                .collect();
            let missing_here: Vec<&str> = theirs
                .iter()
                .map(|(id, _)| id.as_str())
                .filter(|id| !mine.contains(id))
                .collect();
            let missing_there: Vec<&str> = mine
                .iter()
                .copied()
                .filter(|id| !theirs.iter().any(|(t, _)| t == id))
                .collect();
            if !missing_here.is_empty() || !missing_there.is_empty() {
                problems.push(format!(
                    "registry {name} differs (missing on the client: {}; missing on the server: {})",
                    join_or_none(&missing_here),
                    join_or_none(&missing_there)
                ));
            }
        }
        for (name, entries) in &self.registries {
            if !server.contains_key(name) && entries.iter().any(|e| e.synced) {
                problems.push(format!("registry {name} is unknown to the server"));
            }
        }
        if !problems.is_empty() {
            return Err(problems.join("; "));
        }
        let mut changed = 0;
        for (name, theirs) in server {
            if let Some(ours) = self.registries.get_mut(name) {
                for e in ours.iter_mut().filter(|e| e.synced) {
                    if let Some((_, raw)) = theirs.iter().find(|(id, _)| *id == e.id)
                        && e.raw != *raw
                    {
                        e.raw = *raw;
                        changed += 1;
                    }
                }
            }
        }
        Ok(changed)
    }
}

fn join_or_none(v: &[&str]) -> String {
    if v.is_empty() {
        "none".to_string()
    } else {
        v.join(", ")
    }
}

#[derive(Default)]
struct Peer {
    hello: Option<Hello>,
    /// The connection reached the play phase.
    joined: bool,
}

/// The networking state of one loader.
#[derive(Default)]
pub(crate) struct Net {
    /// `(direction, id)` to `(max size, owner token)`.
    types: Mutex<HashMap<(u8, String), (u32, u64)>>,
    receivers: Mutex<HashMap<(u8, String), Arc<Receiver>>>,
    /// Peers by name: the server's view of each client, or `""` for the client's view of the server.
    peers: Mutex<HashMap<String, Peer>>,
    out: RwLock<Option<Arc<dyn NetOut>>>,
    pub(crate) registries: Mutex<RegistryTables>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Net {
    /// Replaces the wire of this loader (tests; the edition uses the process-wide one).
    pub(crate) fn attach(&self, out: Arc<dyn NetOut>) {
        *self.out.write().unwrap_or_else(|e| e.into_inner()) = Some(out);
    }

    fn with_out<R>(&self, f: impl FnOnce(&dyn NetOut) -> R) -> Option<R> {
        if let Some(o) = self.out.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
            return Some(f(&**o));
        }
        fish_hooks::net_out().map(f)
    }

    /// Drops receivers before the mod libraries unload.
    pub(crate) fn clear(&self) {
        lock(&self.receivers).clear();
    }

    pub(crate) fn register_type(&self, owner: u64, dir: u8, id: &str, max: u32) -> FishStatus {
        if dir > C2S_CONFIG || !fish_abi::ident::is_valid_identifier(id) {
            return FishStatus::INVALID_ARGUMENT;
        }
        if id == HANDSHAKE_CHANNEL || id == REGISTRY_SYNC_CHANNEL || id.starts_with("minecraft:") {
            return FishStatus::INVALID_ARGUMENT;
        }
        if max == 0 || max as usize > limit_of(dir) {
            return FishStatus::INVALID_ARGUMENT;
        }
        let mut types = lock(&self.types);
        match types.get(&(dir, id.to_string())) {
            Some((m, _)) if *m == max => FishStatus::OK,
            Some(_) => FishStatus::ALREADY_EXISTS,
            None => {
                types.insert((dir, id.to_string()), (max, owner));
                FishStatus::OK
            }
        }
    }

    pub(crate) fn register_receiver(
        &self,
        owner: u64,
        dir: u8,
        id: &str,
        func: NetRecvFn,
        user: *mut c_void,
        drop_user: Option<DropUserFn>,
    ) -> FishStatus {
        let receiver = Receiver {
            func,
            user: UserPtr(user),
            drop_user,
            owner,
        };
        if !lock(&self.types).contains_key(&(dir, id.to_string())) {
            return FishStatus::NOT_FOUND;
        }
        let mut receivers = lock(&self.receivers);
        if receivers.contains_key(&(dir, id.to_string())) {
            return FishStatus::ALREADY_EXISTS;
        }
        receivers.insert((dir, id.to_string()), Arc::new(receiver));
        FishStatus::OK
    }

    /// True when the remote `peer` announced a receiver for `id` in `dir`, or when nothing is
    /// known about the peer yet (then sending is allowed and the remote decides).
    fn remote_accepts(&self, peer: &str, dir: u8, id: &str) -> Result<bool, ()> {
        let peers = lock(&self.peers);
        match peers.get(peer).and_then(|p| p.hello.as_ref()) {
            Some(h) => Ok(h
                .channels
                .get(&dir)
                .is_some_and(|c| c.iter().any(|x| x == id))),
            None => Err(()),
        }
    }

    /// Nonzero when the peer can receive `id` in `dir`.
    pub(crate) fn can_send(&self, dir: u8, peer: &str, id: &str) -> bool {
        self.remote_accepts(peer, dir, id).unwrap_or(false)
    }

    fn check_send(&self, dir: u8, id: &str, len: usize) -> Result<(), FishStatus> {
        let max = lock(&self.types)
            .get(&(dir, id.to_string()))
            .map(|(m, _)| *m as usize)
            .ok_or(FishStatus::NOT_FOUND)?;
        if len > max || len > limit_of(dir) {
            return Err(FishStatus::INVALID_ARGUMENT);
        }
        Ok(())
    }

    /// Client to server.
    pub(crate) fn send_to_server(&self, dir: u8, id: &str, data: &[u8]) -> FishStatus {
        if dir != C2S_PLAY && dir != C2S_CONFIG {
            return FishStatus::INVALID_ARGUMENT;
        }
        if let Err(st) = self.check_send(dir, id, data.len()) {
            return st;
        }
        if self.remote_accepts("", dir, id) == Ok(false) {
            return FishStatus::UNSUPPORTED;
        }
        match self.with_out(|o| o.to_server(phase_of(dir), id, data)) {
            Some(Ok(())) => FishStatus::OK,
            Some(Err(_)) => FishStatus::INTERNAL,
            None => FishStatus::UNSUPPORTED,
        }
    }

    /// Server to one player, or to everyone when `player` is empty (clients that cannot receive
    /// the channel are skipped).
    pub(crate) fn send_to_player(
        &self,
        dir: u8,
        player: &str,
        id: &str,
        data: &[u8],
    ) -> FishStatus {
        if dir != S2C_PLAY && dir != S2C_CONFIG {
            return FishStatus::INVALID_ARGUMENT;
        }
        if let Err(st) = self.check_send(dir, id, data.len()) {
            return st;
        }
        let phase = phase_of(dir);
        if !player.is_empty() {
            if self.remote_accepts(player, dir, id) == Ok(false) {
                return FishStatus::UNSUPPORTED;
            }
            return match self.with_out(|o| o.to_player(phase, Some(player), id, data)) {
                Some(Ok(())) => FishStatus::OK,
                Some(Err(_)) => FishStatus::NOT_FOUND,
                None => FishStatus::UNSUPPORTED,
            };
        }
        let Some(names) = self.with_out(|o| o.players(phase)) else {
            return FishStatus::UNSUPPORTED;
        };
        for name in names {
            if self.remote_accepts(&name, dir, id) == Ok(false) {
                continue;
            }
            let _ = self.with_out(|o| o.to_player(phase, Some(&name), id, data));
        }
        FishStatus::OK
    }

    pub(crate) fn players(&self, dir: u8) -> Vec<String> {
        self.with_out(|o| o.players(phase_of(dir)))
            .unwrap_or_default()
    }

    fn disconnect(&self, player: Option<&str>, reason: &str) {
        let _ = self.with_out(|o| o.disconnect(player, reason));
    }

    fn send_raw(&self, dir: u8, player: Option<&str>, channel: &str, data: &[u8]) {
        let phase = phase_of(dir);
        let _ = self.with_out(|o| match player {
            Some(p) => o.to_player(phase, Some(p), channel, data),
            None => o.to_server(phase, channel, data),
        });
    }
}

impl Inner {
    /// What this side tells the other in the handshake.
    fn hello(&self) -> Hello {
        let tables = lock(&self.net.registries);
        let mods = self
            .records()
            .iter()
            .filter(|r| !r.is_failed() && r.manifest.library.is_some())
            .map(|r| HelloMod {
                id: r.manifest.id.to_string(),
                version: r.version_text.clone(),
                env: match r.manifest.environment {
                    Environment::Any => "*",
                    Environment::Client => "client",
                    Environment::Server => "server",
                }
                .to_string(),
                sync: r.manifest.environment == Environment::Any
                    && tables.owners.contains(&r.token),
            })
            .collect();
        let mut channels: BTreeMap<u8, Vec<String>> = BTreeMap::new();
        for (dir, id) in lock(&self.net.receivers).keys() {
            channels.entry(*dir).or_default().push(id.clone());
        }
        for v in channels.values_mut() {
            v.sort();
        }
        Hello {
            v: 1,
            fish: self.fish_version.to_string(),
            abi: [fish_abi::FISH_ABI_MAJOR, fish_abi::FISH_ABI_MINOR],
            mods,
            hash: tables.hash(),
            channels,
        }
    }

    /// True when some mod registered content both sides must have.
    fn requires_sync(&self) -> bool {
        self.hello().mods.iter().any(|m| m.sync)
    }

    fn synced_mod_names(&self) -> String {
        let names: Vec<String> = self
            .hello()
            .mods
            .iter()
            .filter(|m| m.sync)
            .map(|m| format!("{} {}", m.id, m.version))
            .collect();
        names.join(", ")
    }

    /// Reacts to a connection event (before mods see it).
    pub(crate) fn net_connection(&self, kind: fish_hooks::Kind, player: &str) {
        use fish_hooks::Kind;
        let net = &self.net;
        match kind {
            Kind::NetServerInit => {
                lock(&net.peers).insert(player.to_string(), Peer::default());
                let hello = serde_json::to_vec(&self.hello()).unwrap_or_default();
                net.send_raw(S2C_CONFIG, Some(player), HANDSHAKE_CHANNEL, &hello);
            }
            Kind::NetServerJoin => {
                let known = {
                    let mut peers = lock(&net.peers);
                    let peer = peers.entry(player.to_string()).or_default();
                    peer.joined = true;
                    peer.hello.is_some()
                };
                if !known && self.requires_sync() {
                    net.disconnect(
                        Some(player),
                        &format!(
                            "This server requires Fish mods, but your client does not run Fish: {}",
                            self.synced_mod_names()
                        ),
                    );
                }
            }
            Kind::NetServerDisconnect => {
                lock(&net.peers).remove(player);
            }
            Kind::NetClientInit => {
                let mut peers = lock(&net.peers);
                peers.clear();
                peers.insert(String::new(), Peer::default());
            }
            Kind::NetClientJoin => {
                let known = {
                    let mut peers = lock(&net.peers);
                    let peer = peers.entry(String::new()).or_default();
                    peer.joined = true;
                    peer.hello.is_some()
                };
                if !known && self.requires_sync() {
                    net.disconnect(
                        None,
                        &format!(
                            "This server does not run Fish, which these mods need: {}",
                            self.synced_mod_names()
                        ),
                    );
                }
            }
            Kind::NetClientDisconnect => {
                lock(&net.peers).clear();
            }
            _ => {}
        }
    }

    /// Handles a custom payload. Returns true when it was consumed.
    pub(crate) fn net_receive(&self, game: &mut dyn ClientView, packet: &NetPacket<'_>) -> bool {
        let server = game.role() != Role::Window;
        let dir = match (server, packet.phase) {
            (false, NetPhase::Play) => S2C_PLAY,
            (false, NetPhase::Configuration) => S2C_CONFIG,
            (true, NetPhase::Play) => C2S_PLAY,
            (true, NetPhase::Configuration) => C2S_CONFIG,
        };
        if packet.channel == HANDSHAKE_CHANNEL {
            if dir == S2C_CONFIG || dir == C2S_CONFIG {
                self.on_handshake(dir, packet);
            }
            return true;
        }
        if packet.channel == REGISTRY_SYNC_CHANNEL {
            if dir == S2C_CONFIG {
                self.on_registry_sync(packet);
            }
            return true;
        }
        let key = (dir, packet.channel.to_string());
        let Some((max, _)) = lock(&self.net.types).get(&key).copied() else {
            return false;
        };
        if packet.data.len() > max as usize || packet.data.len() > limit_of(dir) {
            self.warn(format!(
                "dropped a {}-byte payload on {}: the limit is {} bytes",
                packet.data.len(),
                packet.channel,
                max
            ));
            return true;
        }
        let Some(receiver) = lock(&self.net.receivers).get(&key).cloned() else {
            return true;
        };
        if self.record(receiver.owner).is_some_and(|r| r.is_failed()) {
            return true;
        }
        let (sender, uuid) = packet.from.unwrap_or(("", [0; 16]));
        let status = self.handles.client_scope(game, |view: FishHandle| {
            let payload = NetPayloadV1 {
                size: size_of::<NetPayloadV1>() as u32,
                dir,
                reserved: [0; 3],
                channel: FishStr::new(packet.channel),
                data: packet.data.as_ptr(),
                len: packet.data.len(),
                sender: FishStr::new(sender),
                sender_uuid: uuid,
                view,
            };
            // SAFETY: the receiver and its `user` come from one registration; the payload and the
            // buffers it points to outlive the call.
            unsafe { (receiver.func)(receiver.user.0, &payload) }
        });
        if status == FishStatus::PANICKED {
            self.fail_mod(
                receiver.owner,
                format!("a receiver of {} panicked", packet.channel),
                true,
            );
        }
        true
    }

    fn on_handshake(&self, dir: u8, packet: &NetPacket<'_>) {
        let net = &self.net;
        let Ok(remote) = serde_json::from_slice::<Hello>(packet.data) else {
            self.warn(format!("ignored a malformed {HANDSHAKE_CHANNEL} payload"));
            return;
        };
        let local = self.hello();
        if dir == S2C_CONFIG {
            // client: the server introduced itself
            lock(&net.peers).entry(String::new()).or_default().hello = Some(remote.clone());
            if let Some(why) = mismatch(&remote, &local) {
                net.disconnect(None, &why);
                return;
            }
            let reply = serde_json::to_vec(&local).unwrap_or_default();
            net.send_raw(C2S_CONFIG, None, HANDSHAKE_CHANNEL, &reply);
        } else {
            // server: a client answered
            let name = packet.from.map_or("", |f| f.0).to_string();
            lock(&net.peers).entry(name.clone()).or_default().hello = Some(remote.clone());
            if let Some(why) = mismatch(&local, &remote) {
                net.disconnect(Some(&name), &why);
                return;
            }
            let snapshot = lock(&net.registries).snapshot();
            if !snapshot.is_empty() {
                let body = serde_json::json!({ "registries": snapshot });
                net.send_raw(
                    S2C_CONFIG,
                    Some(&name),
                    REGISTRY_SYNC_CHANNEL,
                    body.to_string().as_bytes(),
                );
            }
        }
    }

    fn on_registry_sync(&self, packet: &NetPacket<'_>) {
        #[derive(Deserialize)]
        struct Sync {
            registries: BTreeMap<String, Vec<(String, u32)>>,
        }
        let Ok(sync) = serde_json::from_slice::<Sync>(packet.data) else {
            self.warn(format!(
                "ignored a malformed {REGISTRY_SYNC_CHANNEL} payload"
            ));
            return;
        };
        let result = lock(&self.net.registries).apply(&sync.registries);
        match result {
            Ok(n) => self.emit(
                log::Level::Info,
                "fish_loader",
                format!(
                    "synchronised {} registr(ies) with the server ({n} id(s) renumbered)",
                    sync.registries.len()
                ),
            ),
            Err(why) => self.net.disconnect(
                None,
                &format!("Fish registries do not match the server's: {why}"),
            ),
        }
    }
}
