// Fish's support code inside the horizon-client library: the hooks of library files call it, and
// the bridge in the game module reuses its thread-locals. Plain comments only (no inner docs).

use std::cell::{Cell, RefCell};

use fish_hooks::{GameEvent, Kind, Outcome, Role};

static CLIENT_STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

thread_local! {
    // Set by the game's tick hook: what this thread's game is, for hooks that have no game at hand.
    static ROLE: Cell<Role> = const { Cell::new(Role::Window) };
    static DIMENSION: Cell<&'static str> = const { Cell::new("minecraft:overworld") };
    // Set while a server stops, so that the later `lan_close` reports the stop as finished.
    static STOPPING: Cell<bool> = const { Cell::new(false) };
    // The player a hook last acted for (player damage has no game at hand).
    static ACTOR: RefCell<(String, [u8; 16])> = const { RefCell::new((String::new(), [0; 16])) };
}

/// The events the loader derives from `kind`: an event nobody listens to still has to be raised
/// when one of these has a listener.
pub fn derived(kind: Kind) -> &'static [Kind] {
    match kind {
        Kind::BlockBreakBefore => &[Kind::BlockBreakCanceled, Kind::BlockBreakAfter],
        Kind::AllowDamage => &[Kind::AfterDamage],
        Kind::AllowSend => &[Kind::ModifySend],
        Kind::ServerStopping => &[Kind::WorldUnload],
        Kind::ReloadStart => &[Kind::ReloadEnd],
        _ => &[],
    }
}

/// True when a dispatcher is installed and has a listener for `kind` or for an event derived from it.
pub fn wants(kind: Kind) -> bool {
    fish_hooks::get().is_some_and(|d| d.wants(kind) || derived(kind).iter().any(|k| d.wants(*k)))
}

/// An event for a hook with no game: role and dimension are what the tick hook last noted.
pub fn event(kind: Kind) -> GameEvent<'static> {
    let mut e = GameEvent::new(kind);
    e.role = ROLE.with(Cell::get);
    e.is_client = e.role == Role::Window;
    e.dimension = DIMENSION.with(Cell::get);
    e
}

/// Fires `event` with no game view.
pub fn fire(event: &GameEvent<'_>) -> Outcome {
    match fish_hooks::get() {
        Some(d) => d.game_event(None, event),
        None => Outcome::Pass,
    }
}

/// Notes which game ticks on this thread.
pub fn note_context(role: Role, dimension: &'static str) {
    ROLE.with(|r| r.set(role));
    DIMENSION.with(|d| d.set(dimension));
    fish_hooks::note_thread_context(role, dimension);
}

/// A server on this thread begins to stop.
pub fn mark_stopping() {
    STOPPING.with(|s| s.set(true));
}

/// True once after `mark_stopping`.
pub fn take_stopping() -> bool {
    STOPPING.with(|s| s.replace(false))
}

/// Remembers the player the game acts for.
pub fn set_actor(name: &str, uuid: [u8; 16]) {
    ACTOR.with(|a| {
        let mut a = a.borrow_mut();
        if a.0 != name {
            a.0.clear();
            a.0.push_str(name);
        }
        a.1 = uuid;
    });
}

fn with_actor<R>(f: impl FnOnce(&str, [u8; 16]) -> R) -> R {
    ACTOR.with(|a| {
        let a = a.borrow();
        f(&a.0, a.1)
    })
}

/// Fires an allow-style event; true when nobody cancelled it.
fn allowed(event: &GameEvent<'_>) -> bool {
    !fire(event).cancelled()
}

/// `ChunkMap::insert`: a chunk entered the loaded set (a replacement is not a load).
pub fn chunk_insert(existed: bool, key: (i32, i32)) {
    if existed || !wants(Kind::ChunkLoad) {
        return;
    }
    let mut e = event(Kind::ChunkLoad);
    e.pos = [key.0, 0, key.1];
    fire(&e);
}

/// `ChunkMap::remove`: a chunk leaves the loaded set.
pub fn chunk_remove(exists: bool, key: (i32, i32)) {
    if !exists || !wants(Kind::ChunkUnload) {
        return;
    }
    let mut e = event(Kind::ChunkUnload);
    e.pos = [key.0, 0, key.1];
    fire(&e);
}

/// `pick_item::pick`: false vetoes the pick.
pub fn pick_item(item: &str) -> bool {
    if !wants(Kind::PickItem) {
        return true;
    }
    let mut e = event(Kind::PickItem);
    e.item = item;
    allowed(&e)
}

/// A mob is about to take damage: false vetoes it. `kind` is the damage kind (its Debug name).
/// `health` is what the mob has: a hit of at least that much also raises `allow_death` (the raw
/// amount, before armor: an approximation), and cancelling it rejects the damage.
pub fn allow_mob_damage(
    id: u32,
    entity_type: &str,
    kind: &dyn std::fmt::Debug,
    amount: f32,
    health: f32,
) -> bool {
    if !wants(Kind::AllowDamage) && !wants(Kind::AllowDeath) {
        return true;
    }
    let label = format!("minecraft:{}", snake(&format!("{kind:?}")));
    let mut e = event(Kind::AllowDamage);
    e.entity_id = i64::from(id);
    e.subject = entity_type;
    e.extra = &label;
    e.amount = amount;
    if !allowed(&e) {
        return false;
    }
    if amount >= health && wants(Kind::AllowDeath) {
        e.kind = Kind::AllowDeath;
        return allowed(&e);
    }
    true
}

/// True when a listener wants player damage events.
pub fn wants_damage() -> bool {
    wants(Kind::AllowDamage) || wants(Kind::AllowDeath)
}

/// True when a listener wants game (system) message events.
pub fn wants_game() -> bool {
    wants(Kind::AllowGame) || wants(Kind::Game)
}

fn snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The player is about to take damage: false vetoes it. The actor is the player the game last acted for.
/// `health` (with absorption) is what the player has: a hit of at least that much also raises `allow_death`.
pub fn allow_player_damage(kind: &str, amount: f32, health: f32) -> bool {
    if !wants(Kind::AllowDamage) && !wants(Kind::AllowDeath) {
        return true;
    }
    with_actor(|name, uuid| {
        let mut e = event(Kind::AllowDamage);
        e.subject = "minecraft:player";
        e.extra = kind;
        e.amount = amount;
        e.actor = name;
        e.actor_uuid = uuid;
        if !allowed(&e) {
            return false;
        }
        if amount >= health && wants(Kind::AllowDeath) {
            e.kind = Kind::AllowDeath;
            return allowed(&e);
        }
        true
    })
}

/// A player's chat line or command reached the server: false drops it.
pub fn allow_chat(name: &str, uuid: u128, message: &str, command: bool) -> bool {
    let kind = if command {
        Kind::AllowCommand
    } else {
        Kind::AllowChat
    };
    if !wants(kind) {
        return true;
    }
    let mut e = event(kind);
    e.actor = name;
    e.actor_uuid = uuid.to_be_bytes();
    e.subject = message;
    allowed(&e)
}

/// A player's chat line was broadcast.
pub fn chat_sent(name: &str, uuid: u128, message: &str) {
    if !wants(Kind::Chat) {
        return;
    }
    let mut e = event(Kind::Chat);
    e.actor = name;
    e.actor_uuid = uuid.to_be_bytes();
    e.subject = message;
    fire(&e);
}

/// The server broadcasts a system message: false drops it.
pub fn allow_game_message(text: &str) -> bool {
    if !wants(Kind::AllowGame) && !wants(Kind::Game) {
        return true;
    }
    let mut e = event(Kind::AllowGame);
    e.subject = text;
    if !allowed(&e) {
        return false;
    }
    let mut e = event(Kind::Game);
    e.subject = text;
    fire(&e);
    true
}

/// The player is about to see a chat line. `source` is `player`, `server` or `client`.
pub fn allow_receive(text: &str, source: &str) -> bool {
    if !wants(Kind::AllowReceive) && !wants(Kind::Receive) {
        return true;
    }
    let mut e = event(Kind::AllowReceive);
    e.subject = text;
    e.extra = source;
    if !allowed(&e) {
        return false;
    }
    let mut e = event(Kind::Receive);
    e.subject = text;
    e.extra = source;
    fire(&e);
    true
}

/// The window closed (detached: the game is gone).
pub fn client_stopped() {
    // The presence is stopped from two exits of the window: report the first.
    if !CLIENT_STOPPED.swap(true, std::sync::atomic::Ordering::Relaxed) && wants(Kind::ClientStopped) {
        fire(&event(Kind::ClientStopped));
    }
}

/// Appends the lines mods add to the F3 screen to the right column.
pub fn debug_columns(mut columns: (Vec<String>, Vec<String>)) -> (Vec<String>, Vec<String>) {
    let Some(d) = fish_hooks::get() else {
        return columns;
    };
    let mut lines = Vec::new();
    d.debug_lines(&mut lines);
    if !lines.is_empty() {
        columns.1.extend(lines);
        columns.1.push(String::new());
    }
    columns
}

/// `Creative::load` (patch 0008): the creative inventory with the tabs and entries mods
/// registered, or `None` when nothing changes.
pub fn creative_patch(raw: &serde_json::Value) -> Option<serde_json::Value> {
    let d = fish_hooks::get()?;
    let text = serde_json::to_string(raw).ok()?;
    let patched = d.creative_inventory(&text)?;
    match serde_json::from_str(&patched) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("Fish: the patched creative inventory is not JSON, the original is used: {e}");
            None
        }
    }
}

/// Puts what the loader reports into the crash report's details; returns the failed mods line.
pub fn refresh_crash_details() -> String {
    let mut failed = String::new();
    if let Some(d) = fish_hooks::get() {
        for (key, value) in d.crash_details() {
            if key == "Fish failed mods" {
                failed.clone_from(&value);
            }
            crate::crash::set_detail(key, value);
        }
    }
    failed
}

// ---- Networking: custom payloads over Horizon's connections ----
//
// Client: the session's writer is registered when the session is created (`client_register`);
// sends go straight to the connection in whichever phase it is in, and wait in `pending` while the
// writer does not exist yet. Server: a send is queued by player name; the connection's own thread
// writes configuration payloads (`server_config_*`, called from a patch in `lan/conn.rs`), the
// server tick writes play payloads (`fish_bridge::net`).

type SharedWriter = std::sync::Arc<std::sync::Mutex<Option<horizon_protocol::connection::ConnWriter>>>;

fn locked<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What a mod queued for a player who is configuring.
pub enum ConfigOut {
    Payload(String, Vec<u8>),
    Kick(String),
}

/// What a mod queued for a player in play.
pub enum PlayOut {
    Payload { player: Option<String>, channel: String, data: Vec<u8> },
    Kick { player: String, reason: String },
}

struct ClientConn {
    writer: SharedWriter,
    configuring: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

struct ClientSlot {
    live: bool,
    conn: Option<ClientConn>,
    pending: Vec<(fish_hooks::NetPhase, String, Vec<u8>)>,
}

static CLIENT: std::sync::Mutex<ClientSlot> = std::sync::Mutex::new(ClientSlot {
    live: false,
    conn: None,
    pending: Vec::new(),
});
// Set when the client has queued sends or a kick for the game thread to look at.
static CLIENT_DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CLIENT_KICK: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static PLAY_OUT: std::sync::Mutex<Vec<PlayOut>> = std::sync::Mutex::new(Vec::new());
static PLAY_PLAYERS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static CONFIG_PLAYERS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static CONFIG_OUT: std::sync::Mutex<Vec<(String, ConfigOut)>> = std::sync::Mutex::new(Vec::new());

/// A view for hooks that run without a `Game` (connection threads).
pub struct DetachedView {
    role: Role,
}

impl fish_hooks::ClientView for DetachedView {
    fn role(&self) -> Role {
        self.role
    }

    fn player(&self) -> Option<fish_hooks::PlayerSnapshot> {
        None
    }

    fn show_chat(&mut self, _text: &str) {}

    fn game_dir(&self) -> std::path::PathBuf {
        if self.role == Role::DedicatedServer {
            std::path::PathBuf::from(".")
        } else {
            crate::logging::game_home()
        }
    }
}

/// The role of server threads that have no game at hand.
fn server_role() -> Role {
    static ROLE: std::sync::LazyLock<Role> = std::sync::LazyLock::new(|| {
        if std::env::args().any(|a| a == "--dedicated") {
            Role::DedicatedServer
        } else {
            Role::IntegratedServer
        }
    });
    *ROLE
}

/// The wire the loader sends payloads through.
pub struct FishNet;

pub static NET: FishNet = FishNet;

/// A player name is not sent anywhere else: `None` means everyone.
fn matches_player(target: &Option<String>, name: &str) -> bool {
    target.as_deref().is_none_or(|t| t == name)
}

impl fish_hooks::NetOut for FishNet {
    fn to_server(&self, phase: fish_hooks::NetPhase, channel: &str, data: &[u8]) -> Result<(), String> {
        client_send(phase, channel, data)
    }

    fn to_player(
        &self,
        phase: fish_hooks::NetPhase,
        player: Option<&str>,
        channel: &str,
        data: &[u8],
    ) -> Result<(), String> {
        match phase {
            fish_hooks::NetPhase::Configuration => {
                let names: Vec<String> = {
                    let players = locked(&CONFIG_PLAYERS);
                    match player {
                        Some(p) if players.iter().any(|n| n == p) => vec![p.to_owned()],
                        Some(p) => return Err(format!("{p} is not in the configuration phase")),
                        None => players.clone(),
                    }
                };
                let mut out = locked(&CONFIG_OUT);
                for name in names {
                    out.push((name, ConfigOut::Payload(channel.to_owned(), data.to_vec())));
                }
                Ok(())
            }
            fish_hooks::NetPhase::Play => {
                if let Some(p) = player
                    && !locked(&PLAY_PLAYERS).iter().any(|n| n == p)
                {
                    return Err(format!("{p} is not playing on this server"));
                }
                locked(&PLAY_OUT).push(PlayOut::Payload {
                    player: player.map(str::to_owned),
                    channel: channel.to_owned(),
                    data: data.to_vec(),
                });
                Ok(())
            }
        }
    }

    fn disconnect(&self, player: Option<&str>, reason: &str) {
        let Some(player) = player else {
            *locked(&CLIENT_KICK) = Some(reason.to_owned());
            CLIENT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
            return;
        };
        if locked(&CONFIG_PLAYERS).iter().any(|n| n == player) {
            locked(&CONFIG_OUT).push((player.to_owned(), ConfigOut::Kick(reason.to_owned())));
        } else {
            locked(&PLAY_OUT).push(PlayOut::Kick {
                player: player.to_owned(),
                reason: reason.to_owned(),
            });
        }
    }

    fn players(&self, phase: fish_hooks::NetPhase) -> Vec<String> {
        match phase {
            fish_hooks::NetPhase::Configuration => locked(&CONFIG_PLAYERS).clone(),
            fish_hooks::NetPhase::Play => locked(&PLAY_PLAYERS).clone(),
        }
    }
}

// ---- client ----

/// Sends one payload on the connection; `Ok(false)` while the connection has no writer yet.
fn send_one(conn: &ClientConn, phase: fish_hooks::NetPhase, channel: &str, data: &[u8]) -> Result<bool, String> {
    use horizon_protocol::packets::{configuration as cfg, play};
    let mut writer = locked(&conn.writer);
    let configuring = conn.configuring.load(std::sync::atomic::Ordering::Relaxed);
    let payload = cfg::CustomPayload {
        channel: channel.to_owned(),
        data: data.to_vec(),
    };
    let body = match (phase, configuring) {
        (fish_hooks::NetPhase::Configuration, true) => cfg::Serverbound::CustomPayload(payload).encode(),
        (fish_hooks::NetPhase::Play, false) => play::Serverbound::CustomPayload(payload).encode(),
        (fish_hooks::NetPhase::Configuration, false) => {
            return Err("the connection is no longer in the configuration phase".to_owned());
        }
        (fish_hooks::NetPhase::Play, true) => {
            return Err("the connection is still in the configuration phase".to_owned());
        }
    };
    let Some(w) = writer.as_mut() else {
        return Ok(false);
    };
    w.send(&body).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Sends what is queued, in order; stops at the first payload the connection cannot take yet.
fn flush_locked(slot: &mut ClientSlot) {
    let Some(conn) = &slot.conn else { return };
    let pending = std::mem::take(&mut slot.pending);
    let mut keep = Vec::new();
    for (phase, channel, data) in pending {
        if !keep.is_empty() {
            keep.push((phase, channel, data));
            continue;
        }
        match send_one(conn, phase, &channel, &data) {
            Ok(true) => {}
            Ok(false) => keep.push((phase, channel, data)),
            Err(e) => log::warn!("Fish: dropped a queued payload on {channel}: {e}"),
        }
    }
    slot.pending = keep;
}

fn client_send(phase: fish_hooks::NetPhase, channel: &str, data: &[u8]) -> Result<(), String> {
    let mut slot = locked(&CLIENT);
    if !slot.live {
        return Err("not connected to a server".to_owned());
    }
    flush_locked(&mut slot);
    if slot.pending.is_empty()
        && let Some(conn) = &slot.conn
    {
        if send_one(conn, phase, channel, data)? {
            return Ok(());
        }
    }
    slot.pending.push((phase, channel.to_owned(), data.to_vec()));
    CLIENT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
    Ok(())
}

/// A session is about to start connecting (`Session::start`): the connection events begin.
pub fn client_init() {
    {
        let mut slot = locked(&CLIENT);
        slot.live = true;
        slot.conn = None;
        slot.pending.clear();
    }
    *locked(&CLIENT_KICK) = None;
    fire(&event(Kind::NetClientInit));
}

/// The session exists: its writer is where payloads go from now on.
pub fn client_register(writer: SharedWriter, configuring: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let mut slot = locked(&CLIENT);
    if !slot.live {
        return;
    }
    slot.conn = Some(ClientConn { writer, configuring });
    flush_locked(&mut slot);
    if !slot.pending.is_empty() {
        CLIENT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// Game thread, every tick: sends what waited for the writer, and carries out a kick.
pub fn client_poll() -> Option<String> {
    if !CLIENT_DIRTY.swap(false, std::sync::atomic::Ordering::AcqRel) {
        return None;
    }
    {
        let mut slot = locked(&CLIENT);
        flush_locked(&mut slot);
        if !slot.pending.is_empty() {
            CLIENT_DIRTY.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    locked(&CLIENT_KICK).take()
}

/// The client entered the play phase.
pub fn client_joined() {
    fire(&event(Kind::NetClientJoin));
}

/// The client's connection ended (once per connection).
pub fn client_left(reason: &str) {
    {
        let mut slot = locked(&CLIENT);
        if !slot.live {
            return;
        }
        slot.live = false;
        slot.conn = None;
        slot.pending.clear();
    }
    let mut e: GameEvent<'_> = event(Kind::NetClientDisconnect);
    e.subject = reason;
    fire(&e);
}

/// A configuration-phase payload reached the client's network thread. True: a receiver took it.
pub fn client_config_payload(channel: &str, data: &[u8]) -> bool {
    let Some(d) = fish_hooks::get() else {
        return false;
    };
    {
        let mut slot = locked(&CLIENT);
        flush_locked(&mut slot);
    }
    let packet = fish_hooks::NetPacket {
        phase: fish_hooks::NetPhase::Configuration,
        channel,
        data,
        from: None,
    };
    d.net_receive(&mut DetachedView { role: Role::Window }, &packet)
}

// ---- server ----

fn take_config(name: &str) -> Vec<ConfigOut> {
    let mut out = locked(&CONFIG_OUT);
    let mut mine = Vec::new();
    let mut i = 0;
    while i < out.len() {
        if out[i].0 == name {
            mine.push(out.remove(i).1);
        } else {
            i += 1;
        }
    }
    mine
}

fn server_event<'a>(kind: Kind, name: &'a str, uuid: u128) -> GameEvent<'a> {
    let mut e = GameEvent::new(kind);
    e.role = server_role();
    e.actor = name;
    e.actor_uuid = uuid.to_be_bytes();
    e
}

/// A player finished logging in and starts configuring (connection thread). Returns what mods
/// queued for it meanwhile.
pub fn server_config_init(name: &str, uuid: u128) -> Vec<ConfigOut> {
    {
        let mut players = locked(&CONFIG_PLAYERS);
        players.retain(|n| n != name);
        players.push(name.to_owned());
    }
    fire(&server_event(Kind::NetServerInit, name, uuid));
    take_config(name)
}

/// A configuration-phase payload reached the server (connection thread). Returns what mods queued
/// for the player meanwhile.
pub fn server_config_payload(name: &str, uuid: u128, channel: &str, data: &[u8]) -> Vec<ConfigOut> {
    if let Some(d) = fish_hooks::get() {
        let packet = fish_hooks::NetPacket {
            phase: fish_hooks::NetPhase::Configuration,
            channel,
            data,
            from: Some((name, uuid.to_be_bytes())),
        };
        d.net_receive(&mut DetachedView { role: server_role() }, &packet);
    }
    take_config(name)
}

/// What mods queued for a configuring player.
pub fn server_config_drain(name: &str) -> Vec<ConfigOut> {
    take_config(name)
}

/// The player's configuration ended: it plays (`joined`) or it was disconnected (`reason`).
pub fn server_config_end(name: &str, uuid: u128, joined: bool, reason: &str) {
    let was_configuring = {
        let mut players = locked(&CONFIG_PLAYERS);
        let before = players.len();
        players.retain(|n| n != name);
        players.len() != before
    };
    take_config(name);
    if !joined && was_configuring {
        let mut e = server_event(Kind::NetServerDisconnect, name, uuid);
        e.subject = reason;
        fire(&e);
    }
}

/// A text component Horizon's disconnect packets take: a plain string.
pub fn reason_tag(text: &str) -> horizon_world::nbt::Tag {
    horizon_world::nbt::Tag::String(horizon_world::nbt::JavaString::from(text))
}

/// The server's play-phase bookkeeping (game thread).
pub fn play_joined(name: &str) {
    let mut players = locked(&PLAY_PLAYERS);
    players.retain(|n| n != name);
    players.push(name.to_owned());
}

pub fn play_left(name: &str) {
    locked(&PLAY_PLAYERS).retain(|n| n != name);
    locked(&PLAY_OUT).retain(|o| match o {
        PlayOut::Payload { player, .. } => !(player.is_some() && matches_player(player, name)),
        PlayOut::Kick { player, .. } => player != name,
    });
}

/// Takes the queued play sends.
pub fn take_play_out() -> Vec<PlayOut> {
    std::mem::take(&mut *locked(&PLAY_OUT))
}

/// Puts play sends back (players not yet placed in the world).
pub fn requeue_play_out(mut held: Vec<PlayOut>) {
    if held.is_empty() {
        return;
    }
    let mut out = locked(&PLAY_OUT);
    held.append(&mut out);
    *out = held;
}
