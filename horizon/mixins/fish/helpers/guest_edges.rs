fn fish_guest_edges(game: &mut Game<'_>, host: &mut LanHost) {
    crate::game::fish_bridge::events::guest_edges(game, &*host);
    crate::game::fish_bridge::net::server_tick(host);
}
