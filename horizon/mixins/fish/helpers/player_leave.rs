fn fish_player_leave(game: &mut Game<'_>, g: &Guest) {
    crate::game::fish_bridge::net::server_left(game, g.conn, &g.name, g.uuid, &g.left_reason);
    crate::game::fish_bridge::events::player_leave(game, g.conn, &g.name, g.uuid);
}
