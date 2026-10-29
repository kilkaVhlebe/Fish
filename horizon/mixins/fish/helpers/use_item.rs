fn fish_use_item(game: &mut Game<'_>) -> std::ops::ControlFlow<Result<Option<Hand>>> {
    if crate::game::fish_bridge::events::use_item(game) {
        std::ops::ControlFlow::Break(Ok(None))
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
