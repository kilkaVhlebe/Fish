fn fish_net_left(game: &Game<'_>, reason: &Option<String>) {
    crate::game::fish_bridge::net::client_left(game, reason.as_deref());
}
