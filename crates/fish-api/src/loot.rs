//! Loot tables (Fabric's `LootTableEvents.MODIFY` and `REPLACE`): a mod adds pools to a loot
//! table, vanilla's or any mod's, or replaces the table, in the data pack's JSON format.
//!
//! ```no_run
//! use fish_api::loot::{LootPool, LootTables};
//!
//! # fn init() -> Result<(), fish_api::Error> {
//! // a zombie sometimes drops a ruby
//! LootTables::add_pool(
//!     "minecraft:entities/zombie",
//!     &LootPool::item("mymod:ruby").chance(0.05).to_json(),
//! )?;
//! // a dead bush drops a ruby instead of sticks
//! LootTables::replace(
//!     "minecraft:blocks/dead_bush",
//!     r#"{"type": "minecraft:block", "pools": [{"rolls": 1, "entries": [{"type": "minecraft:item", "name": "mymod:ruby"}]}]}"#,
//! )?;
//! # Ok(()) }
//! ```
//!
//! Register while `pre_launch` or `main` runs (the same freeze as [`registry`](crate::registry)).
//! The game applies the changes once, when it loads its loot tables: replacements first (one per
//! table: a second is `ALREADY_EXISTS`), then the added pools in registration order. A change of a
//! table the game does not have is skipped with a warning. New tables of a mod are files, not
//! changes: `data/<mod id>/loot_table/...` ([resources](crate::resource)).

use fish_abi::FishStr;
use fish_abi::loot::{LootServiceV1, SERVICE_LOOT, kind};

use crate::client::service;
use crate::content::finish;
use crate::error::Error;
use crate::host;

fn modify(table: &str, change: u8, json: &str) -> Result<(), Error> {
    let (svc, host_ptr) = service::<LootServiceV1>(SERVICE_LOOT)?;
    let t = host::get()?;
    // SAFETY: valid service table; the strings outlive the call.
    let status = unsafe {
        (svc.modify)(
            host_ptr,
            t.mod_token,
            FishStr::new(table),
            change,
            FishStr::new(json),
        )
    };
    finish("changing a loot table", status)
}

/// Loot table changes.
pub struct LootTables;

impl LootTables {
    /// Appends the pool `pool_json` (one pool object, or a list of pools) to loot table `table`.
    pub fn add_pool(table: &str, pool_json: &str) -> Result<(), Error> {
        modify(table, kind::ADD_POOLS, pool_json)
    }

    /// Replaces loot table `table` with `table_json` (a whole table).
    pub fn replace(table: &str, table_json: &str) -> Result<(), Error> {
        modify(table, kind::REPLACE, table_json)
    }
}

/// A pool of one item, written as data pack JSON (for the common case; any pool JSON works with
/// [`LootTables::add_pool`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LootPool {
    item: String,
    rolls: u32,
    count: Option<(u32, u32)>,
    chance: Option<f32>,
}

impl LootPool {
    /// One roll of `item`.
    pub fn item(item: &str) -> LootPool {
        LootPool {
            item: item.to_string(),
            rolls: 1,
            count: None,
            chance: None,
        }
    }

    /// How many times the pool rolls.
    pub fn rolls(mut self, rolls: u32) -> Self {
        self.rolls = rolls;
        self
    }

    /// A stack of `min..=max` items per roll (`minecraft:set_count`).
    pub fn count(mut self, min: u32, max: u32) -> Self {
        self.count = Some((min, max));
        self
    }

    /// The pool rolls only with probability `chance` (`minecraft:random_chance`).
    pub fn chance(mut self, chance: f32) -> Self {
        self.chance = Some(chance);
        self
    }

    /// The pool as JSON.
    pub fn to_json(&self) -> String {
        let functions = match self.count {
            Some((min, max)) => format!(
                r#","functions":[{{"function":"minecraft:set_count","count":{{"type":"minecraft:uniform","min":{min},"max":{max}}}}}]"#
            ),
            None => String::new(),
        };
        let conditions = match self.chance {
            Some(p) => {
                format!(r#","conditions":[{{"condition":"minecraft:random_chance","chance":{p}}}]"#)
            }
            None => String::new(),
        };
        let item = self.item.replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            r#"{{"rolls":{},"entries":[{{"type":"minecraft:item","name":"{item}"{functions}}}]{conditions}}}"#,
            self.rolls
        )
    }
}
