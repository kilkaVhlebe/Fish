fn fish_use_item(game: &mut Game<'_>) -> std::ops::ControlFlow<Result<Option<Hand>>> {
    if crate::game::fish_bridge::events::use_item(game) {
        return std::ops::ControlFlow::Break(Ok(None));
    }
    match crate::game::fish_bridge::blocks::use_block(game) {
        fish_hooks::Outcome::Handled => {
            return std::ops::ControlFlow::Break(Ok(Some(Hand::Main)));
        }
        fish_hooks::Outcome::Cancel => return std::ops::ControlFlow::Break(Ok(None)),
        _ => {}
    }
    match crate::game::fish_bridge::items::use_item(game) {
        (fish_hooks::Outcome::Handled, hand) => std::ops::ControlFlow::Break(Ok(Some(hand))),
        (fish_hooks::Outcome::Cancel, _) => std::ops::ControlFlow::Break(Ok(None)),
        _ => std::ops::ControlFlow::Continue(()),
    }
}
