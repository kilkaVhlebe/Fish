fn fish_hud(game: &mut Game<'_>, batch: &mut HudBatch) {
    crate::game::fish_bridge::api::hud(game, batch);
}
