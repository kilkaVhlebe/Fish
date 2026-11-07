//! A fake `fish:loot` service and the tests of `loot` built on it.

use std::ffi::c_void;
use std::sync::Mutex;

use fish_abi::loot::{LootServiceV1, SERVICE_LOOT, kind};
use fish_abi::{FishServiceHeader, FishStatus, FishStr};

use crate::loot::{LootPool, LootTables};

/// What the fake saw: (table, kind, json).
static SEEN: Mutex<Vec<(String, u8, String)>> = Mutex::new(Vec::new());

unsafe extern "C" fn modify(
    _h: *mut c_void,
    _token: u64,
    table: FishStr,
    change: u8,
    json: FishStr,
) -> FishStatus {
    // SAFETY: the API passes valid strings.
    let (table, json) = unsafe { (table.as_str().unwrap(), json.as_str().unwrap()) };
    if *crate::fake_rules::FROZEN.lock().unwrap() || change >= kind::COUNT {
        return FishStatus::INVALID_ARGUMENT;
    }
    let mut seen = SEEN.lock().unwrap();
    if change == kind::REPLACE
        && seen
            .iter()
            .any(|(t, k, _)| t == table && *k == kind::REPLACE)
    {
        return FishStatus::ALREADY_EXISTS;
    }
    seen.push((table.to_string(), change, json.to_string()));
    FishStatus::OK
}

static LOOT: LootServiceV1 = LootServiceV1 {
    header: FishServiceHeader {
        size: size_of::<LootServiceV1>(),
        version: 1,
    },
    modify,
};

/// The table for a service id of this fake.
pub(crate) fn lookup(id: &str) -> Option<*const c_void> {
    (id == SERVICE_LOOT).then_some((&raw const LOOT).cast())
}

fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let guard = crate::fake_rules::SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::fake::install();
    SEEN.lock().unwrap().clear();
    *crate::fake_rules::FROZEN.lock().unwrap() = false;
    guard
}

#[test]
fn changes_reach_the_host_and_the_freeze_is_reported() {
    let _g = fresh();
    LootTables::add_pool("minecraft:entities/zombie", "{}").unwrap();
    LootTables::replace("minecraft:blocks/dead_bush", r#"{"pools":[]}"#).unwrap();
    assert_eq!(
        LootTables::replace("minecraft:blocks/dead_bush", "{}")
            .unwrap_err()
            .status(),
        Some(FishStatus::ALREADY_EXISTS)
    );
    let seen = SEEN.lock().unwrap().clone();
    assert_eq!(
        seen,
        [
            (
                "minecraft:entities/zombie".to_string(),
                kind::ADD_POOLS,
                "{}".to_string()
            ),
            (
                "minecraft:blocks/dead_bush".to_string(),
                kind::REPLACE,
                r#"{"pools":[]}"#.to_string()
            ),
        ]
    );
    *crate::fake_rules::FROZEN.lock().unwrap() = true;
    let err = LootTables::add_pool("minecraft:entities/zombie", "{}").unwrap_err();
    assert!(err.to_string().contains("frozen"), "{err}");
}

#[test]
fn a_pool_of_one_item_writes_data_pack_json() {
    assert_eq!(
        LootPool::item("m:ruby").to_json(),
        r#"{"rolls":1,"entries":[{"type":"minecraft:item","name":"m:ruby"}]}"#
    );
    assert_eq!(
        LootPool::item("m:ruby")
            .rolls(2)
            .count(1, 3)
            .chance(0.5)
            .to_json(),
        concat!(
            r#"{"rolls":2,"entries":[{"type":"minecraft:item","name":"m:ruby","functions":[{"function":"minecraft:set_count","#,
            r#""count":{"type":"minecraft:uniform","min":1,"max":3}}]}],"conditions":[{"condition":"minecraft:random_chance","chance":0.5}]}"#
        )
    );
}
