//! The `fish:content` service (phase P8): the content tables of the game that mods add to or change
//! (Fabric's `FuelRegistry`, `CompostingChanceRegistry`, `FlammableBlockRegistry`,
//! `StrippableBlockRegistry`, `TillableBlockRegistry`, `FlattenableBlockRegistry`,
//! `OxidizableBlocksRegistry`, `SculkSensorFrequencyRegistry`, and the path node type of a block).
//!
//! Every call names a vanilla or mod item, block or game event by identifier (`namespace:path`;
//! unlike items and blocks of a mod, these ids are *not* confined to the namespace of the calling
//! mod: a table entry about `minecraft:oak_log` is how a mod changes the vanilla game). Registration
//! is allowed while the `pre_launch` and `main` entrypoints run (the same freeze rule as the
//! registries); the first registration of a key wins and a second one is `ALREADY_EXISTS`.
//!
//! The loader keeps the tables; the game reads them where it builds its own (the items, the block
//! behaviours, the block transformers of axes, hoes and shovels, the mob data, the vibration
//! frequencies), so the entries take effect for every game of the process.

use crate::tables::FishServiceHeader;
use crate::types::{FishStatus, FishStr};
use core::ffi::c_void;

/// Service id.
pub const SERVICE_CONTENT: &str = "fish:content";

/// The longest burn time of a fuel, in ticks (a furnace item takes 200 ticks to smelt).
pub const FUEL_TICKS_MAX: u32 = 1_000_000;
/// The largest ignite or burn odds of a flammable block (vanilla's largest is 100).
pub const ODDS_MAX: i32 = 255;
/// The largest vibration frequency a sculk sensor tells apart.
pub const FREQUENCY_MAX: u8 = 15;

/// [`ContentServiceV1::transform`] kinds: what a tool does to a block.
pub mod transform {
    /// An axe strips `from` into `to` (stripped logs): `from` and `to` share their properties.
    pub const STRIP: u8 = 0;
    /// A hoe tills `from` into `to` (farmland), when the block above is air; not from below.
    pub const TILL: u8 = 1;
    /// A shovel flattens `from` into `to` (dirt path), when the block above is air; not from below.
    pub const FLATTEN: u8 = 2;
    /// Waxing: `from` is the unwaxed block and `to` its waxed twin. Honeycomb turns `from` into `to`
    /// and an axe scrapes the wax off `to` again.
    pub const WAX: u8 = 3;
    /// Oxidation: `from` is the less and `to` the more oxidized block. An axe scrapes `to` back
    /// into `from`. Horizon ages only vanilla copper by itself: a block of a mod never advances.
    pub const OXIDIZE: u8 = 4;
    /// Number of kinds.
    pub const COUNT: u8 = 5;
    /// The names of the kinds, by value.
    pub const NAMES: [&str; COUNT as usize] = ["strip", "till", "flatten", "wax", "oxidize"];
}

/// [`ContentServiceV1::path_type`] values: the pathfinding type of a block, in the order of
/// Horizon's `PathType`. The names are the ones the game's mob data uses.
pub mod path_type {
    /// Number of path types.
    pub const COUNT: u8 = 27;
    /// The names of the path types, by value.
    pub const NAMES: [&str; COUNT as usize] = [
        "BLOCKED",
        "OPEN",
        "WALKABLE",
        "WALKABLE_DOOR",
        "TRAPDOOR",
        "POWDER_SNOW",
        "ON_TOP_OF_POWDER_SNOW",
        "FENCE",
        "LAVA",
        "WATER",
        "WATER_BORDER",
        "RAIL",
        "UNPASSABLE_RAIL",
        "FIRE_IN_NEIGHBOR",
        "FIRE",
        "DAMAGING_IN_NEIGHBOR",
        "DAMAGING",
        "DOOR_OPEN",
        "DOOR_WOOD_CLOSED",
        "DOOR_IRON_CLOSED",
        "BREACH",
        "LEAVES",
        "STICKY_HONEY",
        "COCOA",
        "DAMAGE_CAUTIOUS",
        "ON_TOP_OF_TRAPDOOR",
        "BIG_MOBS_CLOSE_TO_DANGER",
    ];
}

/// `fish:content` service, version 1. Every function takes the host pointer and the mod's token and
/// returns `INVALID_ARGUMENT` for a bad identifier or number and after the registries froze, and
/// `ALREADY_EXISTS` when the key already has an entry.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ContentServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Makes `item` a furnace fuel burning `burn_ticks` ticks. Zero removes the fuel property of a
    /// vanilla item (a stick no longer burns). `1..=`[`FUEL_TICKS_MAX`] otherwise.
    pub fuel: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        item: FishStr,
        burn_ticks: u32,
    ) -> FishStatus,
    /// Makes `item` compostable: a composter takes a layer with probability `chance`, in `(0, 1]`
    /// (in whole percent: `0.3` is 30 %). Zero removes the property from a vanilla item.
    pub compostable: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        item: FishStr,
        chance: f32,
    ) -> FishStatus,
    /// Makes `block` flammable: fire spreads onto it with `ignite_odds` and consumes it with
    /// `burn_odds`, each `0..=`[`ODDS_MAX`]. Both zero make a vanilla block fireproof.
    pub flammable: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        block: FishStr,
        ignite_odds: i32,
        burn_odds: i32,
    ) -> FishStatus,
    /// Adds a tool action: a [`transform`] `kind` turning block `from` into block `to` (both
    /// blocks must exist when the game loads, or the entry is dropped with a warning).
    pub transform: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        kind: u8,
        from: FishStr,
        to: FishStr,
    ) -> FishStatus,
    /// Sets the pathfinding type of every state of `block` to a [`path_type`] value.
    pub path_type: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        block: FishStr,
        path_type: u8,
    ) -> FishStatus,
    /// Sets the vibration frequency a sculk sensor hears for the game event `event`
    /// (`minecraft:step`), `0..=`[`FREQUENCY_MAX`] (zero: no vibration). Horizon knows only the
    /// vanilla events.
    pub sculk_frequency: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        event: FishStr,
        frequency: u8,
    ) -> FishStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_lists_match_their_counts() {
        assert_eq!(transform::NAMES.len(), usize::from(transform::COUNT));
        assert_eq!(path_type::NAMES.len(), usize::from(path_type::COUNT));
        assert_eq!(transform::NAMES[usize::from(transform::OXIDIZE)], "oxidize");
        assert_eq!(path_type::NAMES[0], "BLOCKED");
        assert_eq!(path_type::NAMES[26], "BIG_MOBS_CLOSE_TO_DANGER");
        let mut names = path_type::NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), path_type::NAMES.len());
        const { assert!(ODDS_MAX > 100 && FREQUENCY_MAX == 15 && FUEL_TICKS_MAX > 200) };
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn the_table_is_function_pointers_after_the_header() {
        assert_eq!(
            size_of::<ContentServiceV1>(),
            size_of::<FishServiceHeader>() + 6 * size_of::<usize>()
        );
    }
}
