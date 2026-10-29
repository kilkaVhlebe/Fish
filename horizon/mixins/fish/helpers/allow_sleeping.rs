fn fish_allow_sleeping(game: &mut Game<'_>, pos: [i32; 3]) -> std::ops::ControlFlow<()> {
    if crate::game::fish_bridge::events::allow_sleeping(game, pos) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
