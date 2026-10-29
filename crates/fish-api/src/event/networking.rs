//! Connection events (Fabric `ClientPlayConnectionEvents`, `ServerPlayConnectionEvents`, ...).

use fish_abi::game::ids as game_ids;

use super::{Combine, Event};
use crate::game::GameContext;

/// The client's connection to a server (the integrated one included).
pub mod client {
    use super::*;

    /// The connection starts configuring, before the world loads.
    pub static INIT: Event<GameContext> = Event::new(game_ids::NET_CLIENT_INIT, Combine::Notify);
    /// The connection entered the play phase.
    pub static JOIN: Event<GameContext> = Event::new(game_ids::NET_CLIENT_JOIN, Combine::Notify);
    /// The connection closed; `subject()` is the reason when known.
    pub static DISCONNECT: Event<GameContext> =
        Event::new(game_ids::NET_CLIENT_DISCONNECT, Combine::Notify);
}

/// Players' connections, on a server.
pub mod server {
    use super::*;

    /// A player's connection starts configuring; `actor()`, `actor_uuid()`.
    pub static INIT: Event<GameContext> = Event::new(game_ids::NET_SERVER_INIT, Combine::Notify);
    /// A player's connection entered the play phase.
    pub static JOIN: Event<GameContext> = Event::new(game_ids::NET_SERVER_JOIN, Combine::Notify);
    /// A player's connection closed; `subject()` is the reason when known.
    pub static DISCONNECT: Event<GameContext> =
        Event::new(game_ids::NET_SERVER_DISCONNECT, Combine::Notify);
}
