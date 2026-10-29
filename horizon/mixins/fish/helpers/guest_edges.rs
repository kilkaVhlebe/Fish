fn fish_guest_edges(game: &mut Game<'_>, host: &LanHost) {
    crate::game::fish_bridge::events::guest_edges(game, host);
}
