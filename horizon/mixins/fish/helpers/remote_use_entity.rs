fn fish_remote_use_entity(game: &mut Game<'_>, hand: Hand) -> std::ops::ControlFlow<bool> {
    if crate::game::fish_bridge::events::remote_use_entity(game, hand) {
        std::ops::ControlFlow::Break(true)
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
