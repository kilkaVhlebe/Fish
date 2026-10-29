fn fish_system_message(content: &Tag) -> std::ops::ControlFlow<()> {
    if !crate::fish_lib::wants_game() {
        return std::ops::ControlFlow::Continue(());
    }
    let text = horizon_protocol::text::plain(content, &Default::default());
    if crate::fish_lib::allow_game_message(&text) {
        std::ops::ControlFlow::Continue(())
    } else {
        std::ops::ControlFlow::Break(())
    }
}
