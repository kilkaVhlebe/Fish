fn fish_chat_receive_local(game: &mut Game<'_>, content: &horizon_client::text::Text, source: horizon_client::chat::ChatSource) -> std::ops::ControlFlow<()> {
    if crate::game::fish_bridge::events::receive(game, content, source) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
