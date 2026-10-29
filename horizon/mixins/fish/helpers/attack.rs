fn fish_attack(game: &mut Game<'_>) -> std::ops::ControlFlow<bool> {
    if crate::game::fish_bridge::events::attack(game) {
        std::ops::ControlFlow::Break(false)
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
