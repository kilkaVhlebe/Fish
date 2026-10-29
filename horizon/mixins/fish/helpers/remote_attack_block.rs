fn fish_remote_attack_block(game: &mut Game<'_>, start: bool) -> std::ops::ControlFlow<()> {
    if crate::game::fish_bridge::events::remote_attack_block(game, start) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
