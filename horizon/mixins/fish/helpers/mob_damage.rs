fn fish_mob_damage(m: &Mob, source: &Damage, amount: f32) -> std::ops::ControlFlow<bool> {
    if crate::fish_lib::allow_mob_damage(m.id, &m.kind.id, &source.kind, amount, m.health) {
        std::ops::ControlFlow::Continue(())
    } else {
        std::ops::ControlFlow::Break(false)
    }
}
