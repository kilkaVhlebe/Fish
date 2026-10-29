fn fish_player_join(game: &mut Game<'_>, host: &LanHost, conn: u32) {
    crate::game::fish_bridge::events::player_join(game, host, conn);
}
