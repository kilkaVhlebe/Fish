fn fish_local_wake(game: &mut Game<'_>) {
    crate::game::fish_bridge::events::local_wake(game);
}
