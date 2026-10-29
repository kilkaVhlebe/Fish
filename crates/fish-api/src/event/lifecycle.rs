//! Life-cycle events (Fabric `fabric-lifecycle-events-v1`).
//!
//! Tick events are routed by the role of the game: the window's game fires `client`, an integrated
//! or dedicated server fires `server`.

use fish_abi::client::ids;

use super::{Combine, Event};
use crate::client::TickContext;

/// Client-side ticks.
pub mod client {
    use super::*;

    /// Start of every client tick.
    pub static START_TICK: Event<TickContext> = Event::new(ids::CLIENT_START_TICK, Combine::Notify);
    /// End of every client tick.
    pub static END_TICK: Event<TickContext> = Event::new(ids::CLIENT_END_TICK, Combine::Notify);
}

/// Server-side ticks (integrated and dedicated servers).
pub mod server {
    use super::*;

    /// Start of every server tick.
    pub static START_TICK: Event<TickContext> = Event::new(ids::SERVER_START_TICK, Combine::Notify);
    /// End of every server tick.
    pub static END_TICK: Event<TickContext> = Event::new(ids::SERVER_END_TICK, Combine::Notify);
}
