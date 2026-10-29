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
