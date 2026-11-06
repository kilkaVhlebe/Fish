//! Fake `fish:content`, `fish:sound` and `fish:particle` services, and the tests of `content`,
//! `sound` and `particle` built on them.

use std::ffi::c_void;
use std::sync::Mutex;

use fish_abi::content::{ContentServiceV1, SERVICE_CONTENT, path_type, transform};
use fish_abi::particles::{ParticleServiceV1, SERVICE_PARTICLE};
use fish_abi::sounds::{SERVICE_SOUND, SoundServiceV1};
use fish_abi::{FishHandle, FishServiceHeader, FishStatus, FishStr};

use crate::client::ClientHandle;
use crate::content::{Content, PathNodeType};
use crate::particle::Particles;
use crate::sound::Sounds;

/// What the fake saw, one line per call.
static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Ids the fake already knows (a second registration is `ALREADY_EXISTS`).
static KNOWN: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The status the next call answers instead of its own.
static FAIL: Mutex<Option<FishStatus>> = Mutex::new(None);

fn text(s: FishStr) -> String {
    // SAFETY: the API passes valid strings.
    unsafe { s.as_str() }.unwrap().to_string()
}

fn record(line: String) -> FishStatus {
    if let Some(status) = FAIL.lock().unwrap().take() {
        return status;
    }
    if *crate::fake_rules::FROZEN.lock().unwrap() {
        return FishStatus::INVALID_ARGUMENT;
    }
    let mut known = KNOWN.lock().unwrap();
    if known.contains(&line) {
        return FishStatus::ALREADY_EXISTS;
    }
    known.push(line.clone());
    CALLS.lock().unwrap().push(line);
    FishStatus::OK
}

unsafe extern "C" fn fuel(_h: *mut c_void, token: u64, item: FishStr, ticks: u32) -> FishStatus {
    record(format!("fuel {token} {} {ticks}", text(item)))
}

unsafe extern "C" fn compostable(
    _h: *mut c_void,
    token: u64,
    item: FishStr,
    chance: f32,
) -> FishStatus {
    record(format!("compost {token} {} {chance}", text(item)))
}

unsafe extern "C" fn flammable(
    _h: *mut c_void,
    token: u64,
    block: FishStr,
    ignite: i32,
    burn: i32,
) -> FishStatus {
    record(format!("flammable {token} {} {ignite} {burn}", text(block)))
}

unsafe extern "C" fn transform_action(
    _h: *mut c_void,
    token: u64,
    kind: u8,
    from: FishStr,
    to: FishStr,
) -> FishStatus {
    record(format!(
        "{} {token} {} {}",
        transform::NAMES[usize::from(kind)],
        text(from),
        text(to)
    ))
}

unsafe extern "C" fn path_type_of(
    _h: *mut c_void,
    token: u64,
    block: FishStr,
    value: u8,
) -> FishStatus {
    record(format!(
        "path {token} {} {}",
        text(block),
        path_type::NAMES[usize::from(value)]
    ))
}

unsafe extern "C" fn sculk(
    _h: *mut c_void,
    token: u64,
    event: FishStr,
    frequency: u8,
) -> FishStatus {
    record(format!("sculk {token} {} {frequency}", text(event)))
}

static CONTENT: ContentServiceV1 = ContentServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ContentServiceV1>(),
        version: 1,
    },
    fuel,
    compostable,
    flammable,
    transform: transform_action,
    path_type: path_type_of,
    sculk_frequency: sculk,
};

unsafe extern "C" fn sound_register(_h: *mut c_void, token: u64, id: FishStr) -> FishStatus {
    record(format!("sound {token} {}", text(id)))
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn sound_play(
    _h: *mut c_void,
    view: FishHandle,
    id: FishStr,
    positional: u8,
    x: f64,
    y: f64,
    z: f64,
    volume: f32,
    pitch: f32,
) -> FishStatus {
    if let Some(status) = FAIL.lock().unwrap().take() {
        return status;
    }
    CALLS.lock().unwrap().push(format!(
        "play {}/{} {} {positional} {x} {y} {z} {volume} {pitch}",
        view.index,
        view.generation,
        text(id)
    ));
    FishStatus::OK
}

static SOUND: SoundServiceV1 = SoundServiceV1 {
    header: FishServiceHeader {
        size: size_of::<SoundServiceV1>(),
        version: 1,
    },
    register: sound_register,
    play: sound_play,
};

unsafe extern "C" fn particle_register(
    _h: *mut c_void,
    token: u64,
    id: FishStr,
    always_show: u8,
) -> FishStatus {
    record(format!("particle {token} {} {always_show}", text(id)))
}

static PARTICLE: ParticleServiceV1 = ParticleServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ParticleServiceV1>(),
        version: 1,
    },
    register: particle_register,
};

/// The table for a service id of this fake.
pub(crate) fn lookup(id: &str) -> Option<*const c_void> {
    Some(match id {
        SERVICE_CONTENT => (&raw const CONTENT).cast(),
        SERVICE_SOUND => (&raw const SOUND).cast(),
        SERVICE_PARTICLE => (&raw const PARTICLE).cast(),
        _ => return None,
    })
}

/// Starts a test with an empty fake.
fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let guard = crate::fake_rules::SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::fake::install();
    CALLS.lock().unwrap().clear();
    KNOWN.lock().unwrap().clear();
    *FAIL.lock().unwrap() = None;
    *crate::fake_rules::FROZEN.lock().unwrap() = false;
    guard
}

fn calls() -> Vec<String> {
    CALLS.lock().unwrap().clone()
}

fn token() -> u64 {
    crate::host::get().unwrap().mod_token
}

#[test]
fn every_content_table_call_reaches_the_service_with_the_mods_token() {
    let _g = fresh();
    Content::fuel("t:ruby", 1600).unwrap();
    Content::remove_fuel("minecraft:stick").unwrap();
    Content::compostable("t:ruby", 0.65).unwrap();
    Content::remove_compostable("minecraft:wheat").unwrap();
    Content::flammable("t:planks", 5, 20).unwrap();
    Content::fireproof("minecraft:oak_planks").unwrap();
    Content::strippable("t:log", "t:bare").unwrap();
    Content::tillable("t:soil", "minecraft:farmland").unwrap();
    Content::flattenable("t:soil", "minecraft:dirt_path").unwrap();
    Content::waxable("t:copper", "t:waxed").unwrap();
    Content::oxidizable("t:copper", "t:exposed").unwrap();
    Content::path_node_type("t:thorns", PathNodeType::Damaging).unwrap();
    Content::sculk_frequency("minecraft:step", 7).unwrap();
    let t = token();
    assert_eq!(
        calls(),
        [
            format!("fuel {t} t:ruby 1600"),
            format!("fuel {t} minecraft:stick 0"),
            format!("compost {t} t:ruby 0.65"),
            format!("compost {t} minecraft:wheat 0"),
            format!("flammable {t} t:planks 5 20"),
            format!("flammable {t} minecraft:oak_planks 0 0"),
            format!("strip {t} t:log t:bare"),
            format!("till {t} t:soil minecraft:farmland"),
            format!("flatten {t} t:soil minecraft:dirt_path"),
            format!("wax {t} t:copper t:waxed"),
            format!("oxidize {t} t:copper t:exposed"),
            format!("path {t} t:thorns DAMAGING"),
            format!("sculk {t} minecraft:step 7"),
        ]
    );
    const { assert!(Content::FUEL_TICKS_MAX >= 1_000_000 && Content::ODDS_MAX >= 100) };
    assert_eq!(Content::FREQUENCY_MAX, 15);
}

#[test]
fn a_refusal_becomes_an_error_and_a_frozen_registry_says_so() {
    let _g = fresh();
    Content::fuel("t:ruby", 100).unwrap();
    let again = Content::fuel("t:ruby", 100).unwrap_err();
    assert_eq!(again.status(), Some(FishStatus::ALREADY_EXISTS));
    *crate::fake_rules::FROZEN.lock().unwrap() = true;
    let late = Content::flammable("t:x", 1, 1).unwrap_err();
    assert_eq!(
        late,
        crate::Error::InvalidArgument("registries are frozen".to_string())
    );
    *crate::fake_rules::FROZEN.lock().unwrap() = false;
    *FAIL.lock().unwrap() = Some(FishStatus::INVALID_ARGUMENT);
    let bad = Content::sculk_frequency("minecraft:step", 99).unwrap_err();
    assert_eq!(bad.status(), Some(FishStatus::INVALID_ARGUMENT));
    assert_eq!(calls().len(), 1);
}

#[test]
fn a_sound_event_is_registered_and_played_in_the_game_that_raised_the_event() {
    let _g = fresh();
    let chime = Sounds::register("t:ruby_chime").unwrap();
    assert_eq!(chime.id(), "t:ruby_chime");
    assert_eq!(
        Sounds::register("t:ruby_chime").unwrap_err().status(),
        Some(FishStatus::ALREADY_EXISTS)
    );
    let client = ClientHandle::from_raw(FishHandle {
        index: 4,
        generation: 2,
    });
    chime.play(&client, 0.5, 2.0).unwrap();
    chime.play_at(&client, [1.5, 64.0, -3.5], 1.0, 1.0).unwrap();
    Sounds::play(
        &client,
        "minecraft:entity.experience_orb.pickup",
        None,
        1.0,
        1.0,
    )
    .unwrap();
    let t = token();
    assert_eq!(
        calls(),
        [
            format!("sound {t} t:ruby_chime"),
            "play 4/2 t:ruby_chime 0 0 0 0 0.5 2".to_string(),
            "play 4/2 t:ruby_chime 1 1.5 64 -3.5 1 1".to_string(),
            "play 4/2 minecraft:entity.experience_orb.pickup 0 0 0 0 1 1".to_string(),
        ]
    );
    *FAIL.lock().unwrap() = Some(FishStatus::NOT_FOUND);
    let unknown = chime.play(&client, 1.0, 1.0).unwrap_err();
    assert_eq!(unknown.status(), Some(FishStatus::NOT_FOUND));
    assert_eq!(Sounds::GAIN_MAX, 10.0);
}

#[test]
fn a_particle_type_is_registered_with_its_flag_and_its_raw_id_comes_from_the_registry() {
    let _g = fresh();
    let spark = Particles::register("t:spark").unwrap();
    let glow = Particles::register_with("t:glow", true).unwrap();
    assert_eq!((spark.id(), glow.id()), ("t:spark", "t:glow"));
    let t = token();
    assert_eq!(
        calls(),
        [
            format!("particle {t} t:spark 0"),
            format!("particle {t} t:glow 1")
        ]
    );
    // The fake registry knows no entries.
    assert_eq!(
        spark.raw_id().unwrap_err().status(),
        Some(FishStatus::NOT_FOUND)
    );
    *crate::fake_rules::FROZEN.lock().unwrap() = true;
    assert_eq!(
        Particles::register("t:late").unwrap_err(),
        crate::Error::InvalidArgument("registries are frozen".to_string())
    );
}
