//! The `fish:item` and `fish:item_group` services (phase P8): items and creative tabs registered by
//! mods.
//!
//! An item is declared by data, like a block: an identifier (`namespace:path`, the namespace is
//! the registering mod's id), a vanilla *template* item whose components it copies, and the few
//! settings that differ from the template (Horizon's item declarations know a stack size and a
//! rarity). The loader writes the items of every mod into the same content overlay as its blocks
//! (`data/<namespace>/horizon/item/<path>.json`), so an item gets its model, texture and
//! translation from the `.fish` archive of the mod.
//!
//! A creative tab ([`ItemGroupServiceV1::register`]) lists items by id and may be added to by
//! other mods ([`ItemGroupServiceV1::modify_entries`]); the loader applies both to the game's
//! creative inventory.
//!
//! Registration is allowed while the `pre_launch` and `main` entrypoints run (the same freeze rule
//! as the registries).

use core::ffi::c_void;

use crate::tables::{DropUserFn, FishServiceHeader};
use crate::types::{FishHandle, FishStatus, FishStr};

/// Service id of items.
pub const SERVICE_ITEM: &str = "fish:item";
/// Service id of creative tabs.
pub const SERVICE_ITEM_GROUP: &str = "fish:item_group";

/// [`ItemSettingsV1::set`] bits: the settings that differ from the template.
pub mod set {
    /// [`ItemSettingsV1::max_stack`](super::ItemSettingsV1::max_stack) is set.
    pub const MAX_STACK: u32 = 1;
    /// [`ItemSettingsV1::rarity`](super::ItemSettingsV1::rarity) is set.
    pub const RARITY: u32 = 1 << 1;
    /// Every bit above.
    pub const ALL: u32 = (1 << 2) - 1;
}

/// [`ItemSettingsV1::rarity`] values.
pub mod rarity {
    /// White name.
    pub const COMMON: u8 = 0;
    /// Yellow name.
    pub const UNCOMMON: u8 = 1;
    /// Aqua name.
    pub const RARE: u8 = 2;
    /// Light purple name.
    pub const EPIC: u8 = 3;
    /// The names of the rarities, by value (the words Horizon's item files use).
    pub const NAMES: [&str; 4] = ["common", "uncommon", "rare", "epic"];
}

/// [`ItemPayloadV1::event`] values, and the bit (`1 << value`) a mod sets in the `events` mask of
/// [`ItemServiceV1::register`] for each event its handler wants.
pub mod event {
    /// The item was used (`Item.use`: a right click that is not on a block or an entity, or one
    /// the block and the entity left to the item). The handler's result decides: `Pass` leaves the
    /// click to the game, `Fail` cancels it, anything else ends it.
    pub const USE: u8 = 0;
    /// The item was used on a block (`Item.useOn`; the block's own use came first). `pos` is the
    /// block, `face` the face clicked and `target` the block's id. Decides like `USE`.
    pub const USE_ON_BLOCK: u8 = 1;
    /// The item was used on an entity (`Item.interactLivingEntity`). `entity_id` and `target` (its
    /// type) name it. Decides like `USE`.
    pub const USE_ON_ENTITY: u8 = 2;
    /// The player finished using the item: the last tick of eating or drinking, before the stack
    /// shrinks. Only consumables finish. The result is ignored.
    pub const FINISH_USING: u8 = 3;
    /// Every tick, for each stack of the item in the player's inventory (`Item.inventoryTick`).
    /// `slot` and `selected` tell where. The result is ignored.
    pub const INVENTORY_TICK: u8 = 4;
    /// The tooltip of a stack of the item is built (`Item.appendHoverText`; client only, called
    /// whenever the tooltip is drawn, so keep it cheap): the handler adds lines through
    /// [`add_line`](super::ItemPayloadV1::add_line). The result is ignored.
    pub const APPEND_TOOLTIP: u8 = 5;
    /// Number of events.
    pub const COUNT: u8 = 6;
    /// The mask with every event.
    pub const ALL: u32 = (1 << COUNT) - 1;
    /// The names of the events, by value.
    pub const NAMES: [&str; COUNT as usize] = [
        "use",
        "use_on_block",
        "use_on_entity",
        "finish_using",
        "inventory_tick",
        "append_tooltip",
    ];
}

/// [`ItemPayloadV1::slot`] values.
pub mod slot {
    /// No slot (every event but `INVENTORY_TICK`).
    pub const NONE: i32 = -1;
    /// Slots `0..36` are the inventory: `0..9` the hotbar, then the main inventory.
    pub const INVENTORY_COUNT: i32 = 36;
    /// Slots `36..40` are the armor: head, chest, legs, feet.
    pub const ARMOR_FIRST: i32 = 36;
    /// The off hand.
    pub const OFFHAND: i32 = 40;
}

/// What differs from the template. Strings are borrowed for the call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ItemSettingsV1 {
    /// `size_of::<ItemSettingsV1>()`.
    pub size: u32,
    /// The `set::*` bits of the fields below that mean something; the others keep the template's.
    pub set: u32,
    /// The largest stack, 1 to 99.
    pub max_stack: u8,
    /// A `rarity::*` value.
    pub rarity: u8,
    /// Padding, zero.
    pub reserved: [u8; 2],
    /// The vanilla item to copy (`minecraft:<path>`).
    pub template: FishStr,
}

/// Receives one tooltip line (UTF-8, `§` colour codes allowed) while an `APPEND_TOOLTIP` handler
/// runs; `lines` is [`ItemPayloadV1::lines`].
pub type AddLineFn = unsafe extern "C" fn(lines: *mut c_void, line: FishStr);

/// What an item handler is called with. Strings are borrowed for the call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ItemPayloadV1 {
    /// `size_of::<ItemPayloadV1>()`.
    pub size: u32,
    /// An `event::*` value.
    pub event: u8,
    /// A `client::ROLE_*` value: which game raised the event.
    pub role: u8,
    /// `game::HAND_*` (use, finish).
    pub hand: u8,
    /// `0..=5` (a `game::FACE_NAMES` index) or `game::FACE_NONE` (use on a block).
    pub face: u8,
    /// A `slot::*` value (inventory tick), else `slot::NONE`.
    pub slot: i32,
    /// How many items the stack holds.
    pub count: u32,
    /// The block clicked (use on a block), else zero.
    pub pos: [i32; 3],
    /// Nonzero when the stack is in the selected hotbar slot or the off hand (inventory tick).
    pub selected: u8,
    /// Padding, zero.
    pub reserved: [u8; 3],
    /// The entity's network id (use on an entity), else zero.
    pub entity_id: i64,
    /// The acting player's UUID bytes, zero when there is none.
    pub actor_uuid: [u8; 16],
    /// The item's id.
    pub item: FishStr,
    /// The acting player's name, empty when there is none.
    pub actor: FishStr,
    /// The dimension id.
    pub dimension: FishStr,
    /// The block's id (use on a block) or the entity's type (use on an entity), else empty.
    pub target: FishStr,
    /// Adds a tooltip line (`APPEND_TOOLTIP`), else absent.
    pub add_line: Option<AddLineFn>,
    /// The first argument of `add_line`.
    pub lines: *mut c_void,
    /// The game that raised the event, valid during the call.
    pub view: FishHandle,
}

/// [`ItemHandlerFn`] result: the handler panicked (the loader stops the mod).
pub const HANDLER_PANICKED: u8 = 255;

/// Handles the events of one item: `event` is an `event::*` value. The result is an
/// `ActionResult` byte, or [`HANDLER_PANICKED`]; the events that take no decision ignore an
/// `ActionResult`.
pub type ItemHandlerFn =
    unsafe extern "C" fn(user: *mut c_void, event: u8, payload: *const ItemPayloadV1) -> u8;

/// `fish:item` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ItemServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Registers item `id` for mod `token`. `events` is the mask of `1 << event::*` bits the
    /// `handler` wants (a handler may be absent: the item then only exists). `user` is dropped
    /// with `drop_user` when the loader goes. `INVALID_ARGUMENT` for a bad id (the namespace must
    /// be the mod's id), template or setting and after the registries froze; `ALREADY_EXISTS`
    /// when an item or a block of that id exists.
    pub register: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        id: FishStr,
        settings: *const ItemSettingsV1,
        events: u32,
        handler: Option<ItemHandlerFn>,
        user: *mut c_void,
        drop_user: Option<DropUserFn>,
    ) -> FishStatus,
}

/// `fish:item_group` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ItemGroupServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Registers the creative tab `id` (`namespace:path`, the namespace is the mod's id) of mod
    /// `token`. `title` is the text on the tab (empty: made from the id), `icon` the item shown on
    /// it, `entries` the `entry_count` item ids it lists. `INVALID_ARGUMENT` for a bad id, icon or
    /// entry and after the registries froze; `ALREADY_EXISTS` when the tab exists.
    pub register: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        id: FishStr,
        title: FishStr,
        icon: FishStr,
        entries: *const FishStr,
        entry_count: usize,
    ) -> FishStatus,
    /// Adds the `entry_count` item ids `entries` to the end of the tab `group` (a vanilla one such
    /// as `minecraft:ingredients`, or any tab a mod registers). A tab that does not exist is
    /// logged when the creative inventory is built. `INVALID_ARGUMENT` for a bad id and after the
    /// registries froze.
    pub modify_entries: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        group: FishStr,
        entries: *const FishStr,
        entry_count: usize,
    ) -> FishStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn layouts_are_fixed() {
        assert_eq!(size_of::<ItemSettingsV1>(), 32);
        assert_eq!(offset_of!(ItemSettingsV1, max_stack), 8);
        assert_eq!(offset_of!(ItemSettingsV1, rarity), 9);
        assert_eq!(offset_of!(ItemSettingsV1, template), 16);
        assert_eq!(size_of::<ItemPayloadV1>(), 144);
        assert_eq!(offset_of!(ItemPayloadV1, event), 4);
        assert_eq!(offset_of!(ItemPayloadV1, slot), 8);
        assert_eq!(offset_of!(ItemPayloadV1, count), 12);
        assert_eq!(offset_of!(ItemPayloadV1, pos), 16);
        assert_eq!(offset_of!(ItemPayloadV1, selected), 28);
        assert_eq!(offset_of!(ItemPayloadV1, entity_id), 32);
        assert_eq!(offset_of!(ItemPayloadV1, actor_uuid), 40);
        assert_eq!(offset_of!(ItemPayloadV1, item), 56);
        assert_eq!(offset_of!(ItemPayloadV1, target), 104);
        assert_eq!(offset_of!(ItemPayloadV1, add_line), 120);
        assert_eq!(offset_of!(ItemPayloadV1, view), 136);
    }

    #[test]
    fn masks_cover_every_event() {
        assert_eq!(event::ALL, 0b11_1111);
        assert_eq!(event::NAMES.len(), usize::from(event::COUNT));
        assert_eq!(set::ALL, 0b11);
        assert_eq!(rarity::NAMES[usize::from(rarity::EPIC)], "epic");
        assert_eq!(slot::OFFHAND, slot::ARMOR_FIRST + 4);
    }
}
