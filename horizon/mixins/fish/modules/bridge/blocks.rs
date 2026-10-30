// Fish blocks in the game: the content folders Horizon reads as content mods, the state ids it gave
// the blocks, and the events of the blocks (use, place, broken). The block simulation's own events
// (neighbour changes, ticks) come from the patch 0007 in horizon-world.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use fish_hooks::{BlockEvent, BlockEventKind, Dispatch, Outcome};
use horizon_client::first_person::Hand;
use horizon_world::registry::BlockStateId;

use super::events::{actor, face_index, held_item};
use super::views::{GameView, role_of};
use crate::game::{Game, GuestUse};

// Set once the loader was told which states Horizon gave the blocks of content mods.
static BUILT: AtomicBool = AtomicBool::new(false);

thread_local! {
    // Breaks of Fish blocks that await their `broken` event at the tick's end: the position, the
    // state that was there, and who broke it.
    static BREAKS: RefCell<Vec<Break>> = const { RefCell::new(Vec::new()) };
}

struct Break {
    pos: [i32; 3],
    state: u32,
    actor: String,
    uuid: [u8; 16],
}

/// Patch 0006 (`content_root`): the folders of the content mods Fish wrote for the blocks mods
/// registered. They are mods that are always on.
pub(in crate::game) fn content_folders() -> Vec<std::path::PathBuf> {
    fish_hooks::get().map(|d| d.block_content()).unwrap_or_default()
}

/// The first tick of a game: the resources are loaded, so the block states of content mods exist.
/// Tells the loader which states belong to which block (once per process).
pub(in crate::game) fn built(game: &Game<'_>) {
    if BUILT.load(Ordering::Acquire) {
        return;
    }
    let Some(d) = fish_hooks::get() else { return };
    let registry = game.pack.assets.registry;
    let states = (0..registry.len() as u32)
        .filter_map(|i| registry.state(BlockStateId(i)).map(|s| (i, s.name.as_str())));
    d.blocks_built(&fish_hooks::block_runs(states));
    BUILT.store(true, Ordering::Release);
}

fn event<'a>(game: &Game<'_>, kind: BlockEventKind, state: u32, pos: [i32; 3]) -> BlockEvent<'a> {
    let mut e = BlockEvent::new(kind, state, pos);
    e.role = role_of(game);
    e.dimension = game.dim.id();
    e
}

// The state at `pos` when it is one of a Fish block's.
fn fish_state(game: &Game<'_>, d: &dyn Dispatch, pos: [i32; 3]) -> Option<u32> {
    let state = game.world.block(pos[0], pos[1], pos[2]).0;
    d.is_block_state(state).then_some(state)
}

/// `use_item`, after the generic use events: a click on a block a mod registered. As in
/// `Block.useWithoutItem` only the main hand's turn counts, and not while sneaking with something
/// in a hand. `Handled` or `Cancel` end the click.
pub(in crate::game) fn use_block(game: &mut Game<'_>) -> Outcome {
    let Some(d) = fish_hooks::get() else {
        return Outcome::Pass;
    };
    let hand = match game.guest_use {
        Some(GuestUse::OnBlock(hand)) => hand,
        None if !game.entity_targeted() => Hand::Main,
        _ => return Outcome::Pass,
    };
    let sneaking = game.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
    let hands_full = game.stack_in(game.selected).is_some() || game.offhand.is_some();
    if hand != Hand::Main || (sneaking && hands_full) {
        return Outcome::Pass;
    }
    let Some(hit) = game.pick() else {
        return Outcome::Pass;
    };
    let Some(state) = fish_state(game, d, hit.pos) else {
        return Outcome::Pass;
    };
    let who = actor(game);
    let item = held_item(game, hand);
    let mut e = event(game, BlockEventKind::Use, state, hit.pos);
    e.face = face_index(hit.face);
    e.actor = &who.name;
    e.actor_uuid = who.uuid;
    e.item = &item;
    let out = d.block_event(Some(&mut GameView { game: &mut *game }), &e);
    if out == Outcome::Handled {
        game.swing_arm();
    }
    out
}

/// `apply_item_block_state`: an item (a player's or a dispenser's) placed `state` at `pos`.
pub(in crate::game) fn placed(game: &mut Game<'_>, pos: [i32; 3], state: u32, item: &str) {
    let Some(d) = fish_hooks::get().filter(|d| d.is_block_state(state)) else {
        return;
    };
    let who = actor(game);
    let mut e = event(game, BlockEventKind::Place, state, pos);
    e.actor = &who.name;
    e.actor_uuid = who.uuid;
    e.item = item;
    d.block_event(Some(&mut GameView { game: &mut *game }), &e);
}

/// `break_block_as_player`, when no listener vetoed the break: a Fish block that is about to be
/// broken reports it at the tick's end (`flush`), once the game removed it.
pub(in crate::game) fn break_before(game: &Game<'_>, pos: [i32; 3]) {
    let Some(d) = fish_hooks::get() else { return };
    let Some(state) = fish_state(game, d, pos) else {
        return;
    };
    let who = actor(game);
    BREAKS.with(|b| {
        b.borrow_mut().push(Break {
            pos,
            state,
            actor: who.name,
            uuid: who.uuid,
        });
    });
}

/// The end of a tick: the Fish blocks players broke report it. A block still standing was not
/// broken (the game refused), so it reports nothing.
pub(in crate::game) fn flush(game: &mut Game<'_>) {
    let pending = BREAKS.with(|b| std::mem::take(&mut *b.borrow_mut()));
    let Some(d) = fish_hooks::get().filter(|_| !pending.is_empty()) else {
        return;
    };
    for b in pending {
        if game.world.block(b.pos[0], b.pos[1], b.pos[2]).0 == b.state {
            continue;
        }
        let mut e = event(game, BlockEventKind::Broken, b.state, b.pos);
        e.actor = &b.actor;
        e.actor_uuid = b.uuid;
        d.block_event(Some(&mut GameView { game: &mut *game }), &e);
    }
}
