//! Boot: discovery, resolution, loading and entrypoint execution.

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Arc;

use fish_abi::{FISH_ABI_MAJOR, FISH_ABI_MINOR, FishStatus};
use fish_metadata::{Builtins, Candidate, LoadCondition, ModId, Side, resolve};
use semver::Version;

use crate::bus::EventBus;
use crate::discovery::{Discovery, discover};
use crate::host::LOADER_SERVICE;
use crate::inner::{Inner, InnerInit, LogSink, ModRecord, ModState, Service};
use crate::load::{self, INIT_KEYS};
use crate::report::{AbiReport, BootReport, ModReport};

/// Version of Horizon assumed when the edition build did not provide one.
const UNKNOWN_HORIZON: &str = "0.0.0-dev";

/// Inputs to [`boot`].
pub struct BootConfig {
    /// Client or dedicated server.
    pub side: Side,
    /// The game directory (`fish-mods/`, `config/` and `.fish/` live here).
    pub game_dir: PathBuf,
    /// Horizon version, for the `horizon` builtin.
    pub horizon_version: Version,
    /// Extra development mods (files or directories), loaded as development mods.
    pub extra_mods: Vec<PathBuf>,
    /// A mod panic aborts the process instead of disabling the mod.
    pub strict: bool,
    /// Development mode.
    pub dev: bool,
    /// Where log records go; `None` uses the `log` crate.
    pub log_sink: Option<LogSink>,
}

impl BootConfig {
    /// A config with defaults for everything but side and game directory.
    pub fn new(side: Side, game_dir: impl Into<PathBuf>) -> BootConfig {
        BootConfig {
            side,
            game_dir: game_dir.into(),
            horizon_version: baked_horizon_version(),
            extra_mods: Vec::new(),
            strict: false,
            dev: false,
            log_sink: None,
        }
    }

    /// Like [`new`](Self::new), reading `FISH_LOADER_ADD_MODS`, `FISH_LOADER_STRICT` and
    /// `FISH_LOADER_DEV` from the environment. Only the edition bridge calls this.
    pub fn from_env(side: Side, game_dir: impl Into<PathBuf>) -> BootConfig {
        Self::from_vars(side, game_dir, |name| std::env::var_os(name))
    }

    /// Like [`from_env`](Self::from_env) with an explicit variable lookup (testable).
    pub fn from_vars(
        side: Side,
        game_dir: impl Into<PathBuf>,
        var: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> BootConfig {
        let mut cfg = BootConfig::new(side, game_dir);
        if let Some(paths) = var("FISH_LOADER_ADD_MODS") {
            cfg.extra_mods = std::env::split_paths(&paths)
                .filter(|p| !p.as_os_str().is_empty())
                .collect();
        }
        let flag = |name: &str| var(name).is_some_and(|v| v == "1");
        cfg.strict = flag("FISH_LOADER_STRICT");
        cfg.dev = flag("FISH_LOADER_DEV");
        cfg
    }
}

/// The Horizon version the edition build baked in (`FISH_HORIZON_VERSION`), else `0.0.0-dev`.
fn baked_horizon_version() -> Version {
    option_env!("FISH_HORIZON_VERSION")
        .and_then(|v| Version::parse(v).ok())
        .unwrap_or_else(|| Version::parse(UNKNOWN_HORIZON).expect("constant"))
}

/// Boot failed before any mod could be considered.
#[derive(Debug)]
pub struct BootError(pub String);

impl std::fmt::Display for BootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BootError {}

/// A booted loader. Dropping it drops every listener, then unloads the mod libraries.
#[derive(Clone)]
pub struct Loader {
    pub(crate) inner: Arc<Inner>,
}

/// A mod as seen from outside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModSummary {
    /// Mod id.
    pub id: String,
    /// Mod version.
    pub version: String,
    /// Display name.
    pub name: String,
    /// Current state.
    pub state: ModState,
}

fn remove_failed(
    candidates: &mut Vec<Candidate>,
    failed: &mut Vec<(Candidate, String)>,
    id: &ModId,
    reason: &str,
) {
    let (gone, kept): (Vec<Candidate>, Vec<Candidate>) = std::mem::take(candidates)
        .into_iter()
        .partition(|c| &c.manifest.id == id);
    *candidates = kept;
    for c in gone {
        if c.condition == LoadCondition::Always {
            failed.push((c, reason.to_string()));
        }
    }
}

/// Resolves, removing the mods the resolver names until what remains is consistent.
fn resolve_leniently(
    inner: &Inner,
    mut candidates: Vec<Candidate>,
    builtins: &Builtins,
) -> (fish_metadata::Resolution, Vec<(Candidate, String)>) {
    let mut failed: Vec<(Candidate, String)> = Vec::new();
    loop {
        match resolve(candidates.clone(), inner.side, builtins) {
            Ok(res) => return (res, failed),
            Err(err) => {
                let mut subjects = BTreeSet::new();
                for p in &err.problems {
                    inner.error(p.message.clone());
                    subjects.insert(p.subject.clone());
                }
                let mut removed: Vec<ModId> = Vec::new();
                for id in &subjects {
                    let reason = err
                        .problems
                        .iter()
                        .filter(|p| &p.subject == id)
                        .map(|p| p.message.as_str())
                        .collect::<Vec<_>>()
                        .join("; ");
                    remove_failed(&mut candidates, &mut failed, id, &reason);
                    removed.push(id.clone());
                }
                // Cascade: mods that depended on a removed mod fail with a clear reason.
                loop {
                    let victim = candidates.iter().find_map(|c| {
                        let provides_removed = |key: &ModId| {
                            failed.iter().any(|(f, _)| {
                                f.manifest.id == *key || f.manifest.provides.contains(key)
                            })
                        };
                        let provided_elsewhere = |key: &ModId| {
                            candidates
                                .iter()
                                .any(|o| o.manifest.id == *key || o.manifest.provides.contains(key))
                        };
                        c.manifest
                            .depends
                            .keys()
                            .find(|k| provides_removed(k) && !provided_elsewhere(k))
                            .map(|k| (c.manifest.id.clone(), k.clone()))
                    });
                    let Some((id, dep)) = victim else { break };
                    let reason = format!("dependency {dep} failed to load");
                    remove_failed(&mut candidates, &mut failed, &id, &reason);
                }
            }
        }
    }
}

/// Boots the loader: discovers, resolves, loads and initialises mods.
///
/// Problems with individual mods never fail the boot; they are logged, recorded in the
/// [`BootReport`], and the affected mods (and their dependents) are marked failed.
pub fn boot(cfg: BootConfig) -> Result<Loader, BootError> {
    let game_dir = cfg.game_dir.clone();
    let config_dir = game_dir.join("config");
    let cache_root = game_dir.join(".fish").join("cache");
    for dir in [&config_dir, &cache_root] {
        std::fs::create_dir_all(dir)
            .map_err(|e| BootError(format!("cannot create {}: {e}", dir.display())))?;
    }
    let fish_version =
        Version::parse(env!("CARGO_PKG_VERSION")).expect("package version is semver");
    let inner = Inner::new(InnerInit {
        side: cfg.side,
        dev: cfg.dev,
        strict: cfg.strict,
        game_dir: game_dir.display().to_string(),
        config_dir: config_dir.display().to_string(),
        fish_version: fish_version.clone(),
        horizon_version: cfg.horizon_version.clone(),
        sink: cfg.log_sink.clone(),
    });
    inner.services.write().expect("services").insert(
        fish_abi::services::SERVICE_LOADER.to_string(),
        Service {
            version: 1,
            ptr: std::ptr::from_ref(&LOADER_SERVICE) as usize,
        },
    );

    for (id, combine) in crate::dispatch::HOST_EVENTS
        .into_iter()
        .chain(fish_abi::game::EVENTS)
    {
        let status = inner.bus.define(id, combine);
        debug_assert!(status.is_ok(), "defining {id}");
    }
    for (id, table) in [
        (
            fish_abi::client::services::CLIENT,
            std::ptr::from_ref(&crate::client_services::CLIENT) as usize,
        ),
        (
            fish_abi::client::services::HUD,
            std::ptr::from_ref(&crate::client_services::HUD) as usize,
        ),
        (
            fish_abi::client::services::INPUT,
            std::ptr::from_ref(&crate::client_services::INPUT) as usize,
        ),
        (
            fish_abi::client::services::COMMAND_RAW,
            std::ptr::from_ref(&crate::client_services::COMMAND_RAW) as usize,
        ),
        (
            fish_abi::networking::SERVICE_NETWORKING,
            std::ptr::from_ref(&crate::net_services::NETWORKING) as usize,
        ),
        (
            fish_abi::registry::SERVICE_REGISTRY,
            std::ptr::from_ref(&crate::registry_services::REGISTRY) as usize,
        ),
        (
            fish_abi::commands::SERVICE_COMMANDS,
            std::ptr::from_ref(&crate::command_services::COMMANDS) as usize,
        ),
        (
            fish_abi::gamerules::SERVICE_GAME_RULE,
            std::ptr::from_ref(&crate::gamerule_services::GAME_RULE) as usize,
        ),
        (
            fish_abi::blocks::SERVICE_BLOCK,
            std::ptr::from_ref(&crate::block_services::BLOCK) as usize,
        ),
    ] {
        inner.services.write().expect("services").insert(
            id.to_string(),
            Service {
                version: 1,
                ptr: table,
            },
        );
    }

    let Discovery {
        candidates,
        mut archives,
        errors,
    } = discover(&game_dir, &cfg.extra_mods);
    for e in errors {
        inner.error(e);
    }
    inner.emit(
        log::Level::Info,
        "fish_loader",
        format!(
            "Fish {fish_version} (ABI {FISH_ABI_MAJOR}.{FISH_ABI_MINOR}) found {} mod candidate(s)",
            candidates.len()
        ),
    );

    let builtins = Builtins::new(fish_version, cfg.horizon_version);
    let (resolution, failed) = resolve_leniently(&inner, candidates, &builtins);
    for w in &resolution.warnings {
        inner.warn(w.clone());
    }

    // Records: load order first, then mods that failed during resolution.
    let mut records: Vec<Arc<ModRecord>> = Vec::new();
    let inner_ptr = Arc::as_ptr(&inner);
    for m in &resolution.active {
        let token = records.len() as u64 + 1;
        records.push(Arc::new(ModRecord::new(
            token,
            m.manifest.clone(),
            m.origin.clone(),
            inner_ptr,
        )));
    }
    let active_count = records.len();
    for (c, reason) in &failed {
        let token = records.len() as u64 + 1;
        let rec = Arc::new(ModRecord::new(
            token,
            c.manifest.clone(),
            c.origin.clone(),
            inner_ptr,
        ));
        *rec.state.lock().expect("mod state") = ModState::Failed(reason.clone());
        rec.failed.store(true, std::sync::atomic::Ordering::Release);
        records.push(rec);
    }
    *inner.mods.write().expect("mods") = records.clone();
    let active = &records[..active_count];

    // Load libraries in load order.
    let platform = fish_pack::platform();
    for rec in active {
        if rec.is_failed() || rec.manifest.library.is_none() {
            continue;
        }
        let Some(archive) = archives.get_mut(&rec.origin) else {
            inner.fail_mod(
                rec.token,
                "its archive is no longer available".into(),
                false,
            );
            continue;
        };
        let dest = cache_root.join(format!(
            "{}-{}-{}",
            rec.manifest.id,
            rec.version_text,
            &archive.fingerprint()[..16]
        ));
        let loaded = fish_pack::extract_library(archive, &platform, &dest)
            .map_err(|e| e.to_string())
            .and_then(|x| {
                inner.emit(
                    log::Level::Debug,
                    "fish_loader",
                    format!(
                        "{} library of {}",
                        if x.reused {
                            "reused cached"
                        } else {
                            "extracted"
                        },
                        rec.manifest.id
                    ),
                );
                load::load(&x.library, &rec.manifest)
            });
        match loaded {
            Ok(l) => {
                *rec.library.lock().expect("library") = Some(l.library);
                *rec.entries.lock().expect("entries") = l.entries;
            }
            Err(msg) => inner.fail_mod(rec.token, msg, false),
        }
    }

    // Entrypoints: all pre_launch, then all main, then client/server.
    let side_key = match cfg.side {
        Side::Client => "client",
        Side::Server => "server",
    };
    for key in ["pre_launch", "main", side_key] {
        debug_assert!(INIT_KEYS.contains(&key));
        for rec in active {
            run_entrypoints(&inner, rec, key);
        }
    }
    inner.keep_block_archives(&mut archives);
    drop(archives);
    for rec in active {
        let mut state = rec.state.lock().expect("mod state");
        if *state == ModState::Loading {
            *state = ModState::Ok;
        }
    }
    inner
        .net
        .frozen
        .store(true, std::sync::atomic::Ordering::Release);
    for (id, kind) in [
        (
            fish_abi::commands::ids::REGISTER,
            fish_abi::commands::KIND_SERVER,
        ),
        (
            fish_abi::commands::ids::REGISTER_CLIENT,
            fish_abi::commands::KIND_CLIENT,
        ),
    ] {
        let mut payload = fish_abi::commands::CommandRegisterPayload {
            size: size_of::<fish_abi::commands::CommandRegisterPayload>() as u32,
            kind,
            side: match cfg.side {
                Side::Client => fish_abi::services::SIDE_CLIENT,
                Side::Server => fish_abi::services::SIDE_SERVER,
            },
            reserved: [0; 2],
        };
        if let Some(index) = inner.bus.lookup(id) {
            // SAFETY: the payload matches the event's documented layout.
            unsafe {
                inner
                    .bus
                    .invoke(index, (&raw mut payload).cast(), std::ptr::null_mut())
            };
        }
    }
    let ok = active.iter().filter(|r| !r.is_failed()).count();
    inner.emit(
        log::Level::Info,
        "fish_loader",
        format!("Fish loaded {ok} of {} mod(s)", records.len()),
    );
    Ok(Loader { inner })
}

fn run_entrypoints(inner: &Inner, rec: &Arc<ModRecord>, key: &str) {
    for name in rec.manifest.entrypoints_for(key) {
        if rec.is_failed() {
            return;
        }
        let init = {
            let entries = rec.entries.lock().expect("entries");
            entries
                .iter()
                .find(|e| e.key == key && &e.name == name)
                .and_then(|e| e.init)
        };
        let Some(init) = init else { continue };
        // SAFETY: `init` is the mod's own entrypoint; the host table lives in the record.
        let status = unsafe { init(&raw const *rec.host_table) };
        if status == FishStatus::PANICKED {
            inner.fail_mod(rec.token, format!("entrypoint {key}={name} panicked"), true);
        } else if !status.is_ok() {
            inner.fail_mod(
                rec.token,
                format!("entrypoint {key}={name} returned {}", status.name()),
                false,
            );
        }
    }
}

impl Loader {
    /// Uses `out` as this loader's wire instead of the process-wide one (tests, in-memory pairs).
    pub fn attach_net(&self, out: std::sync::Arc<dyn fish_hooks::NetOut>) {
        self.inner.net.attach(out);
    }

    /// Adds `id` to the Fish registry `registry` on behalf of mod `owner` and returns its raw id.
    /// Entries of mods that run on both sides are synchronised with the server; the others are
    /// local. Registration order assigns raw ids.
    pub fn registry_register(&self, registry: &str, id: &str, owner: &str) -> Result<u32, String> {
        let rec = self
            .inner
            .records()
            .into_iter()
            .find(|r| r.manifest.id.as_str() == owner)
            .ok_or_else(|| format!("no mod {owner}"))?;
        let synced = rec.manifest.environment == fish_metadata::Environment::Any;
        Ok(self
            .inner
            .net
            .registries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .register(registry, id, synced, rec.token))
    }

    /// The raw id of `id` in `registry`, after any synchronisation with a server.
    pub fn registry_raw(&self, registry: &str, id: &str) -> Option<u32> {
        self.inner
            .net
            .registries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .raw(registry, id)
    }

    /// The entries of `registry` as `(id, raw)`, in registration order.
    pub fn registry_entries(&self, registry: &str) -> Vec<(String, u32)> {
        self.inner
            .net
            .registries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries(registry)
    }

    /// A stable hash of the synchronised registry entries.
    pub fn registry_hash(&self) -> String {
        self.inner
            .net
            .registries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .hash()
    }

    /// A snapshot of mods, in load order with failed-at-resolution mods last.
    pub fn mods(&self) -> Vec<ModSummary> {
        self.inner
            .records()
            .iter()
            .map(|r| ModSummary {
                id: r.manifest.id.to_string(),
                version: r.version_text.clone(),
                name: r.manifest.name.clone(),
                state: r.state(),
            })
            .collect()
    }

    /// The state of mod `id`.
    pub fn mod_state(&self, id: &str) -> Option<ModState> {
        self.inner
            .records()
            .iter()
            .find(|r| r.manifest.id.as_str() == id)
            .map(|r| r.state())
    }

    /// The report so far (updates as mods fail at runtime).
    pub fn report(&self) -> BootReport {
        let mods = self
            .mods()
            .into_iter()
            .map(|m| {
                let (status, reason) = match m.state {
                    ModState::Ok => ("ok", None),
                    ModState::Loading => ("loading", None),
                    ModState::Failed(r) => ("failed", Some(r)),
                };
                ModReport {
                    id: m.id,
                    version: m.version,
                    status: status.to_string(),
                    reason,
                }
            })
            .collect();
        BootReport {
            fish: self.inner.fish_version.to_string(),
            abi: AbiReport {
                major: FISH_ABI_MAJOR,
                minor: FISH_ABI_MINOR,
            },
            side: match self.inner.side {
                Side::Client => "client",
                Side::Server => "server",
            }
            .to_string(),
            mods,
            warnings: self.inner.warnings.lock().expect("warnings").clone(),
            errors: self.inner.errors.lock().expect("errors").clone(),
        }
    }

    /// The event bus (host-defined events are fired through it).
    pub fn bus(&self) -> &EventBus {
        &self.inner.bus
    }

    /// Which side this loader serves.
    pub fn side(&self) -> Side {
        self.inner.side
    }

    /// The Horizon version served as the `horizon` builtin.
    pub fn horizon_version(&self) -> &Version {
        &self.inner.horizon_version
    }

    /// The game directory.
    pub fn game_dir(&self) -> &str {
        &self.inner.game_dir
    }

    /// Defines a host-owned event.
    pub fn define_event(&self, id: &str, combine: u8) -> FishStatus {
        self.inner.bus.define(id, combine)
    }

    /// Fires an event by id.
    ///
    /// # Safety
    /// `payload` and `out` must match what the event's listeners expect.
    pub unsafe fn fire(&self, id: &str, payload: *mut c_void, out: *mut c_void) -> FishStatus {
        // SAFETY: forwarded to the caller.
        unsafe { self.inner.bus.invoke_id(id, payload, out) }
    }

    /// Reads a value from the object share.
    pub fn object_share_get(&self, key: &str, tag: &str) -> Result<*mut c_void, FishStatus> {
        self.inner.share_get(key, tag)
    }

    /// Registers a service table other mods can look up.
    ///
    /// # Safety
    /// `table` must stay valid for the life of the loader and match what `id`'s version promises.
    pub unsafe fn register_service(
        &self,
        id: &str,
        version: u32,
        table: *const c_void,
    ) -> FishStatus {
        if !fish_abi::ident::is_valid_identifier(id) || table.is_null() {
            return FishStatus::INVALID_ARGUMENT;
        }
        let mut services = self.inner.services.write().expect("services");
        if services.contains_key(id) {
            return FishStatus::ALREADY_EXISTS;
        }
        services.insert(
            id.to_string(),
            Service {
                version,
                ptr: table as usize,
            },
        );
        FishStatus::OK
    }
}
