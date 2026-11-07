//! Host side of `fish:loot`: the changes mods make to loot tables.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;

use fish_abi::loot::{LootServiceV1, MAX_JSON, kind};
use fish_abi::{FishServiceHeader, FishStatus, FishStr};
use fish_hooks::{LootChange, LootChangeKind};
use serde_json::Value;

use crate::inner::Inner;
use crate::items::valid_item_ref;

fn guard(f: impl FnOnce() -> FishStatus) -> FishStatus {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(FishStatus::PANICKED)
}

/// Why the JSON of a change of `kind` is not what the kind takes.
fn check_json(kind: LootChangeKind, json: &str) -> Result<(), String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("not JSON: {e}"))?;
    match (kind, &value) {
        (LootChangeKind::AddPools, Value::Object(_)) => Ok(()),
        (LootChangeKind::AddPools, Value::Array(pools)) if pools.iter().all(Value::is_object) => {
            Ok(())
        }
        (LootChangeKind::AddPools, _) => Err("pools are an object or a list of objects".into()),
        (LootChangeKind::Replace, Value::Object(_)) => Ok(()),
        (LootChangeKind::Replace, _) => Err("a loot table is an object".into()),
    }
}

unsafe extern "C" fn modify(
    host: *mut c_void,
    token: u64,
    table: FishStr,
    change: u8,
    json: FishStr,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`; the strings are valid for the call.
        let inner = unsafe { &*host.cast::<Inner>() };
        let (Ok(table), Ok(json)) = (unsafe { table.as_str() }, unsafe { json.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let Some(rec) = inner.record(token) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let who = rec.manifest.name.clone();
        let refuse = |why: String| {
            inner.warn(format!(
                "{who} changed the loot table {table:?}, refused: {why}"
            ));
            FishStatus::INVALID_ARGUMENT
        };
        if inner.net.frozen.load(Ordering::Acquire) {
            return refuse("registries are frozen".into());
        }
        if !valid_item_ref(table) {
            return refuse("it is not an id (namespace:path)".into());
        }
        let kind = match change {
            kind::ADD_POOLS => LootChangeKind::AddPools,
            kind::REPLACE => LootChangeKind::Replace,
            other => return refuse(format!("unknown kind {other}")),
        };
        if json.len() > MAX_JSON {
            return refuse("the JSON is over 1 MiB".into());
        }
        if let Err(why) = check_json(kind, json) {
            return refuse(why);
        }
        let added = inner
            .content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_loot(
                LootChange {
                    table: table.to_string(),
                    mod_id: rec.manifest.id.as_str().to_string(),
                    kind,
                    json: json.to_string(),
                },
                token,
            );
        match added {
            Ok(()) => FishStatus::OK,
            Err(first) => {
                let first = inner
                    .record(first)
                    .map_or_else(|| "another mod".to_string(), |r| r.manifest.name.clone());
                inner.warn(format!(
                    "{who} replaced the loot table {table}, which {first} already replaced"
                ));
                FishStatus::ALREADY_EXISTS
            }
        }
    })
}

impl Inner {
    /// The loot table changes of the mods that did not fail.
    pub(crate) fn loot_changes(&self, out: &mut Vec<LootChange>) {
        out.extend(
            self.content
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .loot(|o| !self.record(o).is_some_and(|r| r.is_failed())),
        );
    }

    /// Logs the loot table changes the mods registered (once, when the entrypoints are over).
    pub(crate) fn log_loot(&self) {
        let mut changes = Vec::new();
        self.loot_changes(&mut changes);
        if !changes.is_empty() {
            self.emit(
                log::Level::Info,
                "fish_loader",
                format!("Fish loot tables: {} change(s)", changes.len()),
            );
        }
    }
}

/// The `fish:loot` table.
pub(crate) static LOOT: LootServiceV1 = LootServiceV1 {
    header: FishServiceHeader {
        size: size_of::<LootServiceV1>(),
        version: 1,
    },
    modify,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> (crate::boot::Loader, *mut c_void, u64, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("fish-mods");
        std::fs::create_dir_all(&mods).unwrap();
        let spec = fish_pack::PackSpec::new(
            "schema = 1\nid = \"ruby\"\nversion = \"1.0.0\"\nname = \"Ruby\"\n".to_string(),
        );
        fish_pack::write(spec, &mods.join("ruby.fish")).unwrap();
        let mut cfg = crate::boot::BootConfig::new(fish_metadata::Side::Client, dir.path());
        cfg.log_sink = Some(std::sync::Arc::new(|_| {}));
        let loader = crate::boot::boot(cfg).unwrap();
        loader.inner.net.frozen.store(false, Ordering::Release);
        let token = loader.inner.records()[0].token;
        let ptr = std::sync::Arc::as_ptr(&loader.inner)
            .cast_mut()
            .cast::<c_void>();
        (loader, ptr, token, dir)
    }

    #[test]
    fn changes_are_checked_kept_in_order_and_frozen() {
        let (loader, h, t, _dir) = host();
        let call = |table: &str, k: u8, json: &str| {
            // SAFETY: the table's own function with valid arguments.
            unsafe { modify(h, t, FishStr::new(table), k, FishStr::new(json)) }
        };
        let stone = "minecraft:blocks/stone";
        assert_eq!(
            call(stone, kind::ADD_POOLS, r#"{"rolls": 1}"#),
            FishStatus::OK
        );
        assert_eq!(
            call(stone, kind::REPLACE, r#"{"pools": []}"#),
            FishStatus::OK
        );
        assert_eq!(
            call(stone, kind::REPLACE, r#"{"pools": []}"#),
            FishStatus::ALREADY_EXISTS
        );
        assert_eq!(
            call(stone, kind::ADD_POOLS, r#"[{"rolls": 2}]"#),
            FishStatus::OK
        );
        for (table, k, json) in [
            ("Stone", kind::ADD_POOLS, "{}"),
            (stone, 7, "{}"),
            (stone, kind::ADD_POOLS, "nope"),
            (stone, kind::REPLACE, "[]"),
        ] {
            assert_eq!(
                call(table, k, json),
                FishStatus::INVALID_ARGUMENT,
                "{table} {k} {json}"
            );
        }
        // SAFETY: as above, with a token no mod has.
        let stranger = unsafe {
            modify(
                h,
                999,
                FishStr::new(stone),
                kind::ADD_POOLS,
                FishStr::new("{}"),
            )
        };
        assert_eq!(stranger, FishStatus::INVALID_ARGUMENT);
        let mut out = Vec::new();
        loader.inner.loot_changes(&mut out);
        let kinds: Vec<LootChangeKind> = out.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [
                LootChangeKind::AddPools,
                LootChangeKind::Replace,
                LootChangeKind::AddPools
            ]
        );
        assert_eq!(out[0].mod_id, "ruby");
        loader.inner.log_loot();
        loader.inner.net.frozen.store(true, Ordering::Release);
        assert_eq!(
            call(stone, kind::ADD_POOLS, "{}"),
            FishStatus::INVALID_ARGUMENT
        );
    }
    #[test]
    fn the_json_must_fit_the_kind() {
        assert!(check_json(LootChangeKind::AddPools, r#"{"rolls": 1}"#).is_ok());
        assert!(check_json(LootChangeKind::AddPools, r#"[{"rolls": 1}, {}]"#).is_ok());
        assert!(check_json(LootChangeKind::AddPools, r#"[1]"#).is_err());
        assert!(check_json(LootChangeKind::AddPools, r#"3"#).is_err());
        assert!(check_json(LootChangeKind::Replace, r#"{"pools": []}"#).is_ok());
        assert!(check_json(LootChangeKind::Replace, r#"[]"#).is_err());
        assert!(
            check_json(LootChangeKind::Replace, "{")
                .unwrap_err()
                .starts_with("not JSON")
        );
    }
}
