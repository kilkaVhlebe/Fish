//! Interaction events: using and attacking blocks, items and entities, breaking blocks, picking items.
//!
//! All are `FirstNonPass` unless noted: return `ActionResult::Fail` to cancel. `is_client()` is true for the
//! client's prediction of an action the server decides; cancel both or the client will correct itself.

use fish_abi::game::ids as game_ids;

use super::{Combine, Event};
use crate::ActionResult;
use crate::game::GameContext;

/// A player uses a block with a hand. `pos`, `face`, `hand`, `subject` = block id, `item` = held item id. `Fail` cancels.
pub static USE_BLOCK: Event<GameContext, ActionResult> =
    Event::new(game_ids::USE_BLOCK, Combine::FirstNonPass);
/// A player uses the held item in the air. `hand`, `item`. `Fail` cancels.
pub static USE_ITEM: Event<GameContext, ActionResult> =
    Event::new(game_ids::USE_ITEM, Combine::FirstNonPass);
/// A player uses an entity. `entity_id`, `subject` = entity type, `hand`, `item`. `Fail` cancels.
pub static USE_ENTITY: Event<GameContext, ActionResult> =
    Event::new(game_ids::USE_ENTITY, Combine::FirstNonPass);
/// A player starts attacking (mining) a block. `pos`, `face`, `subject` = block id. `Fail` cancels.
pub static ATTACK_BLOCK: Event<GameContext, ActionResult> =
    Event::new(game_ids::ATTACK_BLOCK, Combine::FirstNonPass);
/// A player attacks an entity. `entity_id`, `subject` = entity type. `Fail` cancels.
pub static ATTACK_ENTITY: Event<GameContext, ActionResult> =
    Event::new(game_ids::ATTACK_ENTITY, Combine::FirstNonPass);
/// A player is about to break a block. `pos`, `subject` = block id. `Fail` cancels and fires block_break_canceled.
pub static BLOCK_BREAK_BEFORE: Event<GameContext, ActionResult> =
    Event::new(game_ids::BLOCK_BREAK_BEFORE, Combine::FirstNonPass);
/// A player broke a block (observed on the next tick). `pos`, `subject` = the block that was there.
pub static BLOCK_BREAK_AFTER: Event<GameContext> =
    Event::new(game_ids::BLOCK_BREAK_AFTER, Combine::Notify);
/// A block break was cancelled by a listener. `pos`, `subject`.
pub static BLOCK_BREAK_CANCELED: Event<GameContext> =
    Event::new(game_ids::BLOCK_BREAK_CANCELED, Combine::Notify);
/// A player picks an item (middle click). `item`. `Fail` cancels.
pub static PICK_ITEM: Event<GameContext, ActionResult> =
    Event::new(game_ids::PICK_ITEM, Combine::FirstNonPass);
