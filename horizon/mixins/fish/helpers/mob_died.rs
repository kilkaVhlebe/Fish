fn fish_mob_died(game: &mut Game<'_>, m: &Mob) {
    crate::game::fish_bridge::events::mob_died(game, m.id, &m.kind.id, [m.position.x, m.position.y, m.position.z]);
}
