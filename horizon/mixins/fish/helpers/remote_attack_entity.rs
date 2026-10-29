fn fish_remote_attack_entity(game: &mut Game<'_>) -> std::ops::ControlFlow<bool> {
    if crate::game::fish_bridge::events::remote_attack_entity(game) {
        std::ops::ControlFlow::Break(true)
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
