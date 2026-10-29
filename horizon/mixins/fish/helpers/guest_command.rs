fn fish_guest_command(game: &mut Game<'_>, host: &LanHost, conn: u32, command: &str) {
    crate::game::fish_bridge::events::command_ran(game, host, conn, command);
}
