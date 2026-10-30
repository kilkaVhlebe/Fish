//! The `fish:block` service (phase P8): blocks registered by mods.
//!
//! A block is declared by data, not by code: an identifier (`namespace:path`, the namespace is the
//! registering mod's id), a vanilla *template* block whose behaviour class, state properties and
//! shapes it copies, and the settings that differ from the template. The loader turns the blocks
//! of every mod into a content mod for Horizon (`data/<namespace>/horizon/block/<path>.json`, next
//! to the mod's own `assets/` and `data/`) before the game reads its resources, so a block gets
//! its models, textures and translations from the `.fish` archive of the mod.
//!
//! Registration is allowed while the `pre_launch` and `main` entrypoints run (the same freeze rule
//! as the registries). Once Horizon built the game's block states the loader knows the state ids of
//! every block ([`BlockServiceV1::state_range`]) and calls the mod's handler by state id.

use core::ffi::c_void;

use crate::tables::{DropUserFn, FishServiceHeader};
use crate::types::{FishHandle, FishStatus, FishStr};

/// Service id.
pub const SERVICE_BLOCK: &str = "fish:block";

/// [`BlockSettingsV1::set`] bits: the settings that differ from the template.
pub mod set {
    /// [`BlockSettingsV1::hardness`](super::BlockSettingsV1::hardness) is set.
    pub const HARDNESS: u32 = 1;
    /// [`BlockSettingsV1::resistance`](super::BlockSettingsV1::resistance) is set.
    pub const RESISTANCE: u32 = 1 << 1;
    /// [`BlockSettingsV1::light`](super::BlockSettingsV1::light) is set.
    pub const LIGHT: u32 = 1 << 2;
    /// [`BlockSettingsV1::requires_tool`](super::BlockSettingsV1::requires_tool) is set.
    pub const REQUIRES_TOOL: u32 = 1 << 3;
    /// [`BlockSettingsV1::friction`](super::BlockSettingsV1::friction) is set.
    pub const FRICTION: u32 = 1 << 4;
    /// [`BlockSettingsV1::map_color`](super::BlockSettingsV1::map_color) is set.
    pub const MAP_COLOR: u32 = 1 << 5;
    /// [`BlockSettingsV1::sounds`](super::BlockSettingsV1::sounds) is set.
    pub const SOUNDS: u32 = 1 << 6;
    /// Every bit above.
    pub const ALL: u32 = (1 << 7) - 1;
}

/// [`BlockPayloadV1::event`] values, and the bit (`1 << value`) a mod sets in the `events` mask of
/// [`BlockServiceV1::register`] for each event its handler wants.
pub mod event {
    /// A player used the block (`Block.useWithoutItem`). The handler's result decides: `Pass`
    /// leaves the click to the game, anything else ends it.
    pub const USE: u8 = 0;
    /// An item placed the block (a player's, or a dispenser's; `BlockItem.place`). The result is
    /// ignored.
    pub const PLACE: u8 = 1;
    /// A player broke the block (`Block.playerDestroy`; reported once the block is gone). The
    /// result is ignored.
    pub const BROKEN: u8 = 2;
    /// A neighbour of the block changed (`Block.neighborChanged`). `other_pos` is the neighbour
    /// when the game says which one changed, else `pos` itself (Horizon names the block that
    /// changed, not where). The result is ignored.
    pub const NEIGHBOR_CHANGED: u8 = 3;
    /// The block was picked by a random tick (`Block.randomTick`). The result is ignored.
    pub const RANDOM_TICK: u8 = 4;
    /// A scheduled tick of the block ran (`Block.tick`). The result is ignored.
    pub const SCHEDULED_TICK: u8 = 5;
    /// Number of events.
    pub const COUNT: u8 = 6;
    /// The mask with every event.
    pub const ALL: u32 = (1 << COUNT) - 1;
    /// The names of the events, by value.
    pub const NAMES: [&str; COUNT as usize] = [
        "use",
        "place",
        "broken",
        "neighbor_changed",
        "random_tick",
        "scheduled_tick",
    ];
}

/// What differs from the template. Strings are borrowed for the call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BlockSettingsV1 {
    /// `size_of::<BlockSettingsV1>()`.
    pub size: u32,
    /// The `set::*` bits of the fields below that mean something; the others keep the template's.
    pub set: u32,
    /// Seconds of hand mining (`-1`: unbreakable).
    pub hardness: f32,
    /// Explosion resistance, 0 or more.
    pub resistance: f32,
    /// Friction, 0 to 2.
    pub friction: f32,
    /// Light emitted, 0 to 15.
    pub light: u8,
    /// Nonzero: the right tool is needed for drops.
    pub requires_tool: u8,
    /// A map colour id, 0 to 61.
    pub map_color: u8,
    /// Padding, zero.
    pub reserved: u8,
    /// The vanilla block to copy (`minecraft:<path>`).
    pub template: FishStr,
    /// The vanilla block whose sounds the block uses (`minecraft:<path>`).
    pub sounds: FishStr,
}

/// What a block handler is called with. Strings are borrowed for the call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BlockPayloadV1 {
    /// `size_of::<BlockPayloadV1>()`.
    pub size: u32,
    /// An `event::*` value.
    pub event: u8,
    /// A `client::ROLE_*` value: which game raised the event.
    pub role: u8,
    /// `game::HAND_*` (use).
    pub hand: u8,
    /// `0..=5` (a `game::FACE_NAMES` index) or `game::FACE_NONE` (use).
    pub face: u8,
    /// The block state's id in the game.
    pub state: u32,
    /// The state's index among the block's own states (`state - first state of the block`).
    pub state_index: u32,
    /// The block's position.
    pub pos: [i32; 3],
    /// The neighbour that changed (`NEIGHBOR_CHANGED`; `pos` when unknown), else zero.
    pub other_pos: [i32; 3],
    /// The acting player's UUID bytes, zero when there is none.
    pub actor_uuid: [u8; 16],
    /// The block's id.
    pub block: FishStr,
    /// The acting player's name, empty when there is none.
    pub actor: FishStr,
    /// The dimension id.
    pub dimension: FishStr,
    /// The item in the acting hand (use, place), else empty.
    pub item: FishStr,
    /// The game that raised the event, valid during the call.
    pub view: FishHandle,
}

/// [`BlockHandlerFn`] result: the handler panicked (the loader stops the mod).
pub const HANDLER_PANICKED: u8 = 255;

/// Handles the events of one block: `event` is an `event::*` value. The result is an
/// `ActionResult` byte, or [`HANDLER_PANICKED`]; the events that take no decision ignore an
/// `ActionResult`.
pub type BlockHandlerFn =
    unsafe extern "C" fn(user: *mut c_void, event: u8, payload: *const BlockPayloadV1) -> u8;

/// `fish:block` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BlockServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Registers block `id` for mod `token`. `events` is the mask of `1 << event::*` bits the
    /// `handler` wants (a handler may be absent: the block then only exists). `user` is dropped
    /// with `drop_user` when the loader goes. `INVALID_ARGUMENT` for a bad id (the namespace must
    /// be the mod's id), template or setting and after the registries froze; `ALREADY_EXISTS`
    /// when a block of that id exists.
    pub register: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        id: FishStr,
        settings: *const BlockSettingsV1,
        events: u32,
        handler: Option<BlockHandlerFn>,
        user: *mut c_void,
        drop_user: Option<DropUserFn>,
    ) -> FishStatus,
    /// Reads the state ids Horizon gave block `id`: the first one and how many. `NOT_FOUND` for an
    /// unknown block, and while the game has not built its block states yet (or refused the block).
    pub state_range: unsafe extern "C" fn(
        host: *mut c_void,
        id: FishStr,
        first: *mut u32,
        count: *mut u32,
    ) -> FishStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn layouts_are_fixed() {
        assert_eq!(size_of::<BlockSettingsV1>(), 56);
        assert_eq!(offset_of!(BlockSettingsV1, hardness), 8);
        assert_eq!(offset_of!(BlockSettingsV1, light), 20);
        assert_eq!(offset_of!(BlockSettingsV1, template), 24);
        assert_eq!(offset_of!(BlockSettingsV1, sounds), 40);
        assert_eq!(size_of::<BlockPayloadV1>(), 128);
        assert_eq!(offset_of!(BlockPayloadV1, event), 4);
        assert_eq!(offset_of!(BlockPayloadV1, state), 8);
        assert_eq!(offset_of!(BlockPayloadV1, pos), 16);
        assert_eq!(offset_of!(BlockPayloadV1, other_pos), 28);
        assert_eq!(offset_of!(BlockPayloadV1, actor_uuid), 40);
        assert_eq!(offset_of!(BlockPayloadV1, block), 56);
        assert_eq!(offset_of!(BlockPayloadV1, view), 120);
    }

    #[test]
    fn event_masks_cover_every_event() {
        assert_eq!(event::ALL, 0b11_1111);
        assert_eq!(event::NAMES.len(), usize::from(event::COUNT));
        assert_eq!(set::ALL, 0b111_1111);
    }
}
