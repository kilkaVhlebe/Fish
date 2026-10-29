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
    let ok = checks.iter().all(|c| c.ok);
    SelftestReport {
        boot: report,
        ok,
        checks,
    }
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
