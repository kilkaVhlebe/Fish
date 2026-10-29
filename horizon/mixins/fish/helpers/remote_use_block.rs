fn fish_remote_use_block(game: &mut Game<'_>, hand: Hand) -> std::ops::ControlFlow<bool> {
    if crate::game::fish_bridge::events::remote_use_block(game, hand) {
        std::ops::ControlFlow::Break(true)
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
