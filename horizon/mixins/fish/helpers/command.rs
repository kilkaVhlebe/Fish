fn fish_command(game: &mut Game<'_>, command: &str, ran: &mut bool) {
    if crate::game::fish_bridge::api::command(game, command) {
        *ran = true;
    }
}
