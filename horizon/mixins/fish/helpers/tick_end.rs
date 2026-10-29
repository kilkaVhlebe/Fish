fn fish_tick_end(game: &mut Game<'_>) {
    crate::game::fish_bridge::api::tick_end(game);
}
