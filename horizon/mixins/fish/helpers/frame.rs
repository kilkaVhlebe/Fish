fn fish_frame(game: &mut Game<'_>, elapsed: Duration) {
    crate::game::fish_bridge::api::frame(game, elapsed.as_secs_f32());
}
