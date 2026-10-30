fn fish_use_item(game: &mut Game<'_>) -> std::ops::ControlFlow<Result<Option<Hand>>> {
    if crate::game::fish_bridge::events::use_item(game) {
        return std::ops::ControlFlow::Break(Ok(None));
    }
    match crate::game::fish_bridge::blocks::use_block(game) {
        fish_hooks::Outcome::Handled => std::ops::ControlFlow::Break(Ok(Some(Hand::Main))),
        fish_hooks::Outcome::Cancel => std::ops::ControlFlow::Break(Ok(None)),
        _ => std::ops::ControlFlow::Continue(()),
    }
}
