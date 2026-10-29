// What the hook helpers call.

use fish_hooks::NativeModRow;
use horizon_client::hud::HudBatch;

use super::views::{DrawView, GameView, SnapshotView};
use crate::game::Game;
use crate::game::native_hooks::NativeMod;

pub(in crate::game) fn tick_start(game: &mut Game<'_>) {
    super::net::client_tick(game);
    super::events::note_context(game);
    super::events::note_actor(game);
    super::events::local_sleep_edge(game);
    if let Some(d) = fish_hooks::get() {
        d.tick_start(&mut GameView { game });
    }
}

thread_local! {
    static TICKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    // The failed mods the Options > Mods rows were last built for.
    static FAILED: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

pub(in crate::game) fn tick_end(game: &mut Game<'_>) {
    if let Some(d) = fish_hooks::get() {
        d.tick_end(&mut GameView { game });
        // The crash report lists the mods and the ones that failed since: refreshed now and then.
        if TICKS.with(|t| {
            t.set(t.get().wrapping_add(1));
            t.get() % 100 == 1
        }) {
            let failed = horizon_client::fish_lib::refresh_crash_details();
            // A mod stopped: its Options > Mods row says so.
            if FAILED.with(|f| f.replace(failed.clone()) != failed) && game.native_window() {
                game.list_native_mods();
            }
        }
    }
}

pub(in crate::game) fn frame(game: &mut Game<'_>, elapsed_secs: f32) {
    if let Some(d) = fish_hooks::get() {
        d.frame(&mut GameView { game }, elapsed_secs);
    }
}

pub(in crate::game) fn hud(game: &mut Game<'_>, batch: &mut HudBatch) {
    let Some(d) = fish_hooks::get() else { return };
    let mut view = SnapshotView::of(game);
    {
        let mut draw = DrawView { game: &*game, batch };
        d.hud(&mut view, &mut draw);
    }
    for line in view.chat {
        game.native_chat(&line);
    }
}

pub(in crate::game) fn command(game: &mut Game<'_>, command: &str) -> bool {
    match fish_hooks::get() {
        Some(d) => d.client_command(&mut GameView { game }, command),
        None => false,
    }
}

pub(in crate::game) fn key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if let Some(d) = fish_hooks::get() {
        d.key(&mut GameView { game }, key, pressed);
    }
}

pub(in crate::game) fn list_mods(mods: &mut Vec<NativeMod>) {
    let Some(d) = fish_hooks::get() else { return };
    let mut rows: Vec<NativeModRow> = Vec::new();
    d.list_mods(&mut rows);
    mods.extend(rows.into_iter().map(|r| NativeMod {
        id: r.id,
        title: r.title,
        description: r.description,
        keys: r.keys,
    }));
}

pub(in crate::game) fn settings_buttons(mod_id: &str, buttons: &mut Vec<String>) {
    if let Some(d) = fish_hooks::get() {
        d.settings_buttons(mod_id, buttons);
    }
}

pub(in crate::game) fn settings_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if let Some(d) = fish_hooks::get() {
        d.settings_click(&mut GameView { game }, mod_id, button);
    }
}
