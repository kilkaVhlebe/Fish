//! The edition's self test: boots, drives the hook dispatch with synthetic views, reports JSON.
//!
//! Run with `FISH_LOADER_SELFTEST=<report path>`; the edition exits with 0 if every check passed.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use fish_hooks::{ClientView, Dispatch, HudView, PlayerSnapshot, Role};
use serde::{Deserialize, Serialize};

use crate::dispatch::LoaderDispatch;
use crate::report::BootReport;
use crate::{Loader, Side};

/// One named check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// What was checked.
    pub name: String,
    /// Whether it held.
    pub ok: bool,
    /// Extra information.
    #[serde(default)]
    pub detail: String,
}

/// The boot report plus the checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelftestReport {
    /// The loader's report.
    #[serde(flatten)]
    pub boot: BootReport,
    /// True if every check passed.
    pub ok: bool,
    /// The checks, in order.
    pub checks: Vec<Check>,
}

struct SyntheticGame {
    role: Role,
    chat: Vec<String>,
}

impl ClientView for SyntheticGame {
    fn role(&self) -> Role {
        self.role
    }
    fn player(&self) -> Option<PlayerSnapshot> {
        None
    }
    fn show_chat(&mut self, text: &str) {
        self.chat.push(text.to_string());
    }
    fn game_dir(&self) -> PathBuf {
        PathBuf::from(".")
    }
}

#[derive(Default)]
struct SyntheticHud {
    texts: Vec<(String, f32, f32)>,
}

impl HudView for SyntheticHud {
    fn fill(&mut self, _: [f32; 4], _: u32) {}
    fn text(&mut self, text: &str, x: f32, y: f32, _: u32, _: bool) {
        self.texts.push((text.to_string(), x, y));
    }
    fn text_width(&self, text: &str) -> f32 {
        6.0 * text.chars().count() as f32
    }
    fn gui_size(&self) -> [f32; 2] {
        [427.0, 240.0]
    }
}

fn check(checks: &mut Vec<Check>, name: &str, ok: bool, detail: impl Into<String>) {
    checks.push(Check {
        name: name.to_string(),
        ok,
        detail: detail.into(),
    });
}

/// Number of synthetic ticks the self test drives.
pub const TICKS: u64 = 20;

/// Runs the checks against `loader` through the dispatcher the hooks use.
pub fn run(loader: &Loader, dispatch: &LoaderDispatch) -> SelftestReport {
    let mut checks = Vec::new();
    let role = match loader.side() {
        Side::Client => Role::Window,
        Side::Server => Role::DedicatedServer,
    };
    let mut game = SyntheticGame {
        role,
        chat: Vec::new(),
    };
    for _ in 0..TICKS {
        dispatch.tick_start(&mut game);
        dispatch.tick_end(&mut game);
    }
    let mut hud = SyntheticHud::default();
    dispatch.frame(&mut game, 1.0 / 60.0);
    dispatch.hud(&mut game, &mut hud);
    let handled = dispatch.client_command(&mut game, "fish hello");
    dispatch.key(&mut game, "key.keyboard.h", true);

    let report = loader.report();
    check(
        &mut checks,
        "boot_clean",
        report.errors.is_empty(),
        report.errors.join("; "),
    );
    let failed: Vec<&str> = report
        .mods
        .iter()
        .filter(|m| m.status != "ok")
        .map(|m| m.id.as_str())
        .collect();
    check(&mut checks, "mods_ok", failed.is_empty(), failed.join(", "));
    check(
        &mut checks,
        "hooks_installed",
        fish_hooks::get().is_some(),
        "",
    );

    if report.mods.iter().any(|m| m.id == "hello") {
        let ticks = loader
            .object_share_get("hello:ticks", std::any::type_name::<AtomicU64>())
            .ok()
            // SAFETY: the tag matched, so the publisher stored a `&'static AtomicU64`.
            .map(|p| unsafe { &*p.cast::<AtomicU64>() }.load(Ordering::SeqCst));
        check(
            &mut checks,
            "hello_ticks",
            ticks == Some(TICKS) || loader.side() == Side::Server && ticks == Some(0),
            format!("hello:ticks = {ticks:?}"),
        );
        if loader.side() == Side::Client {
            check(
                &mut checks,
                "hello_hud",
                hud.texts
                    .iter()
                    .any(|(t, x, y)| t == "Hello from Fish" && *x == 4.0 && *y == 4.0),
                format!("{:?}", hud.texts),
            );
            check(
                &mut checks,
                "hello_command",
                handled && game.chat.iter().any(|c| c.contains("Fish says hello")),
                format!("{handled}, {:?}", game.chat),
            );
        }
    }
    if report.mods.iter().any(|m| m.id == "content_demo") {
        content_checks(&mut checks, dispatch);
    }
    let ok = checks.iter().all(|c| c.ok);
    SelftestReport {
        boot: report,
        ok,
        checks,
    }
}

/// The checks of phase P8 when the example mod `content_demo` is loaded: its blocks (one with
/// state properties), the content folder the loader writes for Horizon, the content tables, the
/// sound event and the particle type, all through the dispatcher the hooks use.
fn content_checks(checks: &mut Vec<Check>, dispatch: &LoaderDispatch) {
    use fish_hooks::Dispatch;
    let mut blocks = Vec::new();
    dispatch.blocks(&mut blocks);
    let lamp = blocks.iter().find(|b| b.id == "content_demo:ruby_lamp");
    let lamp_properties: Vec<&str> = lamp
        .map(|b| b.properties.iter().map(|p| p.name.as_str()).collect())
        .unwrap_or_default();
    check(
        checks,
        "content_blocks",
        blocks.len() >= 6 && lamp_properties == ["lit", "level"],
        format!(
            "{} block(s), lamp properties {lamp_properties:?}",
            blocks.len()
        ),
    );
    let folders = dispatch.block_content();
    let folder = folders.iter().find(|f| {
        f.join("data/content_demo/horizon/block/ruby_lamp.json")
            .is_file()
    });
    let declaration = folder
        .and_then(|f| {
            std::fs::read_to_string(f.join("data/content_demo/horizon/block/ruby_lamp.json")).ok()
        })
        .unwrap_or_default();
    let assets = folder.is_some_and(|f| {
        [
            "assets/content_demo/sounds.json",
            "assets/content_demo/sounds/ruby_chime.ogg",
            "assets/content_demo/particles/ruby_sparkle.json",
            "assets/content_demo/blockstates/ruby_lamp.json",
        ]
        .iter()
        .all(|p| f.join(p).is_file())
    });
    check(
        checks,
        "content_folder",
        declaration.contains("\"state_properties\"") && assets,
        format!("{} folder(s), assets {assets}", folders.len()),
    );
    let tables = dispatch.content_tables();
    let (mut sounds, mut particles) = (Vec::new(), Vec::new());
    dispatch.sound_events(&mut sounds);
    dispatch.particle_types(&mut particles);
    check(
        checks,
        "content_tables",
        tables.fuels.len() == 1
            && tables.transforms.len() == 2
            && dispatch.sculk_frequency("minecraft:eat") == Some(12)
            && sounds.iter().any(|s| s.id == "content_demo:ruby_chime")
            && particles
                .iter()
                .any(|p| p.id == "content_demo:ruby_sparkle"),
        crate::tables::summary(&tables, sounds.len(), particles.len()),
    );
}

/// Runs the self test and writes the JSON report to `path`; returns the process exit code.
pub fn run_and_write(path: &Path, loader: &Loader, dispatch: &LoaderDispatch) -> i32 {
    let report = run(loader, dispatch);
    let json =
        serde_json::to_string_pretty(&report).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
    if let Err(e) = std::fs::write(path, json) {
        eprintln!(
            "fish: cannot write the self test report {}: {e}",
            path.display()
        );
        return 1;
    }
    i32::from(!report.ok)
}
