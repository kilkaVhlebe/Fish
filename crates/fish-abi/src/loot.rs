//! The `fish:loot` service (phase P9): changes mods make to loot tables (Fabric's
//! `LootTableEvents.MODIFY` and `REPLACE`).
//!
//! A change names a loot table by id (`minecraft:blocks/stone`, `minecraft:entities/zombie`,
//! `mymod:chests/cave`; vanilla's or any mod's) and carries JSON in the data pack format: pools to
//! add, or a whole table that replaces the table. Changes are registered while the `pre_launch` and
//! `main` entrypoints run (the same freeze rule as the registries); the game applies them once,
//! when it loads its loot tables: every table's replacement first (the first registration wins),
//! then the added pools in registration order. A change of a table the game does not have is
//! skipped with a warning.

use crate::tables::FishServiceHeader;
use crate::types::{FishStatus, FishStr};
use core::ffi::c_void;

/// Service id.
pub const SERVICE_LOOT: &str = "fish:loot";

/// The most bytes of JSON one change carries.
pub const MAX_JSON: usize = 1 << 20;

/// [`LootServiceV1::modify`] kinds.
pub mod kind {
    /// `json` is one pool object or a list of pools, appended to the table's `pools`.
    pub const ADD_POOLS: u8 = 0;
    /// `json` is a whole loot table that replaces the table.
    pub const REPLACE: u8 = 1;
    /// Number of kinds.
    pub const COUNT: u8 = 2;
}

/// `fish:loot` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LootServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Registers a change of loot table `table` for mod `token`: a [`kind`] and its JSON.
    /// `INVALID_ARGUMENT` for a bad id, kind or JSON and after the registries froze;
    /// `ALREADY_EXISTS` for a second replacement of one table.
    pub modify: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        table: FishStr,
        kind: u8,
        json: FishStr,
    ) -> FishStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn the_table_is_one_function_after_the_header() {
        assert_eq!(
            size_of::<LootServiceV1>(),
            size_of::<FishServiceHeader>() + size_of::<usize>()
        );
        assert_eq!(kind::COUNT, 2);
        const { assert!(MAX_JSON >= 1 << 16) };
    }
}
