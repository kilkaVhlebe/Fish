fn fish_local_respawn(game: &mut Game<'_>) {
    crate::game::fish_bridge::events::local_respawn(game);
}
