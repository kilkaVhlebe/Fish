fn fish_attack_block(game: &mut Game<'_>, pos: [i32; 3]) -> std::ops::ControlFlow<Result<()>> {
    if crate::game::fish_bridge::events::attack_block(game, pos) {
        std::ops::ControlFlow::Break(Ok(()))
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
