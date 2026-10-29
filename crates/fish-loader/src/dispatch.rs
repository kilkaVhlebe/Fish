//! The [`Dispatch`] implementation: Horizon's hooks become Fish events.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use fish_abi::client::{CommandPayload, FramePayload, HudPayload, KeyPayload, TickPayload, ids};
use fish_abi::commands::{KIND_CLIENT, KIND_SERVER};
use fish_abi::debug::DebugPayload;
use fish_abi::game::{EVENTS, GamePayload};
use fish_abi::{ActionResult, COMBINE_FIRST_NON_PASS, COMBINE_NOTIFY, FishHandle, FishStr};
use fish_hooks::{
    ClientView, CommandOutcome, CommandSource, Dispatch, GameEvent, HudView, Kind, NativeModRow,
    Outcome, Role,
};

use crate::boot::Loader;
use crate::bus::EventIndex;
use crate::client_services::role_byte;
use crate::inner::ModState;

/// Routes Horizon's hook points to the events of a [`Loader`].
pub struct LoaderDispatch {
    loader: Loader,
    client_start: EventIndex,
    client_end: EventIndex,
    server_start: EventIndex,
    server_end: EventIndex,
    frame: EventIndex,
    hud: EventIndex,
    command: EventIndex,
    key: EventIndex,
    game: Vec<EventIndex>,
    f3: EventIndex,
    ticks: [AtomicU64; 3],
    /// `client_started` was fired.
    client_started: AtomicBool,
    /// `/reload` started and its end is due at the next server tick.
    reload_pending: AtomicBool,
    /// Levels loaded per role, for `world_unload` at server stop.
    levels: Mutex<Vec<(Role, String)>>,
    /// Breaks that were not cancelled; their `block_break_after` fires at the next server tick.
    breaks: Mutex<Vec<Deferred>>,
    /// When on, every game event fired is recorded as `group/name outcome` (tests).
    trace: Mutex<Option<Vec<String>>>,
}

/// A `block_break_after` waiting for the next server tick.
struct Deferred {
    role: Role,
    pos: [i32; 3],
    subject: String,
    actor: String,
    actor_uuid: [u8; 16],
    dimension: String,
}

/// The events every loader defines for the client hooks, with their combine modes.
pub(crate) const HOST_EVENTS: [(&str, u8); 11] = [
    (ids::CLIENT_START_TICK, COMBINE_NOTIFY),
    (ids::CLIENT_END_TICK, COMBINE_NOTIFY),
    (ids::SERVER_START_TICK, COMBINE_NOTIFY),
    (ids::SERVER_END_TICK, COMBINE_NOTIFY),
    (ids::FRAME, COMBINE_NOTIFY),
    (ids::HUD_RENDER, COMBINE_NOTIFY),
    (ids::RAW_COMMAND, COMBINE_FIRST_NON_PASS),
    (ids::KEY, COMBINE_NOTIFY),
    (fish_abi::commands::ids::REGISTER, COMBINE_NOTIFY),
    (fish_abi::commands::ids::REGISTER_CLIENT, COMBINE_NOTIFY),
    (fish_abi::debug::F3_LINES, COMBINE_NOTIFY),
];

/// A shorter-lived copy of an optional game borrow.
fn reborrow<'b>(game: &'b mut Option<&mut dyn ClientView>) -> Option<&'b mut dyn ClientView> {
    match game {
        Some(g) => Some(&mut **g),
        None => None,
    }
}

fn quiet<R>(default: R, f: impl FnOnce() -> R) -> R {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(default)
}

impl LoaderDispatch {
    /// Builds the dispatcher for `loader`.
    pub fn new(loader: Loader) -> LoaderDispatch {
        let idx = |id: &str| {
            loader
                .bus()
                .lookup(id)
                .expect("host events are defined at boot")
        };
        LoaderDispatch {
            client_start: idx(ids::CLIENT_START_TICK),
            client_end: idx(ids::CLIENT_END_TICK),
            server_start: idx(ids::SERVER_START_TICK),
            server_end: idx(ids::SERVER_END_TICK),
            frame: idx(ids::FRAME),
            hud: idx(ids::HUD_RENDER),
            command: idx(ids::RAW_COMMAND),
            key: idx(ids::KEY),
            f3: idx(fish_abi::debug::F3_LINES),
            game: EVENTS.iter().map(|(id, _)| idx(id)).collect(),
            loader,
            ticks: Default::default(),
            client_started: AtomicBool::new(false),
            reload_pending: AtomicBool::new(false),
            levels: Mutex::default(),
            breaks: Mutex::default(),
            trace: Mutex::new(None),
        }
    }

    /// Starts recording every game event fired (see [`take_trace`](Self::take_trace)).
    pub fn enable_trace(&self) {
        let mut trace = self.trace.lock().unwrap_or_else(|e| e.into_inner());
        trace.get_or_insert_with(Vec::new);
    }

    /// The events fired since the last call, as `group/name outcome` (`outcome` is `pass`,
    /// `handled`, `cancel` or `replace:<text>`); empty unless [`enable_trace`](Self::enable_trace) ran.
    pub fn take_trace(&self) -> Vec<String> {
        let mut trace = self.trace.lock().unwrap_or_else(|e| e.into_inner());
        trace.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// The loader behind this dispatcher.
    pub fn loader(&self) -> &Loader {
        &self.loader
    }

    fn tick(&self, game: &mut dyn ClientView, start: bool) {
        let role = game.role();
        let slot = &self.ticks[usize::from(role_byte(role))];
        let tick = if start {
            slot.fetch_add(1, Ordering::SeqCst) + 1
        } else {
            slot.load(Ordering::SeqCst)
        };
        if start && role.is_server() && tick == 1 {
            let level = game.level_id();
            let mut e = GameEvent::new(Kind::WorldLoad);
            e.dimension = &level;
            self.game_event_flow(Some(&mut *game), &e);
            self.game_event_flow(Some(&mut *game), &GameEvent::new(Kind::ServerStarted));
        }
        if !start && role.is_server() {
            self.after_server_tick(game);
        }
        if !start && role == Role::Window {
            let notices = std::mem::take(
                &mut *self
                    .loader
                    .inner
                    .notices
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()),
            );
            for notice in notices {
                game.show_chat(&notice);
            }
        }
        let event = match (role.is_server(), start) {
            (false, true) => self.client_start,
            (false, false) => self.client_end,
            (true, true) => self.server_start,
            (true, false) => self.server_end,
        };
        let inner = &self.loader.inner;
        inner.handles.client_scope(game, |view| {
            let mut p = TickPayload {
                size: size_of::<TickPayload>() as u32,
                role: role_byte(role),
                reserved: [0; 3],
                tick,
                view,
            };
            // SAFETY: the payload matches the event's documented layout.
            unsafe {
                inner
                    .bus
                    .invoke(event, (&raw mut p).cast(), std::ptr::null_mut())
            };
        });
    }
}

unsafe extern "C" fn collect_replacement(user: *mut c_void, text: FishStr) {
    // SAFETY: `user` is the `Option<String>` of `game_event`; the text is valid UTF-8 or rejected.
    unsafe {
        if let Ok(s) = text.as_str() {
            *user.cast::<Option<String>>() = Some(s.to_string());
        }
    }
}

impl LoaderDispatch {
    fn fire_game(&self, game: Option<&mut dyn ClientView>, e: &GameEvent<'_>) -> Outcome {
        let out = self.fire_game_untraced(game, e);
        if let Some(trace) = self
            .trace
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            let result = match &out {
                Outcome::Pass => "pass".to_string(),
                Outcome::Handled => "handled".to_string(),
                Outcome::Cancel => "cancel".to_string(),
                Outcome::Replace(t) => format!("replace:{t}"),
            };
            trace.push(format!("{} {result}", e.kind.name()));
        }
        out
    }

    fn fire_game_untraced(&self, game: Option<&mut dyn ClientView>, e: &GameEvent<'_>) -> Outcome {
        let index = self.game[e.kind as usize];
        let inner = &self.loader.inner;
        if !inner.bus.has_listeners(index) {
            return Outcome::Pass;
        }
        let role = game.as_ref().map_or(e.role, |g| g.role());
        let fire = |view| {
            let mut p = GamePayload::new(role_byte(role), view);
            p.is_client = u8::from(e.is_client);
            p.hand = e.hand;
            p.face = e.face;
            p.pos = e.pos;
            p.entity_id = e.entity_id;
            p.amount = e.amount;
            p.actor_uuid = e.actor_uuid;
            p.actor = FishStr::new(e.actor);
            p.dimension = FishStr::new(e.dimension);
            p.subject = FishStr::new(e.subject);
            p.item = FishStr::new(e.item);
            p.extra = FishStr::new(e.extra);
            let mut replaced: Option<String> = None;
            if e.rewritable {
                p.replace = Some(collect_replacement);
                p.replace_user = (&raw mut replaced).cast();
            }
            let mut result = ActionResult::Pass.as_u8();
            let out: *mut c_void = if EVENTS[e.kind as usize].1 == COMBINE_FIRST_NON_PASS {
                (&raw mut result).cast()
            } else {
                std::ptr::null_mut()
            };
            // SAFETY: the payload and result match the event's documented layout.
            unsafe { inner.bus.invoke(index, (&raw mut p).cast(), out) };
            match ActionResult::from_u8(result) {
                Some(ActionResult::Fail) => Outcome::Cancel,
                Some(ActionResult::Pass) | None => replaced.map_or(Outcome::Pass, Outcome::Replace),
                Some(_) => Outcome::Handled,
            }
        };
        match game {
            Some(g) => inner.handles.client_scope(g, fire),
            None => fire(FishHandle {
                index: u32::MAX,
                generation: 0,
            }),
        }
    }
}

unsafe extern "C" fn add_debug_line(user: *mut c_void, line: FishStr) {
    // SAFETY: `user` is the `Vec<String>` of `debug_lines`; the text is valid UTF-8 or skipped.
    unsafe {
        let lines = &mut *user.cast::<Vec<String>>();
        if let Ok(s) = line.as_str()
            && lines.len() < MAX_DEBUG_LINES
        {
            lines.push(s.chars().take(MAX_DEBUG_LINE_CHARS).collect());
        }
    }
}

/// Most lines mods may add to the F3 screen.
const MAX_DEBUG_LINES: usize = 32;
/// Longest F3 line kept.
const MAX_DEBUG_LINE_CHARS: usize = 200;

impl LoaderDispatch {
    /// Runs `line` against the command trees of `kind`.
    fn run_tree(&self, kind: u8, source: &CommandSource<'_>, line: &str) -> CommandOutcome {
        let inner = &self.loader.inner;
        let parsed = {
            let commands = inner.commands.lock().unwrap_or_else(|e| e.into_inner());
            if !commands.owns(kind, line) {
                return CommandOutcome::default();
            }
            commands.parse(kind, source, line)
        };
        match parsed {
            Err(message) => CommandOutcome {
                handled: true,
                ok: false,
                result: 0,
                messages: vec![message],
            },
            Ok(parsed) => {
                let owner = parsed.executor.owner;
                if inner.record(owner).is_some_and(|r| r.is_failed()) {
                    return CommandOutcome::default();
                }
                let ran = parsed.run(source);
                if let Some(why) = ran.broken {
                    inner.fail_mod(owner, why, true);
                }
                ran.outcome
            }
        }
    }

    /// Fires `event` and the events that follow from its outcome (see `Dispatch::game_event`).
    fn game_event_flow(
        &self,
        mut game: Option<&mut dyn ClientView>,
        event: &GameEvent<'_>,
    ) -> Outcome {
        let role = game.as_ref().map_or(event.role, |g| g.role());
        let out = self.fire_game(reborrow(&mut game), event);
        match event.kind {
            Kind::WorldLoad => {
                let mut levels = self.levels.lock().unwrap_or_else(|e| e.into_inner());
                if !levels
                    .iter()
                    .any(|(r, d)| *r == role && d == event.dimension)
                {
                    levels.push((role, event.dimension.to_string()));
                }
            }
            Kind::ServerStopping => {
                let gone: Vec<String> = {
                    let mut levels = self.levels.lock().unwrap_or_else(|e| e.into_inner());
                    let gone = levels
                        .iter()
                        .filter(|(r, _)| *r == role)
                        .map(|(_, d)| d.clone())
                        .collect();
                    levels.retain(|(r, _)| *r != role);
                    gone
                };
                for dim in gone {
                    let mut e = GameEvent::new(Kind::WorldUnload);
                    e.dimension = &dim;
                    self.fire_game(reborrow(&mut game), &e);
                }
            }
            Kind::ReloadStart => self.reload_pending.store(true, Ordering::SeqCst),
            Kind::BlockBreakBefore if out.cancelled() => {
                let mut e = event.clone();
                e.kind = Kind::BlockBreakCanceled;
                self.fire_game(reborrow(&mut game), &e);
            }
            Kind::BlockBreakBefore => {
                self.breaks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(Deferred {
                        role,
                        pos: event.pos,
                        subject: event.subject.to_string(),
                        actor: event.actor.to_string(),
                        actor_uuid: event.actor_uuid,
                        dimension: event.dimension.to_string(),
                    });
            }
            Kind::AllowDamage if !out.cancelled() => {
                let mut e = event.clone();
                e.kind = Kind::AfterDamage;
                self.fire_game(reborrow(&mut game), &e);
            }
            Kind::AllowSend if !out.cancelled() => {
                let mut e = event.clone();
                e.kind = Kind::ModifySend;
                e.rewritable = true;
                return self.fire_game(reborrow(&mut game), &e);
            }
            _ => {}
        }
        out
    }

    /// What is due at the end of a server tick: `reload_end` and `block_break_after`.
    fn after_server_tick(&self, game: &mut dyn ClientView) {
        let role = game.role();
        if self.reload_pending.swap(false, Ordering::SeqCst) {
            self.fire_game(Some(&mut *game), &GameEvent::new(Kind::ReloadEnd));
        }
        let due: Vec<Deferred> = {
            let mut breaks = self.breaks.lock().unwrap_or_else(|e| e.into_inner());
            let (due, rest) = std::mem::take(&mut *breaks)
                .into_iter()
                .partition(|d| d.role == role);
            *breaks = rest;
            due
        };
        for d in due {
            let mut e = GameEvent::new(Kind::BlockBreakAfter);
            e.pos = d.pos;
            e.subject = &d.subject;
            e.actor = &d.actor;
            e.actor_uuid = d.actor_uuid;
            e.dimension = &d.dimension;
            self.fire_game(Some(&mut *game), &e);
        }
    }
}

impl Dispatch for LoaderDispatch {
    fn game_event(&self, game: Option<&mut dyn ClientView>, event: &GameEvent<'_>) -> Outcome {
        quiet(Outcome::Pass, || self.game_event_flow(game, event))
    }

    fn run_command(
        &self,
        _game: &mut dyn ClientView,
        source: &CommandSource<'_>,
        line: &str,
    ) -> CommandOutcome {
        quiet(CommandOutcome::default(), || {
            self.run_tree(KIND_SERVER, source, line)
        })
    }

    fn suggest_command(&self, source: &CommandSource<'_>, line: &str) -> Vec<String> {
        quiet(Vec::new(), || {
            let commands = self
                .loader
                .inner
                .commands
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let mut out = commands.suggestions(KIND_SERVER, source, line);
            out.extend(commands.suggestions(KIND_CLIENT, source, line));
            out
        })
    }

    fn command_roots(&self) -> Vec<String> {
        let commands = self
            .loader
            .inner
            .commands
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut roots = commands.roots(KIND_SERVER);
        roots.extend(commands.roots(KIND_CLIENT));
        roots
    }

    fn shadowed_command(&self, root: &str) {
        self.loader.inner.warn(format!(
            "the command /{root} of a Fish mod is shadowed by the game's own command of that name and will not run"
        ));
    }

    fn crash_details(&self) -> Vec<(&'static str, String)> {
        quiet(Vec::new(), || {
            let inner = &self.loader.inner;
            let mut running = Vec::new();
            let mut failed = Vec::new();
            for rec in inner.records() {
                match rec.state() {
                    ModState::Failed(reason) => {
                        failed.push(format!(
                            "{} {} ({reason})",
                            rec.manifest.id, rec.version_text
                        ));
                    }
                    ModState::Loading if rec.manifest.library.is_none() => {}
                    _ => running.push(format!("{} {}", rec.manifest.id, rec.version_text)),
                }
            }
            let list = |v: Vec<String>| {
                if v.is_empty() {
                    "none".to_string()
                } else {
                    v.join(", ")
                }
            };
            vec![
                (
                    "Fish",
                    format!(
                        "{} (ABI {}.{}) on Horizon {}",
                        inner.fish_version,
                        fish_abi::FISH_ABI_MAJOR,
                        fish_abi::FISH_ABI_MINOR,
                        inner.horizon_version
                    ),
                ),
                ("Fish mods", list(running)),
                ("Fish failed mods", list(failed)),
            ]
        })
    }

    fn debug_lines(&self, out: &mut Vec<String>) {
        quiet((), || {
            let inner = &self.loader.inner;
            let mods = inner.records().iter().filter(|r| !r.is_failed()).count();
            out.push(format!("[Fish] {} - {mods} mod(s)", inner.fish_version));
            if !inner.bus.has_listeners(self.f3) {
                return;
            }
            let mut lines: Vec<String> = Vec::new();
            let mut p = DebugPayload {
                size: size_of::<DebugPayload>() as u32,
                add: add_debug_line,
                user: (&raw mut lines).cast(),
            };
            // SAFETY: the payload matches the event's documented layout.
            unsafe {
                inner
                    .bus
                    .invoke(self.f3, (&raw mut p).cast(), std::ptr::null_mut())
            };
            out.extend(lines);
        });
    }

    fn wants(&self, kind: Kind) -> bool {
        self.loader
            .inner
            .bus
            .has_listeners(self.game[kind as usize])
    }

    fn tick_start(&self, game: &mut dyn ClientView) {
        quiet((), || self.tick(game, true));
    }

    fn tick_end(&self, game: &mut dyn ClientView) {
        quiet((), || self.tick(game, false));
    }

    fn frame(&self, game: &mut dyn ClientView, elapsed_secs: f32) {
        quiet((), || {
            if !self.client_started.swap(true, Ordering::SeqCst) {
                self.fire_game(Some(&mut *game), &GameEvent::new(Kind::ClientStarted));
            }
            let inner = &self.loader.inner;
            inner.handles.client_scope(game, |view| {
                let mut p = FramePayload {
                    size: size_of::<FramePayload>() as u32,
                    elapsed_secs,
                    view,
                };
                // SAFETY: the payload matches the event's documented layout.
                unsafe {
                    inner
                        .bus
                        .invoke(self.frame, (&raw mut p).cast(), std::ptr::null_mut())
                };
            });
        });
    }

    fn hud(&self, game: &mut dyn ClientView, hud: &mut dyn HudView) {
        quiet((), || {
            let inner = &self.loader.inner;
            inner.handles.client_scope(game, |view| {
                inner.handles.hud_scope(hud, |hud| {
                    let mut p = HudPayload {
                        size: size_of::<HudPayload>() as u32,
                        view,
                        hud,
                    };
                    // SAFETY: the payload matches the event's documented layout.
                    unsafe {
                        inner
                            .bus
                            .invoke(self.hud, (&raw mut p).cast(), std::ptr::null_mut())
                    };
                });
            });
        });
    }

    fn client_command(&self, game: &mut dyn ClientView, command: &str) -> bool {
        quiet(false, || {
            let player = game.player();
            let source = CommandSource {
                name: "",
                uuid: [0; 16],
                level: 4,
                is_player: true,
                pos: player.map(|p| [p.x, p.y, p.z]),
            };
            let outcome = self.run_tree(KIND_CLIENT, &source, command);
            if outcome.handled {
                for message in &outcome.messages {
                    game.show_chat(message);
                }
                return true;
            }
            let inner = &self.loader.inner;
            inner.handles.client_scope(game, |view| {
                let mut p = CommandPayload {
                    size: size_of::<CommandPayload>() as u32,
                    view,
                    command: FishStr::new(command),
                };
                let mut result = ActionResult::Pass.as_u8();
                // SAFETY: the payload and result match the event's documented layout.
                unsafe {
                    inner.bus.invoke(
                        self.command,
                        (&raw mut p).cast(),
                        (&raw mut result).cast::<c_void>(),
                    );
                }
                ActionResult::from_u8(result) == Some(ActionResult::Success)
            })
        })
    }

    fn key(&self, game: &mut dyn ClientView, key: &str, pressed: bool) {
        quiet((), || {
            let inner = &self.loader.inner;
            {
                let mut down = inner.keys_down.lock().unwrap_or_else(|e| e.into_inner());
                if pressed {
                    down.insert(key.to_string());
                } else {
                    down.remove(key);
                }
            }
            inner.handles.client_scope(game, |view| {
                let mut p = KeyPayload {
                    size: size_of::<KeyPayload>() as u32,
                    pressed: u8::from(pressed),
                    reserved: [0; 3],
                    view,
                    key: FishStr::new(key),
                };
                // SAFETY: the payload matches the event's documented layout.
                unsafe {
                    inner
                        .bus
                        .invoke(self.key, (&raw mut p).cast(), std::ptr::null_mut())
                };
            });
        });
    }

    fn list_mods(&self, rows: &mut Vec<NativeModRow>) {
        quiet((), || {
            for rec in self.loader.inner.records() {
                let (suffix, description) = match rec.state() {
                    ModState::Failed(reason) => (" (stopped)", format!("Stopped: {reason}")),
                    ModState::Loading if rec.manifest.library.is_none() => continue,
                    _ => ("", rec.manifest.description.clone()),
                };
                rows.push(NativeModRow {
                    id: rec.manifest.id.to_string(),
                    title: format!("{} {} [Fish]{suffix}", rec.manifest.name, rec.version_text),
                    description,
                    keys: Vec::new(),
                });
            }
        });
    }
}

static GLOBAL: OnceLock<LoaderDispatch> = OnceLock::new();

/// Installs `loader` as the process-wide Fish and registers its dispatcher with `fish-hooks`.
///
/// Only the edition bridge calls this; tests create independent loaders instead. If a loader is
/// already installed, it is kept and returned.
pub fn install_global(loader: Loader) -> &'static Loader {
    let dispatch = GLOBAL.get_or_init(|| LoaderDispatch::new(loader));
    fish_hooks::install(dispatch);
    dispatch.loader()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hooks_kinds_and_the_abi_table_agree() {
        assert_eq!(Kind::ALL.len(), EVENTS.len());
        for (kind, (id, _)) in Kind::ALL.iter().zip(EVENTS) {
            assert_eq!(id, format!("fish:{}", kind.name()), "{kind:?}");
            assert_eq!(
                Kind::ALL[*kind as usize],
                *kind,
                "{kind:?} is its own table index"
            );
        }
    }
}
