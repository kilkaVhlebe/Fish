//! An in-process Fish host for tests.
//!
//! [`TestHost`] boots a real [`fish_loader::Loader`] in a temporary game directory, captures its
//! log records, and lets tests fire events and read the object share. Several hosts can coexist in
//! one process: each extracts mod libraries into its own directory, so each gets its own module
//! instances.

#![deny(missing_docs)]

pub mod script;
pub mod sim;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, LazyLock, Mutex};

use fish_api::event::Payload;
use fish_hooks::{Dispatch, Role};
use fish_loader::{BootConfig, BootError, Loader, LoaderDispatch, LogRecord, ModState, Side};
use fish_pack::PackSpec;

/// A booted (or about to boot) host in a temporary game directory.
pub struct TestHost {
    dir: tempfile::TempDir,
    side: Side,
    strict: bool,
    dev: bool,
    dev_mods: Vec<PathBuf>,
    logs: Arc<Mutex<Vec<LogRecord>>>,
    loader: Option<Loader>,
    dispatch: Option<LoaderDispatch>,
    /// The simulated client driven by scripts.
    pub sim: sim::Sim,
}

impl Default for TestHost {
    fn default() -> Self {
        Self::new()
    }
}

impl TestHost {
    /// A new host with an empty game directory.
    pub fn new() -> TestHost {
        TestHost {
            dir: tempfile::tempdir().expect("temporary directory"),
            side: Side::Client,
            strict: false,
            dev: false,
            dev_mods: Vec::new(),
            logs: Arc::default(),
            loader: None,
            dispatch: None,
            sim: sim::Sim::default(),
        }
    }

    /// The game directory.
    pub fn game_dir(&self) -> &Path {
        self.dir.path()
    }

    /// Installs a packed mod into `fish-mods/` (under its own file name).
    pub fn add_mod(self, path: impl AsRef<Path>) -> TestHost {
        let path = path.as_ref();
        let name = path.file_name().expect("mod path has a file name");
        self.add_mod_as(path, &name.to_string_lossy())
    }

    /// Installs a packed mod into `fish-mods/` under `file_name`.
    pub fn add_mod_as(self, path: impl AsRef<Path>, file_name: &str) -> TestHost {
        let dir = self.dir.path().join("fish-mods");
        std::fs::create_dir_all(&dir).expect("create fish-mods");
        std::fs::copy(path.as_ref(), dir.join(file_name)).expect("copy mod into fish-mods");
        self
    }

    /// Adds a development mod (file or directory) outside `fish-mods/`.
    pub fn add_dev_mod(mut self, path: impl Into<PathBuf>) -> TestHost {
        self.dev_mods.push(path.into());
        self
    }

    /// Chooses the side (default: client).
    pub fn side(mut self, side: Side) -> TestHost {
        self.side = side;
        self
    }

    /// Makes a mod panic abort the process.
    pub fn strict(mut self, strict: bool) -> TestHost {
        self.strict = strict;
        self
    }

    /// Turns development mode on.
    pub fn dev(mut self, dev: bool) -> TestHost {
        self.dev = dev;
        self
    }

    /// Boots the loader, capturing its log.
    pub fn boot(mut self) -> Result<TestHost, BootError> {
        let logs = self.logs.clone();
        let mut cfg = BootConfig::new(self.side, self.dir.path());
        cfg.extra_mods = self.dev_mods.clone();
        cfg.strict = self.strict;
        cfg.dev = self.dev;
        cfg.log_sink = Some(Arc::new(move |r| logs.lock().expect("log lock").push(r)));
        let loader = fish_loader::boot(cfg)?;
        self.dispatch = Some(LoaderDispatch::new(loader.clone()));
        self.loader = Some(loader);
        Ok(self)
    }

    /// Unloads the loader and boots again in the same game directory (logs are kept).
    pub fn reboot(mut self) -> Result<TestHost, BootError> {
        self.dispatch = None;
        self.loader = None;
        self.boot()
    }

    fn game_role(&self) -> Role {
        match self.side {
            Side::Client => Role::Window,
            Side::Server => Role::DedicatedServer,
        }
    }

    fn view(&mut self, role: Role) -> (sim::SimView<'_>, &LoaderDispatch) {
        let dir = self.dir.path().to_path_buf();
        let dispatch = self
            .dispatch
            .as_ref()
            .expect("TestHost::boot() has not been called");
        (
            sim::SimView {
                role,
                dir,
                sim: &mut self.sim,
            },
            dispatch,
        )
    }

    /// Advances `n` ticks of the main game (client, or dedicated server on a server host).
    pub fn tick(&mut self, n: u32) {
        let role = self.game_role();
        self.tick_as(role, n);
        self.sim.ticks += u64::from(n);
    }

    /// Advances `n` ticks of the integrated server (a second game next to the window).
    pub fn tick_integrated_server(&mut self, n: u32) {
        self.tick_as(Role::IntegratedServer, n);
    }

    fn tick_as(&mut self, role: Role, n: u32) {
        let (mut view, dispatch) = self.view(role);
        for _ in 0..n {
            dispatch.tick_start(&mut view);
            dispatch.tick_end(&mut view);
        }
    }

    /// Renders `n` frames (frame event, then the HUD pass); `sim.hud` holds the last frame's text.
    pub fn frame(&mut self, n: u32) {
        let role = self.game_role();
        let mut texts = Vec::new();
        for _ in 0..n {
            let mut hud = sim::SimHud::default();
            let (mut view, dispatch) = self.view(role);
            dispatch.frame(&mut view, 1.0 / 60.0);
            dispatch.hud(&mut view, &mut hud);
            texts = hud.texts;
        }
        self.sim.frames += u64::from(n);
        if n > 0 {
            self.sim.hud = texts;
        }
    }

    /// Runs a client command; returns true when a mod handled it.
    pub fn command(&mut self, command: &str) -> bool {
        self.sim.commands.push(command.to_string());
        let role = self.game_role();
        let (mut view, dispatch) = self.view(role);
        dispatch.client_command(&mut view, command)
    }

    /// Presses or releases a key.
    pub fn key(&mut self, name: &str, pressed: bool) {
        self.sim.press_key(name, pressed);
        let role = self.game_role();
        let (mut view, dispatch) = self.view(role);
        dispatch.key(&mut view, name, pressed);
    }

    /// The player sends a chat message.
    pub fn chat(&mut self, text: &str) {
        self.sim.sent_chat.push(text.to_string());
    }

    /// The loader.
    ///
    /// # Panics
    /// Before [`boot`](Self::boot).
    pub fn loader(&self) -> &Loader {
        self.loader
            .as_ref()
            .expect("TestHost::boot() has not been called")
    }

    /// Every record logged so far.
    pub fn logs(&self) -> Vec<LogRecord> {
        self.logs.lock().expect("log lock").clone()
    }

    /// Messages logged by mod `id` (target `fish/<id>`), in order.
    pub fn mod_log(&self, id: &str) -> Vec<String> {
        let target = format!("fish/{id}");
        self.logs()
            .into_iter()
            .filter(|r| r.target == target)
            .map(|r| r.message)
            .collect()
    }

    /// True if any record's message contains `needle`.
    pub fn logged(&self, needle: &str) -> bool {
        self.logs().iter().any(|r| r.message.contains(needle))
    }

    /// The state of mod `id`.
    pub fn state(&self, id: &str) -> Option<ModState> {
        self.loader().mod_state(id)
    }

    /// Fires a `Notify` event with `payload`.
    pub fn fire<P: Payload>(&self, event_id: &str, payload: &mut P) {
        // SAFETY: `P: Payload` guarantees the documented layout; `out` is null for Notify events.
        let st = unsafe {
            self.loader()
                .fire(event_id, (payload as *mut P).cast(), std::ptr::null_mut())
        };
        assert!(st.is_ok(), "firing {event_id} failed: {}", st.name());
    }

    /// Fires a `FirstNonPass` event; `R` must start with an action byte initialised to `Pass`.
    pub fn fire_with_result<P: Payload, R>(&self, event_id: &str, payload: &mut P, result: &mut R) {
        // SAFETY: as for `fire`; `result` is a valid out value for the event.
        let st = unsafe {
            self.loader().fire(
                event_id,
                (payload as *mut P).cast(),
                (result as *mut R).cast(),
            )
        };
        assert!(st.is_ok(), "firing {event_id} failed: {}", st.name());
    }

    /// Reads a value another mod published to the object share (`T` as the publisher used).
    pub fn share<T: 'static>(&self, key: &str) -> Option<&'static T> {
        let p = self
            .loader()
            .object_share_get(key, std::any::type_name::<T>())
            .ok()?;
        // SAFETY: the tag matched, so the publisher stored a `&'static T`.
        Some(unsafe { &*p.cast::<T>() })
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

static BUILT: LazyLock<Mutex<HashMap<String, PathBuf>>> = LazyLock::new(Mutex::default);

/// Serialises `cargo build` + packing so concurrent tests never race on one output file.
static PACK_LOCK: Mutex<()> = Mutex::new(());

/// Builds `examples/mods/<package>` and returns the path of its packed `.fish` (memoised).
///
/// Uses a private target directory (`target/fish-test-mods`) so it never blocks on the outer
/// `cargo test` lock.
pub fn build_example(package: &str) -> PathBuf {
    let mut built = BUILT.lock().expect("build lock");
    if let Some(p) = built.get(package) {
        return p.clone();
    }
    let manifest = workspace_root()
        .join("examples/mods")
        .join(package)
        .join("fish.mod.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("reading {}: {e}", manifest.display()));
    let out = pack_example(package, &text, package);
    built.insert(package.to_string(), out.clone());
    out
}

/// Like [`build_example`], but with `manifest_text` instead of the example's own manifest and an
/// output name of `<out_name>.fish`. For tests that need a deliberately inconsistent mod.
pub fn pack_example(package: &str, manifest_text: &str, out_name: &str) -> PathBuf {
    let _guard = PACK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = workspace_root();
    let target = root.join("target/fish-test-mods");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(&root)
        .args([
            "build",
            "--locked",
            "-p",
            package,
            "--message-format=json",
            "--target-dir",
        ])
        .arg(&target)
        .output()
        .expect("spawning cargo");
    assert!(
        output.status.success(),
        "cargo build -p {package} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let library = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["reason"] == "compiler-artifact" && v["target"]["name"] == package)
        .flat_map(|v| v["filenames"].as_array().cloned().unwrap_or_default())
        .filter_map(|f| f.as_str().map(PathBuf::from))
        .find(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("dll" | "so" | "dylib")
            )
        })
        .unwrap_or_else(|| panic!("no cdylib artifact for {package}"));
    let mut spec = PackSpec::new(manifest_text);
    let file = library
        .file_name()
        .expect("library file name")
        .to_string_lossy()
        .into_owned();
    spec.add_file(&format!("bin/{}/{file}", fish_pack::platform()), &library)
        .expect("add library");
    let assets = root.join("examples/mods").join(package).join("assets");
    if assets.is_dir() {
        spec.add_dir("assets", &assets).expect("add assets");
    }
    let out = target.join("packed").join(format!("{out_name}.fish"));
    fish_pack::write(spec, &out).expect("write .fish");
    out
}
