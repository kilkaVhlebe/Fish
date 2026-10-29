//! Life-cycle events (Fabric `fabric-lifecycle-events-v1`).
//!
//! Tick events are routed by the role of the game: the window's game fires `client`, an integrated
//! or dedicated server fires `server`. The other events carry a [`GameContext`]; the documentation
//! of each says which of its fields is filled.

#![allow(unused_imports)]

use fish_abi::client::ids;
use fish_abi::game::ids as game_ids;

use super::{Combine, Event};
use crate::client::TickContext;
use crate::game::GameContext;

/// Client-side events.
pub mod client {
    use super::*;

    /// Start of every client tick.
    pub static START_TICK: Event<TickContext> = Event::new(ids::CLIENT_START_TICK, Combine::Notify);
    /// End of every client tick.
    pub static END_TICK: Event<TickContext> = Event::new(ids::CLIENT_END_TICK, Combine::Notify);
    /// The window's game ran its first frame.
    pub static STARTED: Event<GameContext> = Event::new(game_ids::CLIENT_STARTED, Combine::Notify);
    /// The window is closing; the world is not saved yet.
    pub static STOPPING: Event<GameContext> =
        Event::new(game_ids::CLIENT_STOPPING, Combine::Notify);
    /// The window closed and its world is saved. Detached: no game view.
    pub static STOPPED: Event<GameContext> = Event::new(game_ids::CLIENT_STOPPED, Combine::Notify);
}

/// Server-side events (integrated and dedicated servers).
pub mod server {
    use super::*;

    /// Start of every server tick.
    pub static START_TICK: Event<TickContext> = Event::new(ids::SERVER_START_TICK, Combine::Notify);
    /// End of every server tick.
    pub static END_TICK: Event<TickContext> = Event::new(ids::SERVER_END_TICK, Combine::Notify);
    /// A server (integrated, dedicated or in-frame) starts loading its world. `subject` = world name.
    pub static STARTING: Event<GameContext> =
        Event::new(game_ids::SERVER_STARTING, Combine::Notify);
    /// A server ran its first tick.
    pub static STARTED: Event<GameContext> = Event::new(game_ids::SERVER_STARTED, Combine::Notify);
    /// A server begins to stop; players are not saved yet.
    pub static STOPPING: Event<GameContext> =
        Event::new(game_ids::SERVER_STOPPING, Combine::Notify);
    /// A server finished saving and closed.
    pub static STOPPED: Event<GameContext> = Event::new(game_ids::SERVER_STOPPED, Combine::Notify);
}

/// Worlds (levels, one per dimension).
pub mod world {
    use super::*;

    /// A level (dimension) became loaded. `dimension` = its id.
    pub static LOAD: Event<GameContext> = Event::new(game_ids::WORLD_LOAD, Combine::Notify);
    /// A level is about to be unloaded. `dimension` = its id.
    pub static UNLOAD: Event<GameContext> = Event::new(game_ids::WORLD_UNLOAD, Combine::Notify);
}

/// Chunks. Fired without a game view: `client()` fails; use `pos()` (`x`, `z` are chunk coordinates).
pub mod chunk {
    use super::*;

    /// A chunk entered the loaded set. `pos` = [chunk x, 0, chunk z]; detached.
    pub static LOAD: Event<GameContext> = Event::new(game_ids::CHUNK_LOAD, Combine::Notify);
    /// A chunk is about to leave the loaded set. `pos` = [chunk x, 0, chunk z]; detached.
    pub static UNLOAD: Event<GameContext> = Event::new(game_ids::CHUNK_UNLOAD, Combine::Notify);
}

/// Data reloads (`/reload`).
pub mod reload {
    use super::*;

    /// Data (datapacks/functions) reload starts.
    pub static START: Event<GameContext> = Event::new(game_ids::RELOAD_START, Combine::Notify);
    /// Data reload finished (observed on the next server tick).
    pub static END: Event<GameContext> = Event::new(game_ids::RELOAD_END, Combine::Notify);
}
