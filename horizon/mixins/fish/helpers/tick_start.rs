fn fish_tick_start(game: &mut Game<'_>) {
    crate::game::fish_bridge::api::tick_start(game);
}
