fn fish_server_stopped(game: &mut Game<'_>) {
    crate::game::fish_bridge::events::server_stopped(game);
}
