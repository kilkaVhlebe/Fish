//! Host side of `fish:content`, `fish:sound` and `fish:particle`, and the loader's content
//! operations the hooks use.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use fish_abi::content::{
    ContentServiceV1, FREQUENCY_MAX, FUEL_TICKS_MAX, ODDS_MAX, path_type, transform,
};
use fish_abi::particles::{ParticleServiceV1, REGISTRY as PARTICLE_REGISTRY};
use fish_abi::sounds::{GAIN_MAX, REGISTRY as SOUND_REGISTRY, SoundServiceV1};
use fish_abi::{FishHandle, FishServiceHeader, FishStatus, FishStr};
use fish_hooks::{
    CompostDef, ContentTables, FlammableDef, FuelDef, ParticleDef, PathTypeDef, SculkFrequencyDef,
    SoundDef, TransformDef, TransformKind,
};

use crate::blocks::check_id;
use crate::inner::{Inner, ModRecord};
use crate::items::valid_item_ref;

fn guard(f: impl FnOnce() -> FishStatus) -> FishStatus {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(FishStatus::PANICKED)
}

/// What every registration starts with: the loader, the registering mod, and the freeze check.
///
/// # Safety
/// `host` must be the loader's `Inner` (the ABI contract).
unsafe fn begin<'a>(
    host: *mut c_void,
    token: u64,
    what: &str,
    key: &str,
) -> Result<(&'a Inner, Arc<ModRecord>), FishStatus> {
    // SAFETY: the caller's contract.
    let inner = unsafe { &*host.cast::<Inner>() };
    let Some(rec) = inner.record(token) else {
        return Err(FishStatus::INVALID_ARGUMENT);
    };
    if inner.net.frozen.load(Ordering::Acquire) {
        inner.warn(format!(
            "registries are frozen: the {what} {key} of {} was not added",
            rec.manifest.name
        ));
        return Err(FishStatus::INVALID_ARGUMENT);
    }
    Ok((inner, rec))
}

/// Reads a table key: a valid `namespace:path` string, else the refusal (logged).
///
/// # Safety
/// `text` must be valid for the call.
unsafe fn read_ref<'a>(
    inner: &Inner,
    rec: &ModRecord,
    what: &str,
    text: FishStr,
) -> Result<&'a str, FishStatus> {
    // SAFETY: the caller's contract.
    let Ok(id) = (unsafe { text.as_str() }) else {
        return Err(FishStatus::INVALID_ARGUMENT);
    };
    if !valid_item_ref(id) {
        inner.warn(format!(
            "{} registered the invalid {what} {id:?}: it is not an id (namespace:path)",
            rec.manifest.name
        ));
        return Err(FishStatus::INVALID_ARGUMENT);
    }
    Ok(id)
}

fn refuse(inner: &Inner, rec: &ModRecord, what: &str, key: &str, why: String) -> FishStatus {
    inner.warn(format!(
        "{} registered the invalid {what} {key}: {why}",
        rec.manifest.name
    ));
    FishStatus::INVALID_ARGUMENT
}

/// The status for an insertion: `Err` holds the token of the mod that came first.
fn added(
    inner: &Inner,
    rec: &ModRecord,
    what: &str,
    key: &str,
    result: Result<(), u64>,
) -> FishStatus {
    match result {
        Ok(()) => FishStatus::OK,
        Err(first) => {
            let first = inner
                .record(first)
                .map_or_else(|| "another mod".to_string(), |r| r.manifest.name.clone());
            inner.warn(format!(
                "{} registered the {what} {key}, which {first} already registered",
                rec.manifest.name
            ));
            FishStatus::ALREADY_EXISTS
        }
    }
}

unsafe extern "C" fn fuel(
    host: *mut c_void,
    token: u64,
    item: FishStr,
    burn_ticks: u32,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`; `item` is valid for the call.
        let name = unsafe { item.as_str() }.unwrap_or("?");
        let (inner, rec) = match unsafe { begin(host, token, "fuel", name) } {
            Ok(x) => x,
            Err(status) => return status,
        };
        let item = match unsafe { read_ref(inner, &rec, "fuel", item) } {
            Ok(i) => i,
            Err(status) => return status,
        };
        if burn_ticks > FUEL_TICKS_MAX {
            return refuse(
                inner,
                &rec,
                "fuel",
                item,
                format!("{burn_ticks} ticks are more than {FUEL_TICKS_MAX}"),
            );
        }
        let result = inner
            .content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_fuel(
                FuelDef {
                    item: item.to_string(),
                    mod_id: rec.manifest.id.as_str().to_string(),
                    burn_ticks,
                },
                token,
            );
        added(inner, &rec, "fuel", item, result)
    })
}

unsafe extern "C" fn compostable(
    host: *mut c_void,
    token: u64,
    item: FishStr,
    chance: f32,
) -> FishStatus {
    guard(|| {
        // SAFETY: as in `fuel`.
        let name = unsafe { item.as_str() }.unwrap_or("?");
        let (inner, rec) = match unsafe { begin(host, token, "compostable item", name) } {
            Ok(x) => x,
            Err(status) => return status,
        };
        let item = match unsafe { read_ref(inner, &rec, "compostable item", item) } {
            Ok(i) => i,
            Err(status) => return status,
        };
        // Whole percent: Horizon's layers provider is a weighted list.
        if !(chance == 0.0 || (0.01..=1.0).contains(&chance)) {
            return refuse(
                inner,
                &rec,
                "compostable item",
                item,
                format!("the chance {chance} is not 0 or between 0.01 and 1"),
            );
        }
        let result = inner
            .content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_compostable(
                CompostDef {
                    item: item.to_string(),
                    mod_id: rec.manifest.id.as_str().to_string(),
                    chance,
                },
                token,
            );
        added(inner, &rec, "compostable item", item, result)
    })
}

unsafe extern "C" fn flammable(
    host: *mut c_void,
    token: u64,
    block: FishStr,
    ignite_odds: i32,
    burn_odds: i32,
) -> FishStatus {
    guard(|| {
        // SAFETY: as in `fuel`.
        let name = unsafe { block.as_str() }.unwrap_or("?");
        let (inner, rec) = match unsafe { begin(host, token, "flammable block", name) } {
            Ok(x) => x,
            Err(status) => return status,
        };
        let block = match unsafe { read_ref(inner, &rec, "flammable block", block) } {
            Ok(b) => b,
            Err(status) => return status,
        };
        if !(0..=ODDS_MAX).contains(&ignite_odds) || !(0..=ODDS_MAX).contains(&burn_odds) {
            return refuse(
                inner,
                &rec,
                "flammable block",
                block,
                format!("the odds {ignite_odds} and {burn_odds} are not between 0 and {ODDS_MAX}"),
            );
        }
        let result = inner
            .content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_flammable(
                FlammableDef {
                    block: block.to_string(),
                    mod_id: rec.manifest.id.as_str().to_string(),
                    ignite_odds,
                    burn_odds,
                },
                token,
            );
        added(inner, &rec, "flammable block", block, result)
    })
}

unsafe extern "C" fn transform_action(
    host: *mut c_void,
    token: u64,
    kind: u8,
    from: FishStr,
    to: FishStr,
) -> FishStatus {
    guard(|| {
        // SAFETY: as in `fuel`.
        let name = unsafe { from.as_str() }.unwrap_or("?");
        let (inner, rec) = match unsafe { begin(host, token, "tool action", name) } {
            Ok(x) => x,
            Err(status) => return status,
        };
        let from = match unsafe { read_ref(inner, &rec, "tool action", from) } {
            Ok(b) => b,
            Err(status) => return status,
        };
        let to = match unsafe { read_ref(inner, &rec, "tool action", to) } {
            Ok(b) => b,
            Err(status) => return status,
        };
        let Some(kind) = TransformKind::from_u8(kind) else {
            return refuse(
                inner,
                &rec,
                "tool action",
                from,
                format!("the kind {kind} is not one of 0..{}", transform::COUNT),
            );
        };
        if from == to {
            return refuse(
                inner,
                &rec,
                "tool action",
                from,
                "the block does not change".to_string(),
            );
        }
        let result = inner
            .content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_transform(
                TransformDef {
                    kind,
                    from: from.to_string(),
                    to: to.to_string(),
                    mod_id: rec.manifest.id.as_str().to_string(),
                },
                token,
            );
        added(
            inner,
            &rec,
            &format!("{} action of", kind.name()),
            from,
            result,
        )
    })
}

unsafe extern "C" fn path_type_of(
    host: *mut c_void,
    token: u64,
    block: FishStr,
    value: u8,
) -> FishStatus {
    guard(|| {
        // SAFETY: as in `fuel`.
        let name = unsafe { block.as_str() }.unwrap_or("?");
        let (inner, rec) = match unsafe { begin(host, token, "path type of", name) } {
            Ok(x) => x,
            Err(status) => return status,
        };
        let block = match unsafe { read_ref(inner, &rec, "path type of", block) } {
            Ok(b) => b,
            Err(status) => return status,
        };
        let Some(path) = path_type::NAMES.get(usize::from(value)) else {
            return refuse(
                inner,
                &rec,
                "path type of",
                block,
                format!(
                    "the path type {value} is not one of 0..{}",
                    path_type::COUNT
                ),
            );
        };
        let result = inner
            .content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_path_type(
                PathTypeDef {
                    block: block.to_string(),
                    mod_id: rec.manifest.id.as_str().to_string(),
                    path_type: (*path).to_string(),
                },
                token,
            );
        added(inner, &rec, "path type of", block, result)
    })
}

unsafe extern "C" fn sculk_frequency(
    host: *mut c_void,
    token: u64,
    event: FishStr,
    frequency: u8,
) -> FishStatus {
    guard(|| {
        // SAFETY: as in `fuel`.
        let name = unsafe { event.as_str() }.unwrap_or("?");
        let (inner, rec) = match unsafe { begin(host, token, "vibration frequency of", name) } {
            Ok(x) => x,
            Err(status) => return status,
        };
        let event = match unsafe { read_ref(inner, &rec, "vibration frequency of", event) } {
            Ok(e) => e,
            Err(status) => return status,
        };
        if frequency > FREQUENCY_MAX {
            return refuse(
                inner,
                &rec,
                "vibration frequency of",
                event,
                format!("the frequency {frequency} is more than {FREQUENCY_MAX}"),
            );
        }
        let result = inner
            .content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_sculk(
                SculkFrequencyDef {
                    event: event.to_string(),
                    mod_id: rec.manifest.id.as_str().to_string(),
                    frequency,
                },
                token,
            );
        added(inner, &rec, "vibration frequency of", event, result)
    })
}

/// Checks the id of a sound event or particle type: `namespace:path` of the mod's own namespace.
fn check_own(inner: &Inner, rec: &ModRecord, what: &str, id: &str) -> Result<(), FishStatus> {
    if id.starts_with("minecraft:") {
        return Err(refuse(
            inner,
            rec,
            what,
            id,
            "the minecraft namespace belongs to the game".to_string(),
        ));
    }
    check_id(id, rec.manifest.id.as_str()).map_err(|why| refuse(inner, rec, what, id, why))
}

/// Gives `id` a raw id in the Fish registry `registry` (synchronised for mods that run on both
/// sides, like every registry entry).
fn raw_id(inner: &Inner, rec: &ModRecord, registry: &str, id: &str) {
    let synced = rec.manifest.environment == fish_metadata::Environment::Any;
    inner
        .net
        .registries
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .register(registry, id, synced, rec.token);
}

unsafe extern "C" fn sound_register(host: *mut c_void, token: u64, id: FishStr) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`; `id` is valid for the call.
        let Ok(id) = (unsafe { id.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let (inner, rec) = match unsafe { begin(host, token, "sound event", id) } {
            Ok(x) => x,
            Err(status) => return status,
        };
        if let Err(status) = check_own(inner, &rec, "sound event", id) {
            return status;
        }
        let result = inner
            .content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_sound(
                SoundDef {
                    id: id.to_string(),
                    mod_id: rec.manifest.id.as_str().to_string(),
                },
                token,
            );
        let status = added(inner, &rec, "sound event", id, result);
        if status.is_ok() {
            raw_id(inner, &rec, SOUND_REGISTRY, id);
        }
        status
    })
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn sound_play(
    host: *mut c_void,
    view: FishHandle,
    id: FishStr,
    positional: u8,
    x: f64,
    y: f64,
    z: f64,
    volume: f32,
    pitch: f32,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`; `id` is valid for the call.
        let inner = unsafe { &*host.cast::<Inner>() };
        let Ok(id) = (unsafe { id.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let gain = |v: f32| v.is_finite() && v > 0.0 && v <= GAIN_MAX;
        let at = [x, y, z];
        if !valid_item_ref(id)
            || !gain(volume)
            || !gain(pitch)
            || (positional != 0 && !at.iter().all(|c| c.is_finite()))
        {
            return FishStatus::INVALID_ARGUMENT;
        }
        let at = (positional != 0).then_some(at);
        let played = inner.handles.client(view, |c| {
            if c.role().is_server() {
                Err(FishStatus::UNSUPPORTED)
            } else if c.play_sound(id, at, volume, pitch) {
                Ok(())
            } else {
                Err(FishStatus::NOT_FOUND)
            }
        });
        match played {
            Ok(Ok(())) => FishStatus::OK,
            Ok(Err(status)) | Err(status) => status,
        }
    })
}

unsafe extern "C" fn particle_register(
    host: *mut c_void,
    token: u64,
    id: FishStr,
    always_show: u8,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`; `id` is valid for the call.
        let Ok(id) = (unsafe { id.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let (inner, rec) = match unsafe { begin(host, token, "particle type", id) } {
            Ok(x) => x,
            Err(status) => return status,
        };
        if let Err(status) = check_own(inner, &rec, "particle type", id) {
            return status;
        }
        let result = inner
            .content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_particle(
                ParticleDef {
                    id: id.to_string(),
                    mod_id: rec.manifest.id.as_str().to_string(),
                    always_show: always_show != 0,
                },
                token,
            );
        let status = added(inner, &rec, "particle type", id, result);
        if status.is_ok() {
            raw_id(inner, &rec, PARTICLE_REGISTRY, id);
        }
        status
    })
}

impl Inner {
    /// True unless the mod `token` stopped.
    fn content_live(&self, token: u64) -> bool {
        !self.record(token).is_some_and(|r| r.is_failed())
    }

    /// The content table entries of the mods that did not fail.
    pub(crate) fn content_tables(&self) -> ContentTables {
        self.content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tables(|o| self.content_live(o))
    }

    /// The vibration frequency a live mod gave `event`.
    pub(crate) fn sculk_frequency(&self, event: &str) -> Option<u8> {
        self.content
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sculk_frequency(event, |o| self.content_live(o))
    }

    /// The sound events of the mods that did not fail.
    pub(crate) fn sound_events(&self, out: &mut Vec<SoundDef>) {
        out.extend(
            self.content
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .sounds(|o| self.content_live(o)),
        );
    }

    /// The particle types of the mods that did not fail.
    pub(crate) fn particle_types(&self, out: &mut Vec<ParticleDef>) {
        out.extend(
            self.content
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .particles(|o| self.content_live(o)),
        );
    }

    /// What the mods registered, in one line; `None` when it is nothing.
    pub(crate) fn content_summary(&self) -> Option<String> {
        let tables = self.content_tables();
        let (mut sounds, mut particles) = (Vec::new(), Vec::new());
        self.sound_events(&mut sounds);
        self.particle_types(&mut particles);
        if tables.is_empty() && sounds.is_empty() && particles.is_empty() {
            return None;
        }
        Some(format!(
            "Fish content tables: {}",
            crate::tables::summary(&tables, sounds.len(), particles.len())
        ))
    }

    /// Logs what the mods registered (once, when the entrypoints are over).
    pub(crate) fn log_content(&self) {
        if let Some(line) = self.content_summary() {
            self.emit(log::Level::Info, "fish_loader", line);
        }
    }
}

/// The `fish:content` table.
pub(crate) static CONTENT: ContentServiceV1 = ContentServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ContentServiceV1>(),
        version: 1,
    },
    fuel,
    compostable,
    flammable,
    transform: transform_action,
    path_type: path_type_of,
    sculk_frequency,
};

/// The `fish:sound` table.
pub(crate) static SOUND: SoundServiceV1 = SoundServiceV1 {
    header: FishServiceHeader {
        size: size_of::<SoundServiceV1>(),
        version: 1,
    },
    register: sound_register,
    play: sound_play,
};

/// The `fish:particle` table.
pub(crate) static PARTICLE: ParticleServiceV1 = ParticleServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ParticleServiceV1>(),
        version: 1,
    },
    register: particle_register,
};

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use fish_hooks::{ClientView, PlayerSnapshot, Role};
    use fish_metadata::Side;

    use super::*;
    use crate::boot::{BootConfig, Loader, boot};

    /// A booted loader with the mods `ruby` and `gem`, and its host pointer.
    fn host() -> (Loader, *mut c_void, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("fish-mods")).unwrap();
        for (id, name) in [("ruby", "Ruby"), ("gem", "Gem")] {
            let spec = fish_pack::PackSpec::new(format!(
                "schema = 1\nid = \"{id}\"\nversion = \"1.2.3\"\nname = \"{name}\"\ndescription = \"About {name}\"\n"
            ));
            fish_pack::write(
                spec,
                &dir.path().join("fish-mods").join(format!("{id}.fish")),
            )
            .unwrap();
        }
        let mut cfg = BootConfig::new(Side::Client, dir.path());
        cfg.log_sink = Some(Arc::new(|_| {}));
        let loader = boot(cfg).unwrap();
        // Entrypoints are over, so the registries are frozen: open them for these tests.
        loader.inner.net.frozen.store(false, Ordering::Release);
        let ptr = Arc::as_ptr(&loader.inner).cast_mut().cast();
        (loader, ptr, dir)
    }

    fn token(loader: &Loader, id: &str) -> u64 {
        loader
            .inner
            .records()
            .iter()
            .find(|r| r.manifest.id.as_str() == id)
            .unwrap()
            .token
    }

    fn warned(loader: &Loader, text: &str) -> bool {
        loader
            .inner
            .warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w.contains(text))
    }

    fn s(text: &str) -> FishStr {
        FishStr::new(text)
    }

    #[test]
    fn a_fuel_is_stored_with_its_mod_and_a_second_one_is_refused() {
        let (loader, h, _d) = host();
        let (ruby, gem) = (token(&loader, "ruby"), token(&loader, "gem"));
        // SAFETY: a live host; the strings outlive the calls.
        unsafe {
            assert_eq!(fuel(h, ruby, s("ruby:gem"), 1600), FishStatus::OK);
            assert_eq!(fuel(h, ruby, s("minecraft:stick"), 0), FishStatus::OK);
            assert_eq!(fuel(h, gem, s("ruby:gem"), 100), FishStatus::ALREADY_EXISTS);
            assert_eq!(
                fuel(h, ruby, s("no namespace"), 100),
                FishStatus::INVALID_ARGUMENT
            );
            assert_eq!(
                fuel(h, ruby, s("ruby:x"), FUEL_TICKS_MAX + 1),
                FishStatus::INVALID_ARGUMENT
            );
        }
        assert!(warned(&loader, "which Ruby already registered"));
        assert!(warned(&loader, "not an id"));
        assert!(warned(&loader, "more than 1000000"));
        let fuels = loader.inner.content_tables().fuels;
        assert_eq!(fuels.len(), 2);
        assert_eq!(
            (fuels[0].mod_id.as_str(), fuels[0].burn_ticks),
            ("ruby", 1600)
        );
        assert_eq!(fuels[1].burn_ticks, 0);
    }

    #[test]
    fn compost_chances_are_zero_or_whole_percents() {
        let (loader, h, _d) = host();
        let ruby = token(&loader, "ruby");
        // SAFETY: a live host; the strings outlive the calls.
        unsafe {
            assert_eq!(compostable(h, ruby, s("ruby:a"), 0.3), FishStatus::OK);
            assert_eq!(compostable(h, ruby, s("ruby:b"), 1.0), FishStatus::OK);
            assert_eq!(compostable(h, ruby, s("ruby:c"), 0.0), FishStatus::OK);
            for bad in [0.001, 1.5, -0.1, f32::NAN] {
                assert_eq!(
                    compostable(h, ruby, s("ruby:d"), bad),
                    FishStatus::INVALID_ARGUMENT,
                    "{bad}"
                );
            }
        }
        assert_eq!(loader.inner.content_tables().compostables.len(), 3);
    }

    #[test]
    fn fire_odds_and_path_types_and_frequencies_are_checked() {
        let (loader, h, _d) = host();
        let ruby = token(&loader, "ruby");
        // SAFETY: a live host; the strings outlive the calls.
        unsafe {
            assert_eq!(flammable(h, ruby, s("ruby:planks"), 5, 20), FishStatus::OK);
            assert_eq!(
                flammable(h, ruby, s("ruby:planks"), 5, 20),
                FishStatus::ALREADY_EXISTS
            );
            assert_eq!(
                flammable(h, ruby, s("ruby:x"), -1, 0),
                FishStatus::INVALID_ARGUMENT
            );
            assert_eq!(
                flammable(h, ruby, s("ruby:x"), 0, ODDS_MAX + 1),
                FishStatus::INVALID_ARGUMENT
            );
            assert_eq!(path_type_of(h, ruby, s("ruby:thorns"), 16), FishStatus::OK);
            assert_eq!(
                path_type_of(h, ruby, s("ruby:y"), path_type::COUNT),
                FishStatus::INVALID_ARGUMENT
            );
            assert_eq!(
                sculk_frequency(h, ruby, s("minecraft:step"), 7),
                FishStatus::OK
            );
            assert_eq!(
                sculk_frequency(h, ruby, s("minecraft:swim"), 16),
                FishStatus::INVALID_ARGUMENT
            );
        }
        let tables = loader.inner.content_tables();
        assert_eq!(tables.flammables[0].burn_odds, 20);
        assert_eq!(tables.path_types[0].path_type, "DAMAGING");
        assert_eq!(loader.inner.sculk_frequency("minecraft:step"), Some(7));
        assert_eq!(loader.inner.sculk_frequency("minecraft:swim"), None);
    }

    #[test]
    fn tool_actions_need_two_different_blocks_and_a_known_kind() {
        let (loader, h, _d) = host();
        let ruby = token(&loader, "ruby");
        // SAFETY: a live host; the strings outlive the calls.
        unsafe {
            assert_eq!(
                transform_action(h, ruby, transform::STRIP, s("ruby:log"), s("ruby:bare")),
                FishStatus::OK
            );
            assert_eq!(
                transform_action(h, ruby, transform::WAX, s("ruby:log"), s("ruby:waxed")),
                FishStatus::OK
            );
            assert_eq!(
                transform_action(h, ruby, transform::STRIP, s("ruby:log"), s("ruby:other")),
                FishStatus::ALREADY_EXISTS
            );
            assert_eq!(
                transform_action(h, ruby, transform::COUNT, s("ruby:a"), s("ruby:b")),
                FishStatus::INVALID_ARGUMENT
            );
            assert_eq!(
                transform_action(h, ruby, transform::TILL, s("ruby:a"), s("ruby:a")),
                FishStatus::INVALID_ARGUMENT
            );
            assert_eq!(
                transform_action(h, ruby, transform::TILL, s("ruby:a"), s("bad")),
                FishStatus::INVALID_ARGUMENT
            );
        }
        let kinds: Vec<_> = loader
            .inner
            .content_tables()
            .transforms
            .iter()
            .map(|t| t.kind)
            .collect();
        assert_eq!(kinds, [TransformKind::Strip, TransformKind::Wax]);
    }

    #[test]
    fn registrations_after_the_freeze_and_by_unknown_mods_are_refused() {
        let (loader, h, _d) = host();
        let ruby = token(&loader, "ruby");
        // SAFETY: a live host; the strings outlive the calls.
        unsafe {
            assert_eq!(fuel(h, 9_999, s("ruby:a"), 1), FishStatus::INVALID_ARGUMENT);
            loader.inner.net.frozen.store(true, Ordering::Release);
            assert_eq!(fuel(h, ruby, s("ruby:a"), 1), FishStatus::INVALID_ARGUMENT);
            assert_eq!(
                sound_register(h, ruby, s("ruby:late")),
                FishStatus::INVALID_ARGUMENT
            );
            assert_eq!(
                particle_register(h, ruby, s("ruby:late"), 0),
                FishStatus::INVALID_ARGUMENT
            );
        }
        assert!(warned(&loader, "registries are frozen: the fuel ruby:a"));
        assert!(loader.inner.content_tables().is_empty());
    }

    #[test]
    fn the_entries_of_a_stopped_mod_are_left_out() {
        let (loader, h, _d) = host();
        let (ruby, gem) = (token(&loader, "ruby"), token(&loader, "gem"));
        // SAFETY: a live host; the strings outlive the calls.
        unsafe {
            assert_eq!(fuel(h, ruby, s("ruby:a"), 5), FishStatus::OK);
            assert_eq!(fuel(h, gem, s("gem:a"), 5), FishStatus::OK);
            assert_eq!(
                sculk_frequency(h, gem, s("minecraft:step"), 3),
                FishStatus::OK
            );
            assert_eq!(sound_register(h, gem, s("gem:ping")), FishStatus::OK);
            assert_eq!(particle_register(h, gem, s("gem:spark"), 1), FishStatus::OK);
        }
        loader.inner.fail_mod(gem, "test".to_string(), false);
        let fuels = loader.inner.content_tables().fuels;
        assert_eq!(fuels.len(), 1);
        assert_eq!(fuels[0].mod_id, "ruby");
        assert_eq!(loader.inner.sculk_frequency("minecraft:step"), None);
        let (mut sounds, mut particles) = (Vec::new(), Vec::new());
        loader.inner.sound_events(&mut sounds);
        loader.inner.particle_types(&mut particles);
        assert!(sounds.is_empty() && particles.is_empty());
    }

    #[test]
    fn sound_events_and_particle_types_take_the_mods_namespace_and_a_raw_id() {
        let (loader, h, _d) = host();
        let (ruby, gem) = (token(&loader, "ruby"), token(&loader, "gem"));
        // SAFETY: a live host; the strings outlive the calls.
        unsafe {
            assert_eq!(sound_register(h, ruby, s("ruby:chime")), FishStatus::OK);
            assert_eq!(sound_register(h, ruby, s("ruby:deep/hum")), FishStatus::OK);
            assert_eq!(
                sound_register(h, ruby, s("ruby:chime")),
                FishStatus::ALREADY_EXISTS
            );
            assert_eq!(
                sound_register(h, ruby, s("gem:chime")),
                FishStatus::INVALID_ARGUMENT
            );
            assert_eq!(
                sound_register(h, ruby, s("minecraft:chime")),
                FishStatus::INVALID_ARGUMENT
            );
            assert_eq!(
                sound_register(h, ruby, s("ruby:Bad")),
                FishStatus::INVALID_ARGUMENT
            );
            assert_eq!(particle_register(h, gem, s("gem:spark"), 1), FishStatus::OK);
            assert_eq!(
                particle_register(h, gem, s("gem:spark"), 0),
                FishStatus::ALREADY_EXISTS
            );
            assert_eq!(
                particle_register(h, gem, s("ruby:spark"), 0),
                FishStatus::INVALID_ARGUMENT
            );
        }
        assert_eq!(
            loader.registry_entries(SOUND_REGISTRY),
            [
                ("ruby:chime".to_string(), 0),
                ("ruby:deep/hum".to_string(), 1)
            ]
        );
        assert_eq!(
            loader.registry_entries(PARTICLE_REGISTRY),
            [("gem:spark".to_string(), 0)]
        );
        let mut particles = Vec::new();
        loader.inner.particle_types(&mut particles);
        assert!(particles[0].always_show);
        assert!(loader.inner.content.lock().unwrap().has_assets_of(ruby));
    }

    /// A sound a game played: event, position, volume and pitch.
    type Played = (String, Option<[f64; 3]>, f32, f32);

    /// A game that plays sounds, or one that has no audio.
    struct Game {
        role: Role,
        known: &'static str,
        played: Arc<Mutex<Vec<Played>>>,
    }

    impl ClientView for Game {
        fn role(&self) -> Role {
            self.role
        }
        fn player(&self) -> Option<PlayerSnapshot> {
            None
        }
        fn show_chat(&mut self, _: &str) {}
        fn game_dir(&self) -> std::path::PathBuf {
            std::path::PathBuf::new()
        }
        fn play_sound(
            &mut self,
            event: &str,
            at: Option<[f64; 3]>,
            volume: f32,
            pitch: f32,
        ) -> bool {
            self.played
                .lock()
                .unwrap()
                .push((event.to_string(), at, volume, pitch));
            event == self.known
        }
    }

    #[test]
    fn playing_a_sound_reaches_the_game_and_reports_what_it_could_not_do() {
        let (loader, h, _d) = host();
        let played = Arc::new(Mutex::new(Vec::new()));
        let mut game = Game {
            role: Role::Window,
            known: "ruby:chime",
            played: played.clone(),
        };
        let status = loader.inner.handles.client_scope(&mut game, |view| {
            // SAFETY: a live host and a live view; the strings outlive the calls.
            unsafe {
                let play = |id: &str, pos: u8, volume: f32, pitch: f32| {
                    sound_play(h, view, s(id), pos, 1.0, 64.0, -3.5, volume, pitch)
                };
                [
                    play("ruby:chime", 1, 0.5, 2.0),
                    play("ruby:chime", 0, 1.0, 1.0),
                    play("ruby:unknown", 0, 1.0, 1.0),
                    play("no namespace", 0, 1.0, 1.0),
                    play("ruby:chime", 0, 0.0, 1.0),
                    play("ruby:chime", 0, 1.0, GAIN_MAX + 1.0),
                    play("ruby:chime", 0, f32::NAN, 1.0),
                ]
            }
        });
        assert_eq!(
            status,
            [
                FishStatus::OK,
                FishStatus::OK,
                FishStatus::NOT_FOUND,
                FishStatus::INVALID_ARGUMENT,
                FishStatus::INVALID_ARGUMENT,
                FishStatus::INVALID_ARGUMENT,
                FishStatus::INVALID_ARGUMENT
            ]
        );
        let seen = played.lock().unwrap().clone();
        assert_eq!(seen.len(), 3);
        assert_eq!(
            seen[0],
            ("ruby:chime".to_string(), Some([1.0, 64.0, -3.5]), 0.5, 2.0)
        );
        assert_eq!(seen[1].1, None);

        let mut server = Game {
            role: Role::IntegratedServer,
            known: "ruby:chime",
            played: played.clone(),
        };
        let status = loader.inner.handles.client_scope(&mut server, |view| {
            // SAFETY: as above.
            unsafe { sound_play(h, view, s("ruby:chime"), 0, 0.0, 0.0, 0.0, 1.0, 1.0) }
        });
        assert_eq!(status, FishStatus::UNSUPPORTED);
        assert_eq!(played.lock().unwrap().len(), 3);
        // A handle that is not live.
        // SAFETY: as above.
        let stale = unsafe {
            sound_play(
                h,
                FishHandle {
                    index: 99,
                    generation: 1,
                },
                s("ruby:chime"),
                0,
                0.0,
                0.0,
                0.0,
                1.0,
                1.0,
            )
        };
        assert_eq!(stale, FishStatus::STALE_HANDLE);
    }

    #[test]
    fn the_boot_log_names_the_content_the_mods_registered() {
        let (loader, h, _d) = host();
        let ruby = token(&loader, "ruby");
        assert_eq!(loader.inner.content_summary(), None);
        // SAFETY: a live host; the strings outlive the calls.
        unsafe {
            assert_eq!(fuel(h, ruby, s("ruby:a"), 5), FishStatus::OK);
            assert_eq!(sound_register(h, ruby, s("ruby:ping")), FishStatus::OK);
        }
        let line = loader.inner.content_summary().unwrap();
        assert!(
            line.starts_with("Fish content tables: 1 fuel(s), 0 compostable"),
            "{line}"
        );
        assert!(
            line.ends_with("1 sound event(s), 0 particle type(s)"),
            "{line}"
        );
        loader.inner.log_content();
    }
}
