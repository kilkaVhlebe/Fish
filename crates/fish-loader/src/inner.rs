//! Shared loader state, mod records and failure handling.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};

use fish_abi::services::{FishModInfoV1, MOD_STATUS_FAILED, MOD_STATUS_LOADING, MOD_STATUS_OK};
use fish_abi::{FishHostV1, FishStr, InitFn, ObjectFn};
use fish_metadata::{Manifest, Origin, Side};
use semver::Version;

use crate::bus::{BusHooks, EventBus};
use crate::host;

/// One log record produced by the loader or by a mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    /// Severity.
    pub level: log::Level,
    /// Target (`fish_loader`, or `fish/<mod id>` for mods).
    pub target: String,
    /// The message.
    pub message: String,
}

/// Destination for [`LogRecord`]s.
pub type LogSink = Arc<dyn Fn(LogRecord) + Send + Sync>;

/// Life-cycle state of a mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModState {
    /// Library loaded; entrypoints still running.
    Loading,
    /// Fully initialised.
    Ok,
    /// Failed with the given reason; the mod no longer receives events.
    Failed(String),
}

/// An entrypoint exported by a mod library.
pub(crate) struct EntryRec {
    pub key: String,
    pub name: String,
    pub init: Option<InitFn>,
    pub object: Option<ObjectFn>,
}

/// Everything the loader knows about one mod.
pub(crate) struct ModRecord {
    pub token: u64,
    pub manifest: Manifest,
    pub origin: Origin,
    pub version_text: String,
    pub state: Mutex<ModState>,
    pub failed: Arc<AtomicBool>,
    pub library: Mutex<Option<libloading::Library>>,
    pub entries: Mutex<Vec<EntryRec>>,
    pub host_table: Box<FishHostV1>,
}

impl ModRecord {
    pub(crate) fn new(token: u64, manifest: Manifest, origin: Origin, inner: *const Inner) -> Self {
        let host_table = Box::new(host::host_table(inner, token));
        ModRecord {
            token,
            version_text: manifest.version.to_string(),
            manifest,
            origin,
            state: Mutex::new(ModState::Loading),
            failed: Arc::new(AtomicBool::new(false)),
            library: Mutex::new(None),
            entries: Mutex::new(Vec::new()),
            host_table,
        }
    }

    pub(crate) fn state(&self) -> ModState {
        self.state.lock().expect("mod state").clone()
    }

    pub(crate) fn is_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    /// True if this mod answers to `key` (its id or an alias).
    pub(crate) fn answers_to(&self, key: &str) -> bool {
        self.manifest.id.as_str() == key || self.manifest.provides.iter().any(|p| p.as_str() == key)
    }

    /// The ABI view of this mod; strings point into this record.
    pub(crate) fn info(&self) -> FishModInfoV1 {
        FishModInfoV1 {
            id: FishStr::new(self.manifest.id.as_str()),
            version: FishStr::new(&self.version_text),
            name: FishStr::new(&self.manifest.name),
            description: FishStr::new(&self.manifest.description),
            status: match self.state() {
                ModState::Loading => MOD_STATUS_LOADING,
                ModState::Ok => MOD_STATUS_OK,
                ModState::Failed(_) => MOD_STATUS_FAILED,
            },
        }
    }
}

pub(crate) struct Service {
    pub version: u32,
    pub ptr: usize,
}

/// Shared state behind a [`Loader`](crate::Loader) and every host table.
pub(crate) struct Inner {
    pub bus: EventBus,
    pub side: Side,
    pub dev: bool,
    pub strict: bool,
    pub game_dir: String,
    pub config_dir: String,
    pub fish_version: Version,
    pub horizon_version: Version,
    pub mods: RwLock<Vec<Arc<ModRecord>>>,
    pub services: RwLock<HashMap<String, Service>>,
    pub share: Mutex<HashMap<String, (String, usize)>>,
    pub sink: Option<LogSink>,
    pub warnings: Mutex<Vec<String>>,
    pub errors: Mutex<Vec<String>>,
}

/// Construction parameters of [`Inner`].
pub(crate) struct InnerInit {
    pub side: Side,
    pub dev: bool,
    pub strict: bool,
    pub game_dir: String,
    pub config_dir: String,
    pub fish_version: Version,
    pub horizon_version: Version,
    pub sink: Option<LogSink>,
}

impl Inner {
    pub(crate) fn new(init: InnerInit) -> Arc<Inner> {
        let InnerInit {
            side,
            dev,
            strict,
            game_dir,
            config_dir,
            fish_version,
            horizon_version,
            sink,
        } = init;
        Arc::new_cyclic(|weak: &Weak<Inner>| {
            let (w1, w2) = (weak.clone(), weak.clone());
            let hooks = BusHooks {
                on_panic: Box::new(move |token, msg| {
                    if let Some(inner) = w1.upgrade() {
                        inner.fail_mod(token, msg, true);
                    }
                }),
                warn: Box::new(move |msg| {
                    if let Some(inner) = w2.upgrade() {
                        inner.warn(msg);
                    }
                }),
            };
            Inner {
                bus: EventBus::new(hooks),
                side,
                dev,
                strict,
                game_dir,
                config_dir,
                fish_version,
                horizon_version,
                mods: RwLock::default(),
                services: RwLock::default(),
                share: Mutex::default(),
                sink,
                warnings: Mutex::default(),
                errors: Mutex::default(),
            }
        })
    }

    pub(crate) fn emit(&self, level: log::Level, target: &str, message: String) {
        match &self.sink {
            Some(sink) => sink(LogRecord {
                level,
                target: target.to_string(),
                message,
            }),
            None => log::log!(target: target, level, "{message}"),
        }
    }

    pub(crate) fn warn(&self, message: String) {
        self.emit(log::Level::Warn, "fish_loader", message.clone());
        self.warnings.lock().expect("warnings").push(message);
    }

    pub(crate) fn error(&self, message: String) {
        self.emit(log::Level::Error, "fish_loader", message.clone());
        self.errors.lock().expect("errors").push(message);
    }

    pub(crate) fn record(&self, token: u64) -> Option<Arc<ModRecord>> {
        let index = usize::try_from(token.checked_sub(1)?).ok()?;
        self.mods.read().expect("mods").get(index).cloned()
    }

    pub(crate) fn records(&self) -> Vec<Arc<ModRecord>> {
        self.mods.read().expect("mods").clone()
    }

    /// Marks a mod failed, skips its listeners from now on, and fails its dependents.
    pub(crate) fn fail_mod(&self, token: u64, reason: String, panicked: bool) {
        let Some(rec) = self.record(token) else {
            return;
        };
        {
            let mut state = rec.state.lock().expect("mod state");
            if matches!(*state, ModState::Failed(_)) {
                return;
            }
            *state = ModState::Failed(reason.clone());
        }
        rec.failed.store(true, Ordering::Release);
        self.error(format!("Fish mod {} stopped: {reason}", rec.manifest.name));
        if panicked && self.strict {
            eprintln!(
                "fish: mod {} panicked and FISH_LOADER_STRICT is set; aborting",
                rec.manifest.id
            );
            std::process::abort();
        }
        for dep in self.records() {
            if dep.token != rec.token
                && !dep.is_failed()
                && dep
                    .manifest
                    .depends
                    .keys()
                    .any(|k| rec.answers_to(k.as_str()))
            {
                self.fail_mod(
                    dep.token,
                    format!("dependency {} failed to load", rec.manifest.id),
                    false,
                );
            }
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Listener destructors live in mod libraries: drop them before the libraries unload.
        self.bus.clear();
    }
}
