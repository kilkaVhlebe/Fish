fn fish_world_load(game: &Game<'_>, dim: Dim) {
    crate::game::fish_bridge::events::level_opened(game, dim);
}
