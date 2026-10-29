//! Entity events: damage, death, joining, leaving, respawning and sleeping.

use fish_abi::game::ids as game_ids;

use super::{Combine, Event};
use crate::ActionResult;
use crate::game::GameContext;

/// A living entity is about to take damage. `entity_id`, `subject` = entity type, `amount`, `extra` = damage kind. `Fail` cancels.
pub static ALLOW_DAMAGE: Event<GameContext, ActionResult> =
    Event::new(game_ids::ALLOW_DAMAGE, Combine::FirstNonPass);
/// A living entity took damage. `entity_id`, `amount`, `extra`.
pub static AFTER_DAMAGE: Event<GameContext> = Event::new(game_ids::AFTER_DAMAGE, Combine::Notify);
/// A living entity is about to die from damage. `Fail` cancels (approximated, see docs).
pub static ALLOW_DEATH: Event<GameContext, ActionResult> =
    Event::new(game_ids::ALLOW_DEATH, Combine::FirstNonPass);
/// A living entity died. `entity_id`, `subject`, `pos`.
pub static AFTER_DEATH: Event<GameContext> = Event::new(game_ids::AFTER_DEATH, Combine::Notify);

/// Players.
pub mod player {
    use super::*;

    /// A player joined a server. `actor`, `actor_uuid`.
    pub static JOIN: Event<GameContext> = Event::new(game_ids::PLAYER_JOIN, Combine::Notify);
    /// A player left a server. `actor`, `actor_uuid`.
    pub static LEAVE: Event<GameContext> = Event::new(game_ids::PLAYER_LEAVE, Combine::Notify);
    /// A player respawned. `actor`.
    pub static AFTER_RESPAWN: Event<GameContext> =
        Event::new(game_ids::AFTER_RESPAWN, Combine::Notify);
    /// A player's data carried over a respawn. `actor`.
    pub static COPY_FROM: Event<GameContext> = Event::new(game_ids::COPY_FROM, Combine::Notify);
}

/// Sleeping.
pub mod sleep {
    use super::*;

    /// A player tries to sleep in a bed. `pos`, `actor`. `Fail` cancels.
    pub static ALLOW: Event<GameContext, ActionResult> =
        Event::new(game_ids::ALLOW_SLEEPING, Combine::FirstNonPass);
    /// A player fell asleep. `pos`, `actor`.
    pub static START: Event<GameContext> = Event::new(game_ids::SLEEP_START, Combine::Notify);
    /// A player woke up. `actor`.
    pub static STOP: Event<GameContext> = Event::new(game_ids::SLEEP_STOP, Combine::Notify);
}
