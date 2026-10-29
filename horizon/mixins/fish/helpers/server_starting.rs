fn fish_server_starting(game: &mut Game<'_>, request: &world_loading::WorldRequest) {
    let name = request.name.as_deref().unwrap_or(&request.folder);
    crate::game::fish_bridge::events::server_starting(game, name);
}
