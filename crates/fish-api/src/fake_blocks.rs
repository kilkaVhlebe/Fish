//! A fake `fish:block` service (the registry freeze comes from `fake_rules`), and the tests of
//! `block` built on it.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fish_abi::blocks::{
    BlockHandlerFn, BlockPayloadV1, BlockServiceV1, BlockSettingsV1, HANDLER_PANICKED,
    SERVICE_BLOCK, event, set,
};
use fish_abi::client::{ROLE_DEDICATED_SERVER, ROLE_INTEGRATED_SERVER, ROLE_WINDOW};
use fish_abi::game::{FACE_NONE, HAND_MAIN, HAND_OFF};
use fish_abi::{ActionResult, DropUserFn, FishHandle, FishServiceHeader, FishStatus, FishStr};

use crate::block::{BlockBehaviour, BlockContext, BlockSettings, Blocks};
use crate::client::{ClientHandle, Role};
use crate::game::{BlockPos, Face, Hand};

/// What the fake saw of the settings of one registration (owned: the API's strings are borrowed).
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    size: u32,
    set: u32,
    hardness: f32,
    resistance: f32,
    friction: f32,
    light: u8,
    requires_tool: u8,
    map_color: u8,
    reserved: u8,
    template: String,
    sounds: String,
}

/// What the fake knows of one block.
struct Stored {
    id: String,
    token: u64,
    seen: Seen,
    events: u32,
    handler: Option<BlockHandlerFn>,
    user: usize,
    drop_user: Option<DropUserFn>,
}

static BLOCKS: Mutex<Vec<Stored>> = Mutex::new(Vec::new());
static STATES: Mutex<Vec<(String, u32, u32)>> = Mutex::new(Vec::new());

fn text(s: FishStr) -> String {
    // SAFETY: the API passes valid strings.
    unsafe { s.as_str() }.unwrap().to_string()
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn register(
    _h: *mut c_void,
    token: u64,
    id: FishStr,
    settings: *const BlockSettingsV1,
    events: u32,
    handler: Option<BlockHandlerFn>,
    user: *mut c_void,
    drop_user: Option<DropUserFn>,
) -> FishStatus {
    let id = text(id);
    if *crate::fake_rules::FROZEN.lock().unwrap() || id.starts_with("minecraft:") {
        return FishStatus::INVALID_ARGUMENT;
    }
    // SAFETY: the API passes a valid table whose strings live for the call.
    let s = unsafe { *settings };
    let mut blocks = BLOCKS.lock().unwrap();
    if blocks.iter().any(|b| b.id == id) {
        return FishStatus::ALREADY_EXISTS;
    }
    blocks.push(Stored {
        id,
        token,
        seen: Seen {
            size: s.size,
            set: s.set,
            hardness: s.hardness,
            resistance: s.resistance,
            friction: s.friction,
            light: s.light,
            requires_tool: s.requires_tool,
            map_color: s.map_color,
            reserved: s.reserved,
            template: text(s.template),
            sounds: text(s.sounds),
        },
        events,
        handler,
        user: user as usize,
        drop_user,
    });
    FishStatus::OK
}

unsafe extern "C" fn state_range(
    _h: *mut c_void,
    id: FishStr,
    first: *mut u32,
    count: *mut u32,
) -> FishStatus {
    let id = text(id);
    match STATES.lock().unwrap().iter().find(|(n, _, _)| *n == id) {
        Some((_, f, c)) => {
            // SAFETY: the API passes valid out pointers.
            unsafe {
                *first = *f;
                *count = *c;
            }
            FishStatus::OK
        }
        None => FishStatus::NOT_FOUND,
    }
}

static BLOCK: BlockServiceV1 = BlockServiceV1 {
    header: FishServiceHeader {
        size: size_of::<BlockServiceV1>(),
        version: 1,
    },
    register,
    state_range,
};

/// The table for a service id of this fake.
pub(crate) fn lookup(id: &str) -> Option<*const c_void> {
    Some(match id {
        SERVICE_BLOCK => (&raw const BLOCK).cast(),
        _ => return None,
    })
}

/// Starts a test with an empty fake (tests share one process-wide fake host); registered
/// behaviours of earlier tests are dropped the way the loader drops them.
fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let guard = crate::fake_rules::SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::fake::install();
    for b in BLOCKS.lock().unwrap().drain(..) {
        if let Some(f) = b.drop_user {
            // SAFETY: the user pointer was given with this destructor and is dropped once.
            unsafe { f(b.user as *mut c_void) };
        }
    }
    STATES.lock().unwrap().clear();
    *crate::fake_rules::FROZEN.lock().unwrap() = false;
    guard
}

/// Gives block `id` the states `first..first + count`.
fn set_states(id: &str, first: u32, count: u32) {
    STATES.lock().unwrap().push((id.to_string(), first, count));
}

/// A payload for `event`, filled with recognisable values.
fn payload(event: u8) -> BlockPayloadV1 {
    BlockPayloadV1 {
        size: size_of::<BlockPayloadV1>() as u32,
        event,
        role: ROLE_INTEGRATED_SERVER,
        hand: HAND_OFF,
        face: 3,
        state: 35_725,
        state_index: 2,
        pos: [1, -2, 3],
        other_pos: [4, 5, -6],
        actor_uuid: 0x0123_4567_89ab_cdef_0011_2233_4455_6677u128.to_be_bytes(),
        block: FishStr::from_static("t:ruby"),
        actor: FishStr::from_static("Steve"),
        dimension: FishStr::from_static("minecraft:overworld"),
        item: FishStr::from_static("minecraft:stick"),
        view: FishHandle {
            index: 7,
            generation: 9,
        },
    }
}

/// Calls the handler registered for block `id`, as the loader does.
fn call(id: &str, payload: &BlockPayloadV1) -> u8 {
    let (handler, user) = {
        let blocks = BLOCKS.lock().unwrap();
        let b = blocks.iter().find(|b| b.id == id).unwrap();
        (b.handler.unwrap(), b.user)
    };
    // SAFETY: the user pointer belongs to the handler; the payload is valid for the call.
    unsafe { handler(user as *mut c_void, payload.event, payload) }
}

fn stored<R>(id: &str, f: impl FnOnce(&Stored) -> R) -> R {
    let blocks = BLOCKS.lock().unwrap();
    f(blocks.iter().find(|b| b.id == id).unwrap())
}

fn ctx(p: &BlockPayloadV1) -> &BlockContext {
    // SAFETY: `BlockContext` is `repr(transparent)` over the payload.
    unsafe { &*std::ptr::from_ref(p).cast::<BlockContext>() }
}

#[test]
fn a_block_without_behaviour_only_exists() {
    let _g = fresh();
    let block = Blocks::register("t:plain", BlockSettings::copy("minecraft:stone")).unwrap();
    assert_eq!(block.id(), "t:plain");
    stored("t:plain", |b| {
        assert_eq!(b.events, 0);
        assert!(b.handler.is_none());
        assert_eq!(b.user, 0);
        assert!(b.drop_user.is_none());
        assert_eq!(b.token, crate::host::get().unwrap().mod_token);
        assert_eq!(b.seen.template, "minecraft:stone");
        assert_eq!(b.seen.set, 0);
        assert_eq!(b.seen.size as usize, size_of::<BlockSettingsV1>());
        assert_eq!(b.seen.reserved, 0);
        assert_eq!(b.seen.sounds, "");
    });
}

#[test]
fn every_setting_reaches_the_table() {
    let _g = fresh();
    let settings = BlockSettings::copy("minecraft:iron_block")
        .hardness(5.0)
        .resistance(6.5)
        .light(12)
        .requires_tool(true)
        .friction(0.98)
        .map_color(33)
        .sounds("minecraft:amethyst_block");
    assert_eq!(settings.clone(), settings);
    Blocks::register("t:full", settings).unwrap();
    stored("t:full", |b| {
        let s = &b.seen;
        assert_eq!(s.set, set::ALL);
        assert_eq!(s.template, "minecraft:iron_block");
        assert_eq!(s.hardness, 5.0);
        assert_eq!(s.resistance, 6.5);
        assert_eq!(s.light, 12);
        assert_eq!(s.requires_tool, 1);
        assert_eq!(s.friction, 0.98);
        assert_eq!(s.map_color, 33);
        assert_eq!(s.sounds, "minecraft:amethyst_block");
    });
}

#[test]
fn each_setting_sets_only_its_bit() {
    let _g = fresh();
    let cases: [(&str, BlockSettings, u32); 7] = [
        (
            "t:a",
            BlockSettings::copy("minecraft:stone").hardness(-1.0),
            set::HARDNESS,
        ),
        (
            "t:b",
            BlockSettings::copy("minecraft:stone").resistance(1.0),
            set::RESISTANCE,
        ),
        (
            "t:c",
            BlockSettings::copy("minecraft:stone").light(15),
            set::LIGHT,
        ),
        (
            "t:d",
            BlockSettings::copy("minecraft:stone").requires_tool(false),
            set::REQUIRES_TOOL,
        ),
        (
            "t:e",
            BlockSettings::copy("minecraft:stone").friction(2.0),
            set::FRICTION,
        ),
        (
            "t:f",
            BlockSettings::copy("minecraft:stone").map_color(61),
            set::MAP_COLOR,
        ),
        (
            "t:g",
            BlockSettings::copy("minecraft:stone").sounds("minecraft:stone"),
            set::SOUNDS,
        ),
    ];
    for (id, settings, bit) in cases {
        Blocks::register(id, settings).unwrap();
        assert_eq!(stored(id, |b| b.seen.set), bit, "{id}");
    }
    assert_eq!(stored("t:a", |b| b.seen.hardness), -1.0);
    assert_eq!(stored("t:d", |b| b.seen.requires_tool), 0);
}

#[test]
fn each_callback_sets_its_event_bit() {
    let _g = fresh();
    let builders: [(u8, BlockBehaviour); 6] = [
        (
            event::USE,
            BlockBehaviour::new().on_use(|_| ActionResult::Pass),
        ),
        (event::PLACE, BlockBehaviour::new().on_place(|_| {})),
        (event::BROKEN, BlockBehaviour::new().on_broken(|_| {})),
        (
            event::NEIGHBOR_CHANGED,
            BlockBehaviour::new().neighbor_changed(|_| {}),
        ),
        (
            event::RANDOM_TICK,
            BlockBehaviour::new().random_tick(|_| {}),
        ),
        (
            event::SCHEDULED_TICK,
            BlockBehaviour::new().scheduled_tick(|_| {}),
        ),
    ];
    for (e, behaviour) in builders {
        let id = format!("t:b{e}");
        Blocks::register_with(&id, BlockSettings::copy("minecraft:stone"), behaviour).unwrap();
        stored(&id, |b| {
            assert_eq!(b.events, 1 << e, "event {e}");
            assert!(b.handler.is_some());
            assert!(b.drop_user.is_some());
            assert_ne!(b.user, 0);
        });
    }
    let all = BlockBehaviour::new()
        .on_use(|_| ActionResult::Pass)
        .on_place(|_| {})
        .on_broken(|_| {})
        .neighbor_changed(|_| {})
        .random_tick(|_| {})
        .scheduled_tick(|_| {});
    Blocks::register_with("t:all", BlockSettings::copy("minecraft:stone"), all).unwrap();
    assert_eq!(stored("t:all", |b| b.events), event::ALL);
}

#[test]
fn every_event_reaches_its_callback_with_the_context() {
    let _g = fresh();
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let note = |name: &'static str, log: &Arc<Mutex<Vec<String>>>| {
        let log = Arc::clone(log);
        move |c: &BlockContext| {
            log.lock().unwrap().push(format!(
                "{name} {} {} {}",
                c.block(),
                c.pos(),
                c.state_index()
            ));
        }
    };
    let on_use = {
        let log = Arc::clone(&log);
        move |c: &BlockContext| {
            log.lock().unwrap().push(format!("use {}", c.actor()));
            ActionResult::Success
        }
    };
    let behaviour = BlockBehaviour::new()
        .on_use(on_use)
        .on_place(note("place", &log))
        .on_broken(note("broken", &log))
        .neighbor_changed(note("neighbor", &log))
        .random_tick(note("random", &log))
        .scheduled_tick(note("scheduled", &log));
    Blocks::register_with("t:ruby", BlockSettings::copy("minecraft:stone"), behaviour).unwrap();
    for e in 0..event::COUNT {
        let got = call("t:ruby", &payload(e));
        let want = if e == event::USE {
            ActionResult::Success.as_u8()
        } else {
            ActionResult::Pass.as_u8()
        };
        assert_eq!(got, want, "event {e}");
    }
    assert_eq!(
        *log.lock().unwrap(),
        [
            "use Steve",
            "place t:ruby 1 -2 3 2",
            "broken t:ruby 1 -2 3 2",
            "neighbor t:ruby 1 -2 3 2",
            "random t:ruby 1 -2 3 2",
            "scheduled t:ruby 1 -2 3 2",
        ]
    );
}

#[test]
fn a_missing_callback_passes() {
    let _g = fresh();
    let behaviour = BlockBehaviour::new().on_use(|_| ActionResult::Fail);
    Blocks::register_with(
        "t:only_use",
        BlockSettings::copy("minecraft:stone"),
        behaviour,
    )
    .unwrap();
    for e in 1..event::COUNT {
        assert_eq!(call("t:only_use", &payload(e)), ActionResult::Pass.as_u8());
    }
    // an event number from a newer host
    assert_eq!(
        call("t:only_use", &payload(200)),
        ActionResult::Pass.as_u8()
    );
    // a block with only a tick callback: `use` passes
    let behaviour = BlockBehaviour::new().random_tick(|_| {});
    Blocks::register_with(
        "t:only_tick",
        BlockSettings::copy("minecraft:stone"),
        behaviour,
    )
    .unwrap();
    assert_eq!(
        call("t:only_tick", &payload(event::USE)),
        ActionResult::Pass.as_u8()
    );
}

#[test]
fn on_use_results_round_trip() {
    let _g = fresh();
    let next = Arc::new(AtomicUsize::new(0));
    let results = [
        ActionResult::Pass,
        ActionResult::Success,
        ActionResult::Consume,
        ActionResult::Fail,
    ];
    let behaviour = {
        let next = Arc::clone(&next);
        BlockBehaviour::new().on_use(move |_| results[next.fetch_add(1, Ordering::SeqCst)])
    };
    Blocks::register_with("t:use", BlockSettings::copy("minecraft:stone"), behaviour).unwrap();
    for r in results {
        assert_eq!(call("t:use", &payload(event::USE)), r.as_u8());
    }
}

#[test]
fn a_panicking_callback_reports_it() {
    let _g = fresh();
    let behaviour = BlockBehaviour::new()
        .on_use(|_| panic!("use boom"))
        .random_tick(|_| panic!("tick boom"))
        .on_place(|_| {});
    Blocks::register_with("t:boom", BlockSettings::copy("minecraft:stone"), behaviour).unwrap();
    assert_eq!(call("t:boom", &payload(event::USE)), HANDLER_PANICKED);
    assert_eq!(
        call("t:boom", &payload(event::RANDOM_TICK)),
        HANDLER_PANICKED
    );
    // the block keeps working for the events that do not panic
    assert_eq!(
        call("t:boom", &payload(event::PLACE)),
        ActionResult::Pass.as_u8()
    );
}

#[test]
fn state_ranges_are_known_once_the_game_built_them() {
    let _g = fresh();
    let block = Blocks::register("t:ruby", BlockSettings::copy("minecraft:stone")).unwrap();
    assert_eq!(
        block.state_range().unwrap_err().status(),
        Some(FishStatus::NOT_FOUND)
    );
    assert_eq!(
        Blocks::state_range("t:none").unwrap_err().status(),
        Some(FishStatus::NOT_FOUND)
    );
    set_states("t:ruby", 35_723, 3);
    assert_eq!(block.state_range().unwrap(), 35_723..35_726);
    assert_eq!(Blocks::state_range("t:ruby").unwrap(), 35_723..35_726);
}

#[test]
fn refusals_carry_the_hosts_status() {
    let _g = fresh();
    Blocks::register("t:a", BlockSettings::copy("minecraft:stone")).unwrap();
    assert_eq!(
        Blocks::register("t:a", BlockSettings::copy("minecraft:stone"))
            .unwrap_err()
            .status(),
        Some(FishStatus::ALREADY_EXISTS)
    );
    assert_eq!(
        Blocks::register("minecraft:a", BlockSettings::copy("minecraft:stone"))
            .unwrap_err()
            .status(),
        Some(FishStatus::INVALID_ARGUMENT)
    );
    assert_eq!(BLOCKS.lock().unwrap().len(), 1);
}

#[test]
fn registering_after_the_freeze_says_so() {
    let _g = fresh();
    *crate::fake_rules::FROZEN.lock().unwrap() = true;
    let err = Blocks::register("t:late", BlockSettings::copy("minecraft:stone")).unwrap_err();
    assert_eq!(err.to_string(), "invalid argument: registries are frozen");
    assert!(BLOCKS.lock().unwrap().is_empty());
}

#[test]
fn a_refused_behaviour_is_dropped_by_the_api_and_an_accepted_one_by_the_host() {
    struct Count(Arc<AtomicUsize>);
    impl Drop for Count {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let _g = fresh();
    let drops = Arc::new(AtomicUsize::new(0));
    let guarded = |drops: &Arc<AtomicUsize>| {
        let guard = Count(Arc::clone(drops));
        BlockBehaviour::new().on_place(move |_| {
            let _ = &guard;
        })
    };
    Blocks::register_with(
        "t:a",
        BlockSettings::copy("minecraft:stone"),
        guarded(&drops),
    )
    .unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    Blocks::register_with(
        "t:a",
        BlockSettings::copy("minecraft:stone"),
        guarded(&drops),
    )
    .unwrap_err();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    // the loader going drops the accepted one
    drop(_g);
    let _g = fresh();
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}

#[test]
fn the_context_reads_every_field() {
    let p = payload(event::USE);
    let c = ctx(&p);
    assert_eq!(c.pos(), BlockPos { x: 1, y: -2, z: 3 });
    assert_eq!(c.neighbor_pos(), BlockPos { x: 4, y: 5, z: -6 });
    assert_eq!(c.face(), Some(Face::South));
    assert_eq!(c.hand(), Hand::Off);
    assert_eq!(c.actor(), "Steve");
    assert_eq!(
        c.actor_uuid(),
        Some(0x0123_4567_89ab_cdef_0011_2233_4455_6677u128)
    );
    assert_eq!(c.dimension(), "minecraft:overworld");
    assert_eq!(c.block(), "t:ruby");
    assert_eq!(c.state_id(), 35_725);
    assert_eq!(c.state_index(), 2);
    assert_eq!(c.item(), "minecraft:stick");
    assert_eq!(c.role(), Role::IntegratedServer);
    assert_eq!(c.client(), ClientHandle::from_raw(p.view));
}

#[test]
fn the_context_decodes_faces_hands_roles_and_absent_values() {
    let faces = [
        Face::Down,
        Face::Up,
        Face::North,
        Face::South,
        Face::West,
        Face::East,
    ];
    for (i, face) in faces.into_iter().enumerate() {
        let mut p = payload(event::USE);
        p.face = i as u8;
        assert_eq!(ctx(&p).face(), Some(face));
    }
    let mut p = payload(event::PLACE);
    p.face = FACE_NONE;
    p.hand = HAND_MAIN;
    p.actor_uuid = [0; 16];
    p.actor = FishStr::from_static("");
    p.item = FishStr::from_static("");
    p.role = ROLE_WINDOW;
    assert_eq!(ctx(&p).face(), None);
    assert_eq!(ctx(&p).hand(), Hand::Main);
    assert_eq!(ctx(&p).actor_uuid(), None);
    assert_eq!(ctx(&p).actor(), "");
    assert_eq!(ctx(&p).item(), "");
    assert_eq!(ctx(&p).role(), Role::Window);
    p.role = ROLE_DEDICATED_SERVER;
    assert_eq!(ctx(&p).role(), Role::DedicatedServer);
    // text that is not UTF-8 reads as empty
    p.block = FishStr {
        ptr: [0xffu8].as_ptr(),
        len: 1,
    };
    assert_eq!(ctx(&p).block(), "");
}
