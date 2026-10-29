fn fish_chat_sent(game: &mut Game<'_>, message: &str) -> std::ops::ControlFlow<()> {
    match crate::game::fish_bridge::events::typed(game, message, false) {
        crate::game::fish_bridge::events::Typed::Send => std::ops::ControlFlow::Continue(()),
        crate::game::fish_bridge::events::Typed::Cancel => std::ops::ControlFlow::Break(()),
        crate::game::fish_bridge::events::Typed::Replace(text) => {
            let _guard = crate::game::fish_bridge::events::Reentry::enter();
            game.send_chat(&text);
            std::ops::ControlFlow::Break(())
        }
    }
}
