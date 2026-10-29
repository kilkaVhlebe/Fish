//! Chat, game messages and commands, on the server and in the client.

use fish_abi::game::ids as game_ids;

use super::{Combine, Event};
use crate::ActionResult;
use crate::game::GameContext;

/// Server-side messages.
pub mod server {
    use super::*;

    /// A player's chat message reached a server. `actor`, `subject` = message. `Fail` drops it.
    pub static ALLOW_CHAT: Event<GameContext, ActionResult> =
        Event::new(game_ids::ALLOW_CHAT, Combine::FirstNonPass);
    /// A player's chat message was broadcast. `actor`, `subject`.
    pub static CHAT: Event<GameContext> = Event::new(game_ids::CHAT, Combine::Notify);
    /// A server sends a game (system) message. `subject` = text. `Fail` drops it.
    pub static ALLOW_GAME: Event<GameContext, ActionResult> =
        Event::new(game_ids::ALLOW_GAME, Combine::FirstNonPass);
    /// A server sent a game message. `subject`.
    pub static GAME: Event<GameContext> = Event::new(game_ids::GAME, Combine::Notify);
    /// A server is about to run a player's command. `actor`, `subject` = command. `Fail` blocks it.
    pub static ALLOW_COMMAND: Event<GameContext, ActionResult> =
        Event::new(game_ids::ALLOW_COMMAND, Combine::FirstNonPass);
    /// A server ran a player's command. `actor`, `subject`.
    pub static COMMAND: Event<GameContext> = Event::new(game_ids::COMMAND, Combine::Notify);
}

/// Client-side messages.
pub mod client {
    use super::*;

    /// The client received a chat line. `subject` = plain text, `extra` = source (player/server/client). `Fail` hides it.
    pub static ALLOW_RECEIVE: Event<GameContext, ActionResult> =
        Event::new(game_ids::ALLOW_RECEIVE, Combine::FirstNonPass);
    /// The client showed a received chat line. `subject`, `extra`.
    pub static RECEIVE: Event<GameContext> = Event::new(game_ids::RECEIVE, Combine::Notify);
    /// The player typed a chat line. `subject`. `Fail` cancels.
    pub static ALLOW_SEND: Event<GameContext, ActionResult> =
        Event::new(game_ids::ALLOW_SEND, Combine::FirstNonPass);
    /// The player's chat line may be rewritten with `replace_text`. `subject`.
    pub static MODIFY_SEND: Event<GameContext> = Event::new(game_ids::MODIFY_SEND, Combine::Notify);
    /// The player's chat line was sent. `subject`.
    pub static SEND: Event<GameContext> = Event::new(game_ids::SEND, Combine::Notify);
}
