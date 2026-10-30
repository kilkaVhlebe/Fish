//! Fake `fish:item` and `fish:item_group` services (the registry freeze comes from `fake_rules`),
//! and the tests of `item` and `item_group` built on them.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fish_abi::client::{ROLE_DEDICATED_SERVER, ROLE_INTEGRATED_SERVER, ROLE_WINDOW};
use fish_abi::game::{FACE_NONE, HAND_MAIN, HAND_OFF};
use fish_abi::items::{
    AddLineFn, HANDLER_PANICKED, ItemGroupServiceV1, ItemHandlerFn, ItemPayloadV1, ItemServiceV1,
    ItemSettingsV1, SERVICE_ITEM, SERVICE_ITEM_GROUP, event, rarity, set, slot,
};
use fish_abi::{ActionResult, DropUserFn, FishHandle, FishServiceHeader, FishStatus, FishStr};

use crate::client::{ClientHandle, Role};
use crate::game::{BlockPos, Face, Hand};
use crate::item::{
    ItemBehaviour, ItemContext, ItemSettings, ItemSlot, Items, Rarity, TooltipLines,
};
use crate::item_group::{ItemGroup, ItemGroups};

/// What the fake saw of the settings of one registration.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    size: u32,
    set: u32,
    max_stack: u8,
    rarity: u8,
    reserved: [u8; 2],
    template: String,
}

/// What the fake knows of one item.
struct Stored {
    id: String,
    token: u64,
    seen: Seen,
    events: u32,
    handler: Option<ItemHandlerFn>,
    user: usize,
    drop_user: Option<DropUserFn>,
}

/// What the fake knows of one tab.
#[derive(Debug, Clone, PartialEq)]
struct Group {
    id: String,
    title: String,
    icon: String,
    entries: Vec<String>,
}

static ITEMS: Mutex<Vec<Stored>> = Mutex::new(Vec::new());
static GROUPS: Mutex<Vec<Group>> = Mutex::new(Vec::new());
static ADDED: Mutex<Vec<(String, Vec<String>)>> = Mutex::new(Vec::new());

fn text(s: FishStr) -> String {
    // SAFETY: the API passes valid strings.
    unsafe { s.as_str() }.unwrap().to_string()
}

fn texts(entries: *const FishStr, count: usize) -> Vec<String> {
    if count == 0 {
        return Vec::new();
    }
    // SAFETY: the API passes `count` valid strings.
    unsafe { std::slice::from_raw_parts(entries, count) }
        .iter()
        .map(|e| text(*e))
        .collect()
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn register(
    _h: *mut c_void,
    token: u64,
    id: FishStr,
    settings: *const ItemSettingsV1,
    events: u32,
    handler: Option<ItemHandlerFn>,
    user: *mut c_void,
    drop_user: Option<DropUserFn>,
) -> FishStatus {
    let id = text(id);
    // Like the loader: the host owns `user` from here, a refusal drops it at once.
    let refuse = |status: FishStatus| {
        if let Some(f) = drop_user {
            // SAFETY: the pointer came with this destructor and is dropped once.
            unsafe { f(user) };
        }
        status
    };
    if *crate::fake_rules::FROZEN.lock().unwrap() || id.starts_with("minecraft:") {
        return refuse(FishStatus::INVALID_ARGUMENT);
    }
    // SAFETY: the API passes a valid table whose strings live for the call.
    let s = unsafe { *settings };
    let mut items = ITEMS.lock().unwrap();
    if items.iter().any(|i| i.id == id) {
        return refuse(FishStatus::ALREADY_EXISTS);
    }
    items.push(Stored {
        id,
        token,
        seen: Seen {
            size: s.size,
            set: s.set,
            max_stack: s.max_stack,
            rarity: s.rarity,
            reserved: s.reserved,
            template: text(s.template),
        },
        events,
        handler,
        user: user as usize,
        drop_user,
    });
    FishStatus::OK
}

unsafe extern "C" fn register_group(
    _h: *mut c_void,
    _token: u64,
    id: FishStr,
    title: FishStr,
    icon: FishStr,
    entries: *const FishStr,
    entry_count: usize,
) -> FishStatus {
    let id = text(id);
    if *crate::fake_rules::FROZEN.lock().unwrap() || id.starts_with("minecraft:") {
        return FishStatus::INVALID_ARGUMENT;
    }
    let mut groups = GROUPS.lock().unwrap();
    if groups.iter().any(|g| g.id == id) {
        return FishStatus::ALREADY_EXISTS;
    }
    groups.push(Group {
        id,
        title: text(title),
        icon: text(icon),
        entries: texts(entries, entry_count),
    });
    FishStatus::OK
}

unsafe extern "C" fn modify_entries(
    _h: *mut c_void,
    _token: u64,
    group: FishStr,
    entries: *const FishStr,
    entry_count: usize,
) -> FishStatus {
    if *crate::fake_rules::FROZEN.lock().unwrap() {
        return FishStatus::INVALID_ARGUMENT;
    }
    ADDED
        .lock()
        .unwrap()
        .push((text(group), texts(entries, entry_count)));
    FishStatus::OK
}

static ITEM: ItemServiceV1 = ItemServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ItemServiceV1>(),
        version: 1,
    },
    register,
};

static ITEM_GROUP: ItemGroupServiceV1 = ItemGroupServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ItemGroupServiceV1>(),
        version: 1,
    },
    register: register_group,
    modify_entries,
};

/// The table for a service id of this fake.
pub(crate) fn lookup(id: &str) -> Option<*const c_void> {
    Some(match id {
        SERVICE_ITEM => (&raw const ITEM).cast(),
        SERVICE_ITEM_GROUP => (&raw const ITEM_GROUP).cast(),
        _ => return None,
    })
}

/// Starts a test with an empty fake; registered behaviours of earlier tests are dropped the way
/// the loader drops them.
fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let guard = crate::fake_rules::SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::fake::install();
    for i in ITEMS.lock().unwrap().drain(..) {
        if let Some(f) = i.drop_user {
            // SAFETY: the user pointer was given with this destructor and is dropped once.
            unsafe { f(i.user as *mut c_void) };
        }
    }
    GROUPS.lock().unwrap().clear();
    ADDED.lock().unwrap().clear();
    *crate::fake_rules::FROZEN.lock().unwrap() = false;
    guard
}

/// A payload for `event`, filled with recognisable values.
fn payload(event: u8) -> ItemPayloadV1 {
    ItemPayloadV1 {
        size: size_of::<ItemPayloadV1>() as u32,
        event,
        role: ROLE_INTEGRATED_SERVER,
        hand: HAND_OFF,
        face: 3,
        slot: 38,
        count: 7,
        pos: [1, -2, 3],
        selected: 1,
        reserved: [0; 3],
        entity_id: 4_242,
        actor_uuid: 0x0123_4567_89ab_cdef_0011_2233_4455_6677u128.to_be_bytes(),
        item: FishStr::from_static("t:ruby"),
        actor: FishStr::from_static("Steve"),
        dimension: FishStr::from_static("minecraft:overworld"),
        target: FishStr::from_static("minecraft:stone"),
        add_line: None,
        lines: std::ptr::null_mut(),
        view: FishHandle {
            index: 7,
            generation: 9,
        },
    }
}

/// Calls the handler registered for item `id`, as the loader does.
fn call(id: &str, payload: &ItemPayloadV1) -> u8 {
    let (handler, user) = {
        let items = ITEMS.lock().unwrap();
        let i = items.iter().find(|i| i.id == id).unwrap();
        (i.handler.unwrap(), i.user)
    };
    // SAFETY: the user pointer belongs to the handler; the payload is valid for the call.
    unsafe { handler(user as *mut c_void, payload.event, payload) }
}

fn stored<R>(id: &str, f: impl FnOnce(&Stored) -> R) -> R {
    let items = ITEMS.lock().unwrap();
    f(items.iter().find(|i| i.id == id).unwrap())
}

fn ctx(p: &ItemPayloadV1) -> &ItemContext {
    // SAFETY: `ItemContext` is `repr(transparent)` over the payload.
    unsafe { &*std::ptr::from_ref(p).cast::<ItemContext>() }
}

unsafe extern "C" fn collect(lines: *mut c_void, line: FishStr) {
    // SAFETY: `lines` is the `Vec<String>` the test passes in the payload.
    unsafe { &mut *lines.cast::<Vec<String>>() }.push(text(line));
}

#[test]
fn an_item_without_behaviour_only_exists() {
    let _g = fresh();
    let item = Items::register("t:plain", ItemSettings::copy("minecraft:stick")).unwrap();
    assert_eq!(item.id(), "t:plain");
    stored("t:plain", |i| {
        assert_eq!(i.events, 0);
        assert!(i.handler.is_none());
        assert_eq!(i.user, 0);
        assert!(i.drop_user.is_none());
        assert_eq!(i.token, crate::host::get().unwrap().mod_token);
        assert_eq!(i.seen.template, "minecraft:stick");
        assert_eq!(i.seen.set, 0);
        assert_eq!(i.seen.size as usize, size_of::<ItemSettingsV1>());
        assert_eq!(i.seen.reserved, [0; 2]);
    });
}

#[test]
fn every_setting_reaches_the_table() {
    let _g = fresh();
    let settings = ItemSettings::copy("minecraft:diamond")
        .max_stack(16)
        .rarity(Rarity::Rare);
    assert_eq!(settings.clone(), settings);
    Items::register("t:full", settings).unwrap();
    stored("t:full", |i| {
        assert_eq!(i.seen.set, set::ALL);
        assert_eq!(i.seen.max_stack, 16);
        assert_eq!(i.seen.rarity, rarity::RARE);
    });
    Items::register(
        "t:stack",
        ItemSettings::copy("minecraft:stick").max_stack(1),
    )
    .unwrap();
    assert_eq!(stored("t:stack", |i| i.seen.set), set::MAX_STACK);
    for (r, v) in [
        (Rarity::Common, rarity::COMMON),
        (Rarity::Uncommon, rarity::UNCOMMON),
        (Rarity::Rare, rarity::RARE),
        (Rarity::Epic, rarity::EPIC),
    ] {
        let id = format!("t:r{v}");
        Items::register(&id, ItemSettings::copy("minecraft:stick").rarity(r)).unwrap();
        assert_eq!(
            stored(&id, |i| (i.seen.set, i.seen.rarity)),
            (set::RARITY, v)
        );
    }
}

#[test]
fn each_callback_sets_its_event_bit() {
    let _g = fresh();
    let builders: [(u8, ItemBehaviour); 6] = [
        (
            event::USE,
            ItemBehaviour::new().on_use(|_| ActionResult::Pass),
        ),
        (
            event::USE_ON_BLOCK,
            ItemBehaviour::new().on_use_on_block(|_| ActionResult::Pass),
        ),
        (
            event::USE_ON_ENTITY,
            ItemBehaviour::new().on_use_on_entity(|_| ActionResult::Pass),
        ),
        (
            event::FINISH_USING,
            ItemBehaviour::new().on_finish_using(|_| {}),
        ),
        (
            event::INVENTORY_TICK,
            ItemBehaviour::new().on_inventory_tick(|_| {}),
        ),
        (
            event::APPEND_TOOLTIP,
            ItemBehaviour::new().append_tooltip(|_, _| {}),
        ),
    ];
    for (e, behaviour) in builders {
        let id = format!("t:i{e}");
        Items::register_with(&id, ItemSettings::copy("minecraft:stick"), behaviour).unwrap();
        stored(&id, |i| {
            assert_eq!(i.events, 1 << e, "event {e}");
            assert!(i.handler.is_some());
            assert!(i.drop_user.is_some());
            assert_ne!(i.user, 0);
        });
    }
    let all = ItemBehaviour::new()
        .on_use(|_| ActionResult::Pass)
        .on_use_on_block(|_| ActionResult::Pass)
        .on_use_on_entity(|_| ActionResult::Pass)
        .on_finish_using(|_| {})
        .on_inventory_tick(|_| {})
        .append_tooltip(|_, _| {});
    Items::register_with("t:all", ItemSettings::copy("minecraft:stick"), all).unwrap();
    assert_eq!(stored("t:all", |i| i.events), event::ALL);
}

#[test]
fn every_event_reaches_its_callback_with_the_context() {
    let _g = fresh();
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let decide = |name: &'static str, result: ActionResult, log: &Arc<Mutex<Vec<String>>>| {
        let log = Arc::clone(log);
        move |c: &ItemContext| {
            log.lock().unwrap().push(format!(
                "{name} {} {} {} {} {}",
                c.item(),
                c.pos(),
                c.target(),
                c.entity_id(),
                c.actor()
            ));
            result
        }
    };
    let note = |name: &'static str, log: &Arc<Mutex<Vec<String>>>| {
        let log = Arc::clone(log);
        move |c: &ItemContext| {
            log.lock().unwrap().push(format!(
                "{name} {:?} {} {}",
                c.slot(),
                c.selected(),
                c.count()
            ));
        }
    };
    let tip = {
        let log = Arc::clone(&log);
        move |c: &ItemContext, lines: &mut TooltipLines| {
            log.lock().unwrap().push(format!("tip {}", c.count()));
            lines.add("§6one");
            lines.add("two");
        }
    };
    let behaviour = ItemBehaviour::new()
        .on_use(decide("use", ActionResult::Success, &log))
        .on_use_on_block(decide("block", ActionResult::Consume, &log))
        .on_use_on_entity(decide("entity", ActionResult::Fail, &log))
        .on_finish_using(note("finish", &log))
        .on_inventory_tick(note("tick", &log))
        .append_tooltip(tip);
    Items::register_with("t:ruby", ItemSettings::copy("minecraft:stick"), behaviour).unwrap();
    let want = [
        ActionResult::Success,
        ActionResult::Consume,
        ActionResult::Fail,
        ActionResult::Pass,
        ActionResult::Pass,
        ActionResult::Pass,
    ];
    let mut lines: Vec<String> = Vec::new();
    for e in 0..event::COUNT {
        let mut p = payload(e);
        if e == event::APPEND_TOOLTIP {
            p.add_line = Some(collect as AddLineFn);
            p.lines = std::ptr::from_mut(&mut lines).cast();
        }
        assert_eq!(
            call("t:ruby", &p),
            want[usize::from(e)].as_u8(),
            "event {e}"
        );
    }
    assert_eq!(lines, ["§6one", "two"]);
    assert_eq!(
        *log.lock().unwrap(),
        [
            "use t:ruby 1 -2 3 minecraft:stone 4242 Steve",
            "block t:ruby 1 -2 3 minecraft:stone 4242 Steve",
            "entity t:ruby 1 -2 3 minecraft:stone 4242 Steve",
            "finish Some(Armor(2)) true 7",
            "tick Some(Armor(2)) true 7",
            "tip 7",
        ]
    );
}

#[test]
fn a_tooltip_without_a_line_sink_adds_nothing() {
    let _g = fresh();
    let behaviour = ItemBehaviour::new().append_tooltip(|_, lines| lines.add("never"));
    Items::register_with("t:tip", ItemSettings::copy("minecraft:stick"), behaviour).unwrap();
    assert_eq!(
        call("t:tip", &payload(event::APPEND_TOOLTIP)),
        ActionResult::Pass.as_u8()
    );
}

#[test]
fn a_missing_callback_passes() {
    let _g = fresh();
    let behaviour = ItemBehaviour::new().on_use(|_| ActionResult::Fail);
    Items::register_with(
        "t:only_use",
        ItemSettings::copy("minecraft:stick"),
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
    let behaviour = ItemBehaviour::new().on_inventory_tick(|_| {});
    Items::register_with(
        "t:only_tick",
        ItemSettings::copy("minecraft:stick"),
        behaviour,
    )
    .unwrap();
    assert_eq!(
        call("t:only_tick", &payload(event::USE)),
        ActionResult::Pass.as_u8()
    );
}

#[test]
fn use_results_round_trip() {
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
        ItemBehaviour::new().on_use(move |_| results[next.fetch_add(1, Ordering::SeqCst)])
    };
    Items::register_with("t:use", ItemSettings::copy("minecraft:stick"), behaviour).unwrap();
    for r in results {
        assert_eq!(call("t:use", &payload(event::USE)), r.as_u8());
    }
}

#[test]
fn a_panicking_callback_reports_it() {
    let _g = fresh();
    let behaviour = ItemBehaviour::new()
        .on_use(|_| panic!("use boom"))
        .on_inventory_tick(|_| panic!("tick boom"))
        .on_finish_using(|_| {});
    Items::register_with("t:boom", ItemSettings::copy("minecraft:stick"), behaviour).unwrap();
    assert_eq!(call("t:boom", &payload(event::USE)), HANDLER_PANICKED);
    assert_eq!(
        call("t:boom", &payload(event::INVENTORY_TICK)),
        HANDLER_PANICKED
    );
    assert_eq!(
        call("t:boom", &payload(event::FINISH_USING)),
        ActionResult::Pass.as_u8()
    );
}

#[test]
fn refusals_carry_the_hosts_status() {
    let _g = fresh();
    Items::register("t:a", ItemSettings::copy("minecraft:stick")).unwrap();
    assert_eq!(
        Items::register("t:a", ItemSettings::copy("minecraft:stick"))
            .unwrap_err()
            .status(),
        Some(FishStatus::ALREADY_EXISTS)
    );
    assert_eq!(
        Items::register("minecraft:a", ItemSettings::copy("minecraft:stick"))
            .unwrap_err()
            .status(),
        Some(FishStatus::INVALID_ARGUMENT)
    );
    assert_eq!(ITEMS.lock().unwrap().len(), 1);
}

#[test]
fn registering_after_the_freeze_says_so() {
    let _g = fresh();
    *crate::fake_rules::FROZEN.lock().unwrap() = true;
    let err = Items::register("t:late", ItemSettings::copy("minecraft:stick")).unwrap_err();
    assert_eq!(err.to_string(), "invalid argument: registries are frozen");
    assert!(ITEMS.lock().unwrap().is_empty());
    let err = ItemGroup::builder("t:late_tab")
        .icon("t:a")
        .register()
        .unwrap_err();
    assert_eq!(err.to_string(), "invalid argument: registries are frozen");
    let err = ItemGroups::modify_entries("minecraft:ingredients", ["t:a"]).unwrap_err();
    assert_eq!(err.to_string(), "invalid argument: registries are frozen");
    assert!(GROUPS.lock().unwrap().is_empty());
    assert!(ADDED.lock().unwrap().is_empty());
}

#[test]
fn a_refused_behaviour_is_dropped_by_the_host_at_once_and_an_accepted_one_with_the_loader() {
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
        ItemBehaviour::new().on_finish_using(move |_| {
            let _ = &guard;
        })
    };
    Items::register_with(
        "t:a",
        ItemSettings::copy("minecraft:stick"),
        guarded(&drops),
    )
    .unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    Items::register_with(
        "t:a",
        ItemSettings::copy("minecraft:stick"),
        guarded(&drops),
    )
    .unwrap_err();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    drop(_g);
    let _g = fresh();
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}

#[test]
fn groups_reach_the_host_with_their_title_icon_and_entries() {
    let _g = fresh();
    let group = ItemGroup::builder("t:tab")
        .title("Tab")
        .icon("t:ruby")
        .entries(["t:ruby", "t:snack"])
        .entry("minecraft:stick")
        .register()
        .unwrap();
    assert_eq!(group.id(), "t:tab");
    ItemGroup::builder("t:bare")
        .icon("t:ruby")
        .register()
        .unwrap();
    assert_eq!(
        *GROUPS.lock().unwrap(),
        [
            Group {
                id: "t:tab".into(),
                title: "Tab".into(),
                icon: "t:ruby".into(),
                entries: vec!["t:ruby".into(), "t:snack".into(), "minecraft:stick".into()],
            },
            Group {
                id: "t:bare".into(),
                title: String::new(),
                icon: "t:ruby".into(),
                entries: Vec::new(),
            },
        ]
    );
    assert_eq!(
        ItemGroup::builder("t:tab")
            .icon("t:ruby")
            .register()
            .unwrap_err()
            .status(),
        Some(FishStatus::ALREADY_EXISTS)
    );
    assert_eq!(
        ItemGroup::builder("minecraft:tab")
            .icon("t:ruby")
            .register()
            .unwrap_err()
            .status(),
        Some(FishStatus::INVALID_ARGUMENT)
    );
}

#[test]
fn modified_entries_reach_the_host_in_order() {
    let _g = fresh();
    ItemGroups::modify_entries("minecraft:ingredients", ["t:ruby"]).unwrap();
    ItemGroups::modify_entries("t:tab", Vec::<String>::new()).unwrap();
    assert_eq!(
        *ADDED.lock().unwrap(),
        [
            (
                "minecraft:ingredients".to_string(),
                vec!["t:ruby".to_string()]
            ),
            ("t:tab".to_string(), Vec::new()),
        ]
    );
}

#[test]
fn the_context_reads_every_field() {
    let mut p = payload(event::USE_ON_BLOCK);
    p.slot = slot::NONE;
    let c = ctx(&p);
    assert_eq!(c.item(), "t:ruby");
    assert_eq!(c.count(), 7);
    assert_eq!(c.hand(), Hand::Off);
    assert_eq!(c.face(), Some(Face::South));
    assert_eq!(c.slot(), None);
    assert!(c.selected());
    assert_eq!(c.pos(), BlockPos { x: 1, y: -2, z: 3 });
    assert_eq!(c.target(), "minecraft:stone");
    assert_eq!(c.entity_id(), 4_242);
    assert_eq!(c.actor(), "Steve");
    assert_eq!(
        c.actor_uuid(),
        Some(0x0123_4567_89ab_cdef_0011_2233_4455_6677u128)
    );
    assert_eq!(c.dimension(), "minecraft:overworld");
    assert_eq!(c.role(), Role::IntegratedServer);
    assert_eq!(c.client(), ClientHandle::from_raw(p.view));
}

#[test]
fn the_context_decodes_faces_slots_hands_roles_and_absent_values() {
    let faces = [
        Face::Down,
        Face::Up,
        Face::North,
        Face::South,
        Face::West,
        Face::East,
    ];
    for (i, face) in faces.into_iter().enumerate() {
        let mut p = payload(event::USE_ON_BLOCK);
        p.face = i as u8;
        assert_eq!(ctx(&p).face(), Some(face));
    }
    let slots = [
        (0, Some(ItemSlot::Inventory(0))),
        (35, Some(ItemSlot::Inventory(35))),
        (36, Some(ItemSlot::Armor(0))),
        (39, Some(ItemSlot::Armor(3))),
        (40, Some(ItemSlot::OffHand)),
        (41, None),
        (-1, None),
        (-7, None),
    ];
    for (raw, want) in slots {
        let mut p = payload(event::INVENTORY_TICK);
        p.slot = raw;
        assert_eq!(ctx(&p).slot(), want, "slot {raw}");
    }
    let mut p = payload(event::USE);
    p.face = FACE_NONE;
    p.hand = HAND_MAIN;
    p.selected = 0;
    p.actor_uuid = [0; 16];
    p.actor = FishStr::from_static("");
    p.target = FishStr::from_static("");
    p.role = ROLE_WINDOW;
    assert_eq!(ctx(&p).face(), None);
    assert_eq!(ctx(&p).hand(), Hand::Main);
    assert!(!ctx(&p).selected());
    assert_eq!(ctx(&p).actor_uuid(), None);
    assert_eq!(ctx(&p).actor(), "");
    assert_eq!(ctx(&p).target(), "");
    assert_eq!(ctx(&p).role(), Role::Window);
    p.role = ROLE_DEDICATED_SERVER;
    assert_eq!(ctx(&p).role(), Role::DedicatedServer);
    // text that is not UTF-8 reads as empty
    p.item = FishStr {
        ptr: [0xffu8].as_ptr(),
        len: 1,
    };
    assert_eq!(ctx(&p).item(), "");
}
