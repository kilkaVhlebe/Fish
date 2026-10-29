fn fish_server_stopping(game: &mut Game<'_>) {
    crate::game::fish_bridge::events::server_stopping(game);
}
