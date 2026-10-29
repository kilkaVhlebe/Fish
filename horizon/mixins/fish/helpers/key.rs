fn fish_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    crate::game::fish_bridge::api::key(game, key, pressed);
}
