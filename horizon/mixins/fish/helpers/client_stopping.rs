fn fish_client_stopping(game: &mut Game<'_>) {
    crate::game::fish_bridge::events::client_stopping(game);
}
