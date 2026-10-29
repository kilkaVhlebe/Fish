fn fish_chat_send(game: &mut Game<'_>, message: &str) -> std::ops::ControlFlow<()> {
    match crate::game::fish_bridge::events::typed(game, message, true) {
        crate::game::fish_bridge::events::Typed::Send => std::ops::ControlFlow::Continue(()),
        crate::game::fish_bridge::events::Typed::Cancel => std::ops::ControlFlow::Break(()),
        crate::game::fish_bridge::events::Typed::Replace(text) => {
            let _guard = crate::game::fish_bridge::events::Reentry::enter();
            game.handle_chat_input(&text);
            std::ops::ControlFlow::Break(())
        }
    }
}
