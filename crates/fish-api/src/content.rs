//! Content tables: fuel, composting, flammability, tool actions, path node types and sculk
//! frequencies (Fabric's `FuelRegistry`, `CompostingChanceRegistry`, `FlammableBlockRegistry`,
//! `StrippableBlockRegistry`, `TillableBlockRegistry`, `FlattenableBlockRegistry`,
//! `OxidizableBlocksRegistry` and `SculkSensorFrequencyRegistry`).
//!
//! Register while `pre_launch` or `main` runs (the same freeze as [`registry`](crate::registry)):
//!
//! ```no_run
//! use fish_api::content::{Content, PathNodeType};
//!
//! # fn init() -> Result<(), fish_api::Error> {
//! Content::fuel("mymod:ruby", 1600)?;                       // a lump of coal burns 1600 ticks
//! Content::compostable("mymod:ruby", 0.65)?;                // 65 % to add a composter layer
//! Content::flammable("mymod:planks", 5, 20)?;               // ignite odds, burn odds
//! Content::strippable("mymod:log", "mymod:stripped_log")?;  // an axe
//! Content::tillable("mymod:soil", "minecraft:farmland")?;   // a hoe
//! Content::flattenable("mymod:soil", "minecraft:dirt_path")?; // a shovel
//! Content::waxable("mymod:copper", "mymod:waxed_copper")?;  // honeycomb, and the axe back
//! Content::oxidizable("mymod:copper", "mymod:exposed_copper")?; // scraping with an axe
//! Content::path_node_type("mymod:thorns", PathNodeType::Damaging)?;
//! Content::sculk_frequency("minecraft:step", 7)?;
//! # Ok(()) }
//! ```
//!
//! Items, blocks and game events are named by identifier, vanilla or from any mod; the first
//! registration of a key wins and a second one fails with `ALREADY_EXISTS`. A block or item that
//! does not exist when the game loads makes its entry a no-op.
//!
//! Limits: horizon ages only vanilla copper by itself, so [`Content::oxidizable`] adds the scraping
//! but a block of a mod never oxidizes over time; the game knows only the vanilla game events, so
//! [`Content::sculk_frequency`] changes the frequency of those.

use fish_abi::content::{
    ContentServiceV1, FREQUENCY_MAX, FUEL_TICKS_MAX, ODDS_MAX, SERVICE_CONTENT, path_type,
    transform,
};
use fish_abi::{FishStatus, FishStr};

use crate::client::service;
use crate::error::{Error, check};
use crate::host;

/// Turns a registration status into a `Result`, saying so when the registries froze.
pub(crate) fn finish(what: &'static str, status: FishStatus) -> Result<(), Error> {
    if status == FishStatus::INVALID_ARGUMENT && crate::registry::is_frozen()? {
        return Err(Error::InvalidArgument("registries are frozen".to_string()));
    }
    check(what, status)
}

/// What a block makes mobs think of walking through it (Horizon's `PathType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathNodeType {
    /// Mobs never enter.
    Blocked,
    /// Air.
    Open,
    /// A floor to walk on.
    Walkable,
    /// A door in a doorway that can be walked through.
    WalkableDoor,
    /// A trapdoor.
    Trapdoor,
    /// Powder snow.
    PowderSnow,
    /// The top of powder snow.
    OnTopOfPowderSnow,
    /// A fence.
    Fence,
    /// Lava.
    Lava,
    /// Water.
    Water,
    /// The shore of water.
    WaterBorder,
    /// A rail.
    Rail,
    /// A rail that cannot be crossed.
    UnpassableRail,
    /// Next to fire.
    FireInNeighbor,
    /// Fire.
    Fire,
    /// Next to something that hurts.
    DamagingInNeighbor,
    /// Something that hurts (a cactus, a berry bush).
    Damaging,
    /// An open door.
    DoorOpen,
    /// A closed wooden door.
    DoorWoodClosed,
    /// A closed iron door.
    DoorIronClosed,
    /// Water mobs can breach.
    Breach,
    /// Leaves.
    Leaves,
    /// Honey.
    StickyHoney,
    /// Cocoa pods.
    Cocoa,
    /// Mobs step carefully.
    DamageCautious,
    /// The top of a trapdoor.
    OnTopOfTrapdoor,
    /// Big mobs keep clear.
    BigMobsCloseToDanger,
}

impl PathNodeType {
    /// Every type, in the order of the ABI's values.
    pub const ALL: [PathNodeType; 27] = [
        PathNodeType::Blocked,
        PathNodeType::Open,
        PathNodeType::Walkable,
        PathNodeType::WalkableDoor,
        PathNodeType::Trapdoor,
        PathNodeType::PowderSnow,
        PathNodeType::OnTopOfPowderSnow,
        PathNodeType::Fence,
        PathNodeType::Lava,
        PathNodeType::Water,
        PathNodeType::WaterBorder,
        PathNodeType::Rail,
        PathNodeType::UnpassableRail,
        PathNodeType::FireInNeighbor,
        PathNodeType::Fire,
        PathNodeType::DamagingInNeighbor,
        PathNodeType::Damaging,
        PathNodeType::DoorOpen,
        PathNodeType::DoorWoodClosed,
        PathNodeType::DoorIronClosed,
        PathNodeType::Breach,
        PathNodeType::Leaves,
        PathNodeType::StickyHoney,
        PathNodeType::Cocoa,
        PathNodeType::DamageCautious,
        PathNodeType::OnTopOfTrapdoor,
        PathNodeType::BigMobsCloseToDanger,
    ];

    /// The type's value on the wire.
    pub fn as_u8(self) -> u8 {
        Self::ALL
            .iter()
            .position(|t| *t == self)
            .expect("every type is listed") as u8
    }

    /// The name the game's mob data uses (`DAMAGING`).
    pub fn name(self) -> &'static str {
        path_type::NAMES[usize::from(self.as_u8())]
    }
}

/// The content tables.
pub struct Content;

impl Content {
    /// Makes `item` a furnace fuel burning `burn_ticks` ticks (`1..=1_000_000`). Zero removes the
    /// fuel property of a vanilla item.
    pub fn fuel(item: &str, burn_ticks: u32) -> Result<(), Error> {
        let (svc, host_ptr) = service::<ContentServiceV1>(SERVICE_CONTENT)?;
        let table = host::get()?;
        // SAFETY: valid service table; the string outlives the call.
        let status =
            unsafe { (svc.fuel)(host_ptr, table.mod_token, FishStr::new(item), burn_ticks) };
        finish("registering a fuel", status)
    }

    /// Makes `item` no longer a fuel: a stick that does not burn.
    pub fn remove_fuel(item: &str) -> Result<(), Error> {
        Self::fuel(item, 0)
    }

    /// Makes `item` compostable: a composter takes a layer with probability `chance`, in
    /// whole percent (`0.01..=1.0`; `0.3` is 30 %). Zero removes the property of a vanilla item.
    pub fn compostable(item: &str, chance: f32) -> Result<(), Error> {
        let (svc, host_ptr) = service::<ContentServiceV1>(SERVICE_CONTENT)?;
        let table = host::get()?;
        // SAFETY: valid service table; the string outlives the call.
        let status =
            unsafe { (svc.compostable)(host_ptr, table.mod_token, FishStr::new(item), chance) };
        finish("registering a compostable item", status)
    }

    /// Makes `item` no longer compostable.
    pub fn remove_compostable(item: &str) -> Result<(), Error> {
        Self::compostable(item, 0.0)
    }

    /// Makes `block` flammable: fire spreads onto it with `ignite_odds` and consumes it with
    /// `burn_odds`, each `0..=255` (vanilla planks are 5 and 20, wool 30 and 60).
    pub fn flammable(block: &str, ignite_odds: i32, burn_odds: i32) -> Result<(), Error> {
        let (svc, host_ptr) = service::<ContentServiceV1>(SERVICE_CONTENT)?;
        let table = host::get()?;
        // SAFETY: valid service table; the string outlives the call.
        let status = unsafe {
            (svc.flammable)(
                host_ptr,
                table.mod_token,
                FishStr::new(block),
                ignite_odds,
                burn_odds,
            )
        };
        finish("registering a flammable block", status)
    }

    /// Makes a vanilla `block` fireproof (both odds zero).
    pub fn fireproof(block: &str) -> Result<(), Error> {
        Self::flammable(block, 0, 0)
    }

    fn transform(kind: u8, from: &str, to: &str) -> Result<(), Error> {
        let (svc, host_ptr) = service::<ContentServiceV1>(SERVICE_CONTENT)?;
        let table = host::get()?;
        // SAFETY: valid service table; the strings outlive the call.
        let status = unsafe {
            (svc.transform)(
                host_ptr,
                table.mod_token,
                kind,
                FishStr::new(from),
                FishStr::new(to),
            )
        };
        finish("registering a tool action", status)
    }

    /// An axe turns `block` into `stripped`, keeping the properties both have (`axis`).
    pub fn strippable(block: &str, stripped: &str) -> Result<(), Error> {
        Self::transform(transform::STRIP, block, stripped)
    }

    /// A hoe turns `block` into `tilled` (farmland) when the block above is air, but not from
    /// below.
    pub fn tillable(block: &str, tilled: &str) -> Result<(), Error> {
        Self::transform(transform::TILL, block, tilled)
    }

    /// A shovel turns `block` into `flattened` (a dirt path) when the block above is air, but
    /// not from below.
    pub fn flattenable(block: &str, flattened: &str) -> Result<(), Error> {
        Self::transform(transform::FLATTEN, block, flattened)
    }

    /// Honeycomb turns `unwaxed` into `waxed`; an axe takes the wax off again.
    pub fn waxable(unwaxed: &str, waxed: &str) -> Result<(), Error> {
        Self::transform(transform::WAX, unwaxed, waxed)
    }

    /// An axe scrapes `more` back into `less`. The game ages only vanilla copper by itself: a
    /// block of a mod never advances.
    pub fn oxidizable(less: &str, more: &str) -> Result<(), Error> {
        Self::transform(transform::OXIDIZE, less, more)
    }

    /// Sets the pathfinding type of every state of `block`.
    pub fn path_node_type(block: &str, kind: PathNodeType) -> Result<(), Error> {
        let (svc, host_ptr) = service::<ContentServiceV1>(SERVICE_CONTENT)?;
        let table = host::get()?;
        // SAFETY: valid service table; the string outlives the call.
        let status = unsafe {
            (svc.path_type)(host_ptr, table.mod_token, FishStr::new(block), kind.as_u8())
        };
        finish("registering a path node type", status)
    }

    /// Sets the frequency (`0..=15`, zero: no vibration) a sculk sensor hears for the vanilla
    /// game event `event` (`minecraft:step`).
    pub fn sculk_frequency(event: &str, frequency: u8) -> Result<(), Error> {
        let (svc, host_ptr) = service::<ContentServiceV1>(SERVICE_CONTENT)?;
        let table = host::get()?;
        // SAFETY: valid service table; the string outlives the call.
        let status = unsafe {
            (svc.sculk_frequency)(host_ptr, table.mod_token, FishStr::new(event), frequency)
        };
        finish("registering a sculk frequency", status)
    }

    /// The most ticks a fuel may burn.
    pub const FUEL_TICKS_MAX: u32 = FUEL_TICKS_MAX;
    /// The largest fire odds of a block.
    pub const ODDS_MAX: i32 = ODDS_MAX;
    /// The largest sculk frequency.
    pub const FREQUENCY_MAX: u8 = FREQUENCY_MAX;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_node_types_follow_the_abi_names() {
        for (i, t) in PathNodeType::ALL.iter().enumerate() {
            assert_eq!(usize::from(t.as_u8()), i);
            assert_eq!(t.name(), path_type::NAMES[i]);
        }
        assert_eq!(PathNodeType::Damaging.name(), "DAMAGING");
        assert_eq!(PathNodeType::ALL.len(), usize::from(path_type::COUNT));
    }
}
