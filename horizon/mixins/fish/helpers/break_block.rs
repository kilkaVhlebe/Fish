fn fish_break_block(game: &mut Game<'_>, pos: [i32; 3]) -> std::ops::ControlFlow<Result<()>> {
    if crate::game::fish_bridge::events::block_break_before(game, pos) {
        std::ops::ControlFlow::Break(Ok(()))
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
