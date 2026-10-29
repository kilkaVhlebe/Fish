fn fish_reload_start(game: &mut Game<'_>, run_load: bool) {
    crate::game::fish_bridge::events::reload_start(game, run_load);
}
