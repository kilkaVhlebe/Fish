fn fish_settings_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    crate::game::fish_bridge::api::settings_click(game, mod_id, button);
}
