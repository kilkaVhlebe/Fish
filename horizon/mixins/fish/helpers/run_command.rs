fn fish_run_command(game: &mut Game<'_>, command: &str) -> std::ops::ControlFlow<bool> {
    match crate::game::fish_bridge::events::run_command(game, command) {
        Some(ok) => std::ops::ControlFlow::Break(ok),
        None => std::ops::ControlFlow::Continue(()),
    }
}
