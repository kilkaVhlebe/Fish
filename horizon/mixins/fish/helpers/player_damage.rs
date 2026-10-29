fn fish_player_damage(motion: &PlayerMotion, resources: &HealthResources, source: DamageTypeId, amount: f32) -> std::ops::ControlFlow<Result<BlockOutcome>> {
    if !crate::fish_lib::wants_damage() {
        return std::ops::ControlFlow::Continue(());
    }
    let name = resources
        .damage
        .source::<()>(source, None, None, None)
        .map(|s| s.definition().name.to_string())
        .unwrap_or_default();
    if crate::fish_lib::allow_player_damage(&name, amount, motion.health.health() + motion.health.absorption()) {
        std::ops::ControlFlow::Continue(())
    } else {
        std::ops::ControlFlow::Break(Ok(BlockOutcome::default()))
    }
}
