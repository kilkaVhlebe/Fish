//! The [`Dispatch`] implementation: Horizon's hooks become Fish events.

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use fish_abi::client::{CommandPayload, FramePayload, HudPayload, KeyPayload, TickPayload, ids};
use fish_abi::{ActionResult, COMBINE_FIRST_NON_PASS, COMBINE_NOTIFY, FishStr};
use fish_hooks::{ClientView, Dispatch, HudView, NativeModRow};

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
    ticks: [AtomicU64; 3],
}

/// The events every loader defines for the client hooks, with their combine modes.
pub(crate) const HOST_EVENTS: [(&str, u8); 8] = [
    (ids::CLIENT_START_TICK, COMBINE_NOTIFY),
    (ids::CLIENT_END_TICK, COMBINE_NOTIFY),
    (ids::SERVER_START_TICK, COMBINE_NOTIFY),
    (ids::SERVER_END_TICK, COMBINE_NOTIFY),
    (ids::FRAME, COMBINE_NOTIFY),
    (ids::HUD_RENDER, COMBINE_NOTIFY),
    (ids::RAW_COMMAND, COMBINE_FIRST_NON_PASS),
    (ids::KEY, COMBINE_NOTIFY),
];

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
            loader,
            ticks: Default::default(),
        }
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

impl Dispatch for LoaderDispatch {
    fn tick_start(&self, game: &mut dyn ClientView) {
        quiet((), || self.tick(game, true));
    }

    fn tick_end(&self, game: &mut dyn ClientView) {
        quiet((), || self.tick(game, false));
    }

    fn frame(&self, game: &mut dyn ClientView, elapsed_secs: f32) {
        quiet((), || {
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
                if rec.is_failed()
                    || rec.state() == ModState::Loading && rec.manifest.library.is_none()
                {
                    continue;
                }
                rows.push(NativeModRow {
                    id: rec.manifest.id.to_string(),
                    title: format!("{} {} [Fish]", rec.manifest.name, rec.version_text),
                    description: rec.manifest.description.clone(),
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
