// Game events the hook helpers raise: builds `GameEvent`s from the game's state and asks the
// dispatcher. Every function returns quickly when no listener wants the event.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use fish_hooks::{CommandSource, Dispatch, GameEvent, Kind, Outcome, Role};
use horizon_client::first_person::Hand;
use horizon_client::text::Text;

use super::views::{GameView, role_of};
use crate::game::lan::LanHost;
use crate::game::{Dim, Game, GuestUse};

static CLIENT_STOPPING_SENT: AtomicBool = AtomicBool::new(false);

thread_local! {
    // A guest's last seen (dead, asleep), by connection, for the edges the tick hook reports.
    static GUESTS: RefCell<HashMap<u32, (bool, bool)>> = RefCell::new(HashMap::new());
    // Players reported as leaving when the server stopped.
    static LEFT: RefCell<HashSet<u32>> = RefCell::new(HashSet::new());
    // Command roots that shadowed a Fish command, reported once.
    static SHADOWED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    // True while a rewritten chat line is re-entered.
    static REENTERING: RefCell<bool> = const { RefCell::new(false) };
}

// The dispatcher when `kind` (or an event the loader derives from it) has a listener.
fn wanted(kind: Kind) -> Option<&'static dyn Dispatch> {
    fish_hooks::get().filter(|d| {
        d.wants(kind)
            || horizon_client::fish_lib::derived(kind)
                .iter()
                .any(|k| d.wants(*k))
    })
}

fn base<'a>(game: &Game<'_>, kind: Kind) -> GameEvent<'a> {
    let mut e: GameEvent<'a> = GameEvent::new(kind);
    e.role = role_of(game);
    e.is_client = e.role == Role::Window;
    e.dimension = game.dim.id();
    e
}

fn fire(game: &mut Game<'_>, d: &dyn Dispatch, e: &GameEvent<'_>) -> Outcome {
    d.game_event(Some(&mut GameView { game }), e)
}

pub(super) struct Actor {
    pub(super) name: String,
    pub(super) uuid: [u8; 16],
}

pub(super) fn actor(game: &Game<'_>) -> Actor {
    Actor {
        name: game.acting_player_name().to_owned(),
        uuid: game.lan_acting_uuid.map_or([0; 16], u128::to_be_bytes),
    }
}

fn block_pos_of(p: [f64; 3]) -> [i32; 3] {
    p.map(|v| v.floor() as i32)
}

pub(super) fn face_index(face: [i32; 3]) -> u8 {
    match face {
        [0, -1, 0] => 0,
        [0, 1, 0] => 1,
        [0, 0, -1] => 2,
        [0, 0, 1] => 3,
        [-1, 0, 0] => 4,
        [1, 0, 0] => 5,
        _ => 255,
    }
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

// ---- lifecycle ----------------------------------------------------------------------------

/// Called every tick of every game: tells the library hooks what this thread runs.
pub(in crate::game) fn note_context(game: &Game<'_>) {
    horizon_client::fish_lib::note_context(role_of(game), game.dim.id());
}

/// What the actor of this game is, for hooks that have no game at hand.
pub(in crate::game) fn note_actor(game: &Game<'_>) {
    horizon_client::fish_lib::set_actor(
        game.acting_player_name(),
        game.lan_acting_uuid.map_or([0; 16], u128::to_be_bytes),
    );
}

pub(in crate::game) fn client_stopping(game: &mut Game<'_>) {
    if game.options.execution_role != crate::game::execution_role::ExecutionRole::Client {
        return;
    }
    let Some(d) = wanted(Kind::ClientStopping) else {
        return;
    };
    if CLIENT_STOPPING_SENT.swap(true, Ordering::Relaxed) {
        return;
    }
    let e = base(game, Kind::ClientStopping);
    fire(game, d, &e);
}

pub(in crate::game) fn server_starting(game: &mut Game<'_>, world: &str) {
    let Some(d) = wanted(Kind::ServerStarting) else {
        return;
    };
    let mut e = base(game, Kind::ServerStarting);
    e.subject = world;
    fire(game, d, &e);
}

pub(in crate::game) fn server_stopping(game: &mut Game<'_>) {
    horizon_client::fish_lib::mark_stopping();
    super::net::server_stopping(game);
    horizon_client::fish_lib::refresh_crash_details();
    if let Some(d) = wanted(Kind::ServerStopping) {
        let e = base(game, Kind::ServerStopping);
        fire(game, d, &e);
    }
    // The server drops every player as it stops, without a leave of their own.
    if wanted(Kind::PlayerLeave).is_some() {
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
            LEFT.with(|l| l.borrow_mut().insert(conn));
            fire_leave(game, &name, uuid);
        }
    }
}

pub(in crate::game) fn server_stopped(game: &mut Game<'_>) {
    if !horizon_client::fish_lib::take_stopping() {
        return;
    }
    let Some(d) = wanted(Kind::ServerStopped) else {
        return;
    };
    let e = base(game, Kind::ServerStopped);
    fire(game, d, &e);
}

pub(in crate::game) fn level_opened(game: &Game<'_>, dim: Dim) {
    if dim == Dim::Overworld {
        return;
    }
    let Some(d) = wanted(Kind::WorldLoad) else {
        return;
    };
    let mut e = base(game, Kind::WorldLoad);
    e.dimension = dim.id();
    d.game_event(None, &e);
}

pub(in crate::game) fn reload_start(game: &mut Game<'_>, run_load: bool) {
    if !run_load {
        return;
    }
    let Some(d) = wanted(Kind::ReloadStart) else {
        return;
    };
    let e = base(game, Kind::ReloadStart);
    fire(game, d, &e);
}

// ---- interaction --------------------------------------------------------------------------

struct Target {
    pos: Option<[i32; 3]>,
    face: u8,
    entity: Option<(i64, String)>,
}

fn block_target(game: &Game<'_>) -> Option<Target> {
    game.pick().map(|h| Target {
        pos: Some(h.pos),
        face: face_index(h.face),
        entity: None,
    })
}

fn entity_target(game: &Game<'_>) -> Option<Target> {
    let (i, _) = game.mob_hit()?;
    let m = game.mobs.get(i)?;
    Some(Target {
        pos: None,
        face: 255,
        entity: Some((i64::from(m.id), m.kind.id.clone())),
    })
}

pub(super) fn held_item(game: &mut Game<'_>, hand: Hand) -> String {
    let prior = std::mem::replace(&mut game.hand, hand);
    let item = game.held().map(|s| s.item.to_string()).unwrap_or_default();
    game.hand = prior;
    item
}

fn interact(game: &mut Game<'_>, kind: Kind, hand: Hand, target: &Target) -> bool {
    let Some(d) = wanted(kind) else {
        return false;
    };
    let who = actor(game);
    let item = held_item(game, hand);
    let subject = match (&target.entity, target.pos) {
        (Some((_, ty)), _) => ty.clone(),
        (None, Some(p)) => game.block_name(p).to_string(),
        _ => String::new(),
    };
    let mut e = base(game, kind);
    e.hand = u8::from(hand == Hand::Off);
    e.face = target.face;
    e.pos = target.pos.unwrap_or([0; 3]);
    e.entity_id = target.entity.as_ref().map_or(0, |t| t.0);
    e.actor = &who.name;
    e.actor_uuid = who.uuid;
    e.subject = &subject;
    e.item = &item;
    fire(game, d, &e).cancelled()
}

/// `use_item`: the funnel for a guest's use packets and the in-frame player's click. True cancels.
pub(in crate::game) fn use_item(game: &mut Game<'_>) -> bool {
    let (kind, hand) = match game.guest_use {
        Some(GuestUse::Entity(h)) => (Kind::UseEntity, h),
        Some(GuestUse::OnBlock(h)) => (Kind::UseBlock, h),
        Some(GuestUse::Item(h)) => (Kind::UseItem, h),
        None => {
            if game.entity_targeted() {
                (Kind::UseEntity, Hand::Main)
            } else if game.pick().is_some() {
                (Kind::UseBlock, Hand::Main)
            } else {
                (Kind::UseItem, Hand::Main)
            }
        }
    };
    if wanted(kind).is_none() {
        return false;
    }
    let target = match kind {
        Kind::UseBlock => block_target(game),
        Kind::UseEntity => entity_target(game),
        _ => None,
    }
    .unwrap_or(Target {
        pos: None,
        face: 255,
        entity: None,
    });
    interact(game, kind, hand, &target)
}

pub(in crate::game) fn attack_block(game: &mut Game<'_>, pos: [i32; 3]) -> bool {
    if wanted(Kind::AttackBlock).is_none() {
        return false;
    }
    let face = game
        .pick()
        .filter(|h| h.pos == pos)
        .map_or(255, |h| face_index(h.face));
    let target = Target {
        pos: Some(pos),
        face,
        entity: None,
    };
    interact(game, Kind::AttackBlock, Hand::Main, &target)
}

pub(in crate::game) fn attack(game: &mut Game<'_>) -> bool {
    if wanted(Kind::AttackEntity).is_none() {
        return false;
    }
    match entity_target(game) {
        Some(t) => interact(game, Kind::AttackEntity, Hand::Main, &t),
        None => false,
    }
}

pub(in crate::game) fn block_break_before(game: &mut Game<'_>, pos: [i32; 3]) -> bool {
    let target = Target {
        pos: Some(pos),
        face: 255,
        entity: None,
    };
    interact(game, Kind::BlockBreakBefore, Hand::Main, &target)
}

/// A client prediction of a block use (`remote_use_item_on`).
pub(in crate::game) fn remote_use_block(game: &mut Game<'_>, hand: Hand) -> bool {
    if wanted(Kind::UseBlock).is_none() {
        return false;
    }
    match block_target(game) {
        Some(t) => interact(game, Kind::UseBlock, hand, &t),
        None => false,
    }
}

pub(in crate::game) fn remote_use_item(game: &mut Game<'_>, hand: Hand) -> bool {
    let target = Target {
        pos: None,
        face: 255,
        entity: None,
    };
    interact(game, Kind::UseItem, hand, &target)
}

pub(in crate::game) fn remote_use_entity(game: &mut Game<'_>, hand: Hand) -> bool {
    if wanted(Kind::UseEntity).is_none() {
        return false;
    }
    match entity_target(game) {
        Some(t) => interact(game, Kind::UseEntity, hand, &t),
        None => false,
    }
}

pub(in crate::game) fn remote_attack_entity(game: &mut Game<'_>) -> bool {
    attack(game)
}

/// The client starts mining the block it aims at.
pub(in crate::game) fn remote_attack_block(game: &mut Game<'_>, start: bool) -> bool {
    if !start || wanted(Kind::AttackBlock).is_none() {
        return false;
    }
    match block_target(game) {
        Some(t) => interact(game, Kind::AttackBlock, Hand::Main, &t),
        None => false,
    }
}

// ---- entity -------------------------------------------------------------------------------

pub(in crate::game) fn mob_died(game: &mut Game<'_>, id: u32, entity_type: &str, at: [f64; 3]) {
    let Some(d) = wanted(Kind::AfterDeath) else {
        return;
    };
    let mut e = base(game, Kind::AfterDeath);
    e.entity_id = i64::from(id);
    e.subject = entity_type;
    e.pos = block_pos_of(at);
    fire(game, d, &e);
}

/// What a guest was doing on the previous tick, to report what changed (death, respawn, sleep).
pub(in crate::game) fn guest_edges(game: &mut Game<'_>, host: &LanHost) {
    const KINDS: [Kind; 5] = [
        Kind::AfterDeath,
        Kind::AfterRespawn,
        Kind::CopyFrom,
        Kind::SleepStart,
        Kind::SleepStop,
    ];
    if KINDS.iter().all(|k| wanted(*k).is_none()) {
        return;
    }
    struct Edge {
        kinds: &'static [Kind],
        name: String,
        uuid: [u8; 16],
        pos: [i32; 3],
        id: i64,
        dimension: String,
    }
    let mut edges: Vec<Edge> = Vec::new();
    GUESTS.with(|state| {
        let mut state = state.borrow_mut();
        state.retain(|conn, _| host.hub.guests.iter().any(|g| g.conn == *conn));
        for g in &host.hub.guests {
            if !g.placed {
                continue;
            }
            let Some(prev) = state.insert(g.conn, (g.dead, g.sleeping)) else {
                continue;
            };
            let kinds: &'static [Kind] = if !prev.0 && g.dead {
                &[Kind::AfterDeath]
            } else if prev.0 && !g.dead {
                &[Kind::AfterRespawn, Kind::CopyFrom]
            } else if !prev.1 && g.sleeping {
                &[Kind::SleepStart]
            } else if prev.1 && !g.sleeping {
                &[Kind::SleepStop]
            } else {
                continue;
            };
            edges.push(Edge {
                kinds,
                name: g.name.clone(),
                uuid: g.uuid.to_be_bytes(),
                pos: g.bed.unwrap_or_else(|| block_pos_of(g.position)),
                id: i64::from(g.entity_id),
                dimension: g.dimension.clone(),
            });
        }
    });
    for edge in &edges {
        for &kind in edge.kinds {
            let Some(d) = wanted(kind) else { continue };
            let mut e = base(game, kind);
            e.actor = &edge.name;
            e.actor_uuid = edge.uuid;
            e.dimension = &edge.dimension;
            e.pos = edge.pos;
            if kind == Kind::AfterDeath {
                e.entity_id = edge.id;
                e.subject = "minecraft:player";
            }
            fire(game, d, &e);
        }
    }
}

/// The window's own player respawned (the death screen's button, without a server).
pub(in crate::game) fn local_respawn(game: &mut Game<'_>) {
    if game.remote.is_some() {
        return;
    }
    let who = actor(game);
    for kind in [Kind::AfterRespawn, Kind::CopyFrom] {
        let Some(d) = wanted(kind) else { continue };
        let mut e = base(game, kind);
        e.actor = &who.name;
        e.actor_uuid = who.uuid;
        fire(game, d, &e);
    }
}

pub(in crate::game) fn player_join(game: &mut Game<'_>, host: &LanHost, conn: u32) {
    let Some(d) = wanted(Kind::PlayerJoin) else {
        return;
    };
    let Some(g) = host.hub.guest(conn) else {
        return;
    };
    let (name, uuid) = (g.name.clone(), g.uuid.to_be_bytes());
    let mut e = base(game, Kind::PlayerJoin);
    e.actor = &name;
    e.actor_uuid = uuid;
    fire(game, d, &e);
}

pub(in crate::game) fn player_leave(game: &mut Game<'_>, conn: u32, name: &str, uuid: u128) {
    // Already reported when the server stopped.
    if LEFT.with(|l| l.borrow_mut().remove(&conn)) {
        return;
    }
    fire_leave(game, name, uuid);
}

fn fire_leave(game: &mut Game<'_>, name: &str, uuid: u128) {
    let Some(d) = wanted(Kind::PlayerLeave) else {
        return;
    };
    let mut e = base(game, Kind::PlayerLeave);
    e.actor = name;
    e.actor_uuid = uuid.to_be_bytes();
    fire(game, d, &e);
}

pub(in crate::game) fn allow_sleeping(game: &mut Game<'_>, pos: [i32; 3]) -> bool {
    let Some(d) = wanted(Kind::AllowSleeping) else {
        return false;
    };
    let who = actor(game);
    let mut e = base(game, Kind::AllowSleeping);
    e.pos = pos;
    e.actor = &who.name;
    e.actor_uuid = who.uuid;
    fire(game, d, &e).cancelled()
}

thread_local! {
    // Whether this thread's own player was asleep on the previous tick.
    static LOCAL_SLEEPING: RefCell<bool> = const { RefCell::new(false) };
}

/// The window's (or in-frame) own player fell asleep or woke up; seen at the start of a tick.
pub(in crate::game) fn local_sleep_edge(game: &mut Game<'_>) {
    let now = game.sleeping;
    let was = LOCAL_SLEEPING.with(|s| s.replace(now.is_some()));
    if was || now.is_none() {
        return;
    }
    let Some(d) = wanted(Kind::SleepStart) else {
        return;
    };
    let who = actor(game);
    let mut e = base(game, Kind::SleepStart);
    e.pos = now.unwrap_or([0; 3]);
    e.actor = &who.name;
    e.actor_uuid = who.uuid;
    fire(game, d, &e);
}

/// The window's own player wakes up (`wake_up` before it clears the state).
pub(in crate::game) fn local_wake(game: &mut Game<'_>) {
    if game.remote.is_some() || game.sleeping.is_none() {
        return;
    }
    LOCAL_SLEEPING.with(|s| s.replace(false));
    let Some(d) = wanted(Kind::SleepStop) else {
        return;
    };
    let who = actor(game);
    let mut e = base(game, Kind::SleepStop);
    e.actor = &who.name;
    e.actor_uuid = who.uuid;
    fire(game, d, &e);
}

// ---- messages -----------------------------------------------------------------------------

fn source_label(source: horizon_client::chat::ChatSource) -> String {
    snake(&format!("{source:?}"))
}

/// A chat line is about to reach the player's chat. True drops it.
pub(in crate::game) fn receive(game: &mut Game<'_>, text: &Text, source: horizon_client::chat::ChatSource) -> bool {
    if wanted(Kind::AllowReceive).is_none() && wanted(Kind::Receive).is_none() {
        return false;
    }
    let plain = text.plain();
    let label = source_label(source);
    if let Some(d) = wanted(Kind::AllowReceive) {
        let mut e = base(game, Kind::AllowReceive);
        e.subject = &plain;
        e.extra = &label;
        if fire(game, d, &e).cancelled() {
            return true;
        }
    }
    if let Some(d) = wanted(Kind::Receive) {
        let mut e = base(game, Kind::Receive);
        e.subject = &plain;
        e.extra = &label;
        fire(game, d, &e);
    }
    false
}

/// What the player's typed chat line becomes.
pub(in crate::game) enum Typed {
    Send,
    Cancel,
    Replace(String),
}

/// A typed chat line (not a command): listeners may cancel it or rewrite it. `in_input` is true
/// for `handle_chat_input`, which on a server leaves the line to `send_chat` (where it is reported).
pub(in crate::game) fn typed(game: &mut Game<'_>, message: &str, in_input: bool) -> Typed {
    if message.starts_with('/')
        || REENTERING.with(|r| *r.borrow())
        || (in_input && game.remote.is_some())
    {
        return Typed::Send;
    }
    let who = actor(game);
    let mut text: Option<String> = None;
    if let Some(d) = wanted(Kind::AllowSend) {
        let mut e = base(game, Kind::AllowSend);
        e.subject = message;
        e.rewritable = true;
        e.actor = &who.name;
        match fire(game, d, &e) {
            Outcome::Cancel => return Typed::Cancel,
            Outcome::Replace(new) if new != message => text = Some(new),
            _ => {}
        }
    }
    if let Some(d) = wanted(Kind::Send) {
        let mut e = base(game, Kind::Send);
        e.subject = text.as_deref().unwrap_or(message);
        e.actor = &who.name;
        fire(game, d, &e);
    }
    match text {
        Some(new) => Typed::Replace(new),
        None => Typed::Send,
    }
}

/// Keeps `typed` from firing again while a rewritten line is re-entered.
pub(in crate::game) struct Reentry;

impl Reentry {
    pub(in crate::game) fn enter() -> Reentry {
        REENTERING.with(|r| *r.borrow_mut() = true);
        Reentry
    }
}

impl Drop for Reentry {
    fn drop(&mut self) {
        REENTERING.with(|r| *r.borrow_mut() = false);
    }
}


/// A guest's command is about to run on the server.
pub(in crate::game) fn command_ran(game: &mut Game<'_>, host: &LanHost, conn: u32, command: &str) {
    let Some(d) = wanted(Kind::Command) else {
        return;
    };
    let Some(g) = host.hub.guest(conn) else {
        return;
    };
    let (name, uuid) = (g.name.clone(), g.uuid.to_be_bytes());
    let mut e = base(game, Kind::Command);
    e.actor = &name;
    e.actor_uuid = uuid;
    e.subject = command;
    fire(game, d, &e);
}

// ---- commands -----------------------------------------------------------------------------

/// Runs `command` (no slash) when it is a Fish command. `Some(ok)` when Fish ran it.
pub(in crate::game) fn run_command(game: &mut Game<'_>, command: &str) -> Option<bool> {
    let d = fish_hooks::get()?;
    let root = command.split_whitespace().next()?;
    if horizon_client::command::vanilla::game_dispatcher()
        .find(root)
        .is_some()
    {
        if d.command_roots().iter().any(|r| r == root)
            && SHADOWED.with(|s| s.borrow_mut().insert(root.to_owned()))
        {
            d.shadowed_command(root);
        }
        return None;
    }
    let who = actor(game);
    let source = CommandSource {
        name: &who.name,
        uuid: who.uuid,
        level: game.ui.registries.permission,
        is_player: game.lan_acting.is_some() || game.options.dedicated.is_none(),
        pos: game
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z]),
    };
    let outcome = d.run_command(&mut GameView { game }, &source, command);
    if !outcome.handled {
        return None;
    }
    for line in &outcome.messages {
        if outcome.ok {
            game.feedback(Text::literal(line.as_str()));
        } else {
            game.command_failure(Text::literal(line.as_str()));
        }
    }
    Some(outcome.ok)
}
