// Fish items in the game: the use events (in the air, on a block, on an entity), finishing a
// consumable, the inventory tick and the tooltip lines. What the items are comes from the loader
// (`is_item`); the content declarations are written next to the blocks' (see `blocks.rs`).

use fish_hooks::{Dispatch, ItemEvent, ItemEventKind, Outcome};
use horizon_client::first_person::Hand;
use horizon_client::items::ItemStack;
use horizon_client::text::Text;

use super::events::{actor, face_index};
use super::views::{GameView, role_of};
use crate::game::{Game, GuestUse};

// What a click is aimed at, as `use_item_as_player` sorts it.
struct Aim {
    entity: Option<(i64, String)>,
    block: Option<([i32; 3], u8)>,
}

// An event of `kind` for a stack in the acting hand, with what every item event carries.
fn event<'a>(
    game: &Game<'_>,
    kind: ItemEventKind,
    item: &'a str,
    count: u32,
    who: &'a super::events::Actor,
) -> ItemEvent<'a> {
    let mut e = ItemEvent::new(kind, item, count);
    e.role = role_of(game);
    e.dimension = game.dim.id();
    e.actor = &who.name;
    e.actor_uuid = who.uuid;
    e
}

// One hand's turn (`use_hand`): the entity or the block aimed at first, then the item's own use.
// The first result that is not a pass ends the turn.
fn hand_turn(game: &mut Game<'_>, d: &dyn Dispatch, hand: Hand, aim: &Aim) -> Outcome {
    let Some((item, count)) = game.held().map(|s| (s.item.clone(), s.count)) else {
        return Outcome::Pass;
    };
    if !d.is_item(&item) {
        return Outcome::Pass;
    }
    let who = actor(game);
    let hand_index = u8::from(hand == Hand::Off);
    let mut out = Outcome::Pass;
    if let Some((id, ty)) = &aim.entity {
        let mut e = event(game, ItemEventKind::UseOnEntity, &item, count, &who);
        e.hand = hand_index;
        e.entity_id = *id;
        e.target = ty;
        out = d.item_event(Some(&mut GameView { game: &mut *game }), &e);
    } else if let Some((pos, face)) = aim.block {
        let target = game.block_name(pos).to_string();
        let mut e = event(game, ItemEventKind::UseOnBlock, &item, count, &who);
        e.hand = hand_index;
        e.pos = pos;
        e.face = face;
        e.target = &target;
        out = d.item_event(Some(&mut GameView { game: &mut *game }), &e);
    }
    if out == Outcome::Pass {
        let mut e = event(game, ItemEventKind::Use, &item, count, &who);
        e.hand = hand_index;
        out = d.item_event(Some(&mut GameView { game: &mut *game }), &e);
    }
    if out == Outcome::Handled {
        game.swing_arm();
    }
    out
}

/// `use_item`, after the generic use events and the blocks': the hands in turn (the guest's hand
/// alone for a guest), each with the item it holds. `Handled` or `Cancel` end the click, with the
/// hand that acted.
pub(in crate::game) fn use_item(game: &mut Game<'_>) -> (Outcome, Hand) {
    let none = (Outcome::Pass, Hand::Main);
    let Some(d) = fish_hooks::get() else {
        return none;
    };
    let uses = ItemEventKind::Use.bit() | ItemEventKind::UseOnBlock.bit() | ItemEventKind::UseOnEntity.bit();
    if d.item_events() & uses == 0 {
        return none;
    }
    let guest = game.guest_use;
    let hands: Vec<Hand> = match guest {
        Some(GuestUse::OnBlock(hand) | GuestUse::Item(hand) | GuestUse::Entity(hand)) => vec![hand],
        None => Hand::BOTH.to_vec(),
    };
    let entity_aimed = match guest {
        Some(GuestUse::Entity(_)) => true,
        Some(_) => false,
        None => game.entity_targeted(),
    };
    let mut aim = Aim {
        entity: None,
        block: None,
    };
    if entity_aimed {
        aim.entity = game
            .mob_hit()
            .and_then(|(i, _)| game.mobs.get(i))
            .map(|m| (i64::from(m.id), m.kind.id.clone()));
    } else if !matches!(guest, Some(GuestUse::Item(_))) {
        aim.block = game.pick().map(|h| (h.pos, face_index(h.face)));
    }
    for hand in hands {
        let out = game.with_hand(hand, |g| hand_turn(g, d, hand, &aim));
        if out != Outcome::Pass {
            return (out, hand);
        }
    }
    none
}

/// `finish_consuming`: the player ate or drank `stack`, in the acting hand.
pub(in crate::game) fn finished(game: &mut Game<'_>, stack: &ItemStack) {
    let Some(d) = fish_hooks::get() else { return };
    if d.item_events() & ItemEventKind::FinishUsing.bit() == 0 || !d.is_item(&stack.item) {
        return;
    }
    let who = actor(game);
    let mut e = event(game, ItemEventKind::FinishUsing, &stack.item, stack.count, &who);
    e.hand = u8::from(game.hand == Hand::Off);
    d.item_event(Some(&mut GameView { game: &mut *game }), &e);
}

/// The start of a tick: every Fish stack the player carries (inventory, armor, off hand) reports
/// that it sits there.
pub(in crate::game) fn tick(game: &mut Game<'_>) {
    let Some(d) = fish_hooks::get() else { return };
    if d.item_events() & ItemEventKind::InventoryTick.bit() == 0 || game.player.is_none() {
        return;
    }
    // (item, count, slot, selected) of the Fish stacks, collected before the events borrow the game.
    let mut stacks: Vec<(std::sync::Arc<str>, u32, i32, bool)> = Vec::new();
    let mut note = |stack: &Option<ItemStack>, slot: i32, selected: bool| {
        if let Some(s) = stack
            && d.is_item(&s.item)
        {
            stacks.push((s.item.clone(), s.count, slot, selected));
        }
    };
    // Slots as in `fish_abi::items::slot`: 0..36 inventory, 36..40 armor, 40 the off hand.
    for (i, s) in game.inventory.iter().enumerate().take(36) {
        note(s, i as i32, i == game.selected);
    }
    for (i, s) in game.armor.iter().enumerate() {
        note(s, 36 + i as i32, false);
    }
    note(&game.offhand, 40, true);
    if stacks.is_empty() {
        return;
    }
    let who = actor(game);
    for (item, count, slot, selected) in stacks {
        let mut e = event(game, ItemEventKind::InventoryTick, &item, count, &who);
        e.slot = slot;
        e.selected = selected;
        d.item_event(Some(&mut GameView { game: &mut *game }), &e);
    }
}

/// `stack_tooltip`: the lines mods add under a Fish item's tooltip, gray like the details.
pub(in crate::game) fn tooltip(stack: &ItemStack, mut lines: Vec<(Text, u32)>) -> Vec<(Text, u32)> {
    let Some(d) = fish_hooks::get() else {
        return lines;
    };
    if d.item_events() & ItemEventKind::AppendTooltip.bit() == 0 || !d.is_item(&stack.item) {
        return lines;
    }
    let mut extra = Vec::new();
    d.item_tooltip(&stack.item, stack.count, &mut extra);
    let gray = horizon_client::hud::rgba(170, 170, 170, 255);
    lines.extend(extra.iter().map(|l| (Text::legacy(l), gray)));
    lines
}
