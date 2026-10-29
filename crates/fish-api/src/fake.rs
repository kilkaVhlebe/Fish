//! An in-crate fake host table used by the unit tests.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{LazyLock, Mutex};

use fish_abi::services::{
    FishLoaderServiceV1, FishModInfoV1, MOD_STATUS_FAILED, MOD_STATUS_OK, ObjectSink,
    SERVICE_LOADER, SIDE_SERVER,
};
use fish_abi::{
    DropUserFn, FISH_ABI_MAJOR, FISH_ABI_MINOR, FishHostV1, FishListener, FishServiceHeader,
    FishStatus, FishStr,
};

use crate::event::{Combine, DEFAULT_PHASE, Event, Payload, Phase};
use crate::loader::{Environment, FishLoader, ModStatus};
use crate::{ActionResult, Error};

struct Listener {
    f: FishListener,
    user: usize,
}

struct FakeEvent {
    combine: u8,
    listeners: Vec<Listener>,
}

pub(crate) struct Fake {
    pub logs: Mutex<Vec<(u8, String, String)>>,
    events: Mutex<HashMap<String, FakeEvent>>,
    pub orders: Mutex<Vec<(String, String, String)>>,
    share: Mutex<HashMap<String, (String, usize)>>,
}

unsafe fn s<'a>(v: FishStr) -> &'a str {
    // SAFETY: tests only pass valid strings.
    unsafe { v.as_str() }.unwrap()
}

unsafe fn fake<'a>(host: *mut c_void) -> &'a Fake {
    // SAFETY: `host` is the leaked `Fake` installed by `install`.
    unsafe { &*host.cast::<Fake>() }
}

unsafe extern "C" fn log(host: *mut c_void, _token: u64, level: u8, target: FishStr, msg: FishStr) {
    // SAFETY: test strings are valid.
    unsafe {
        fake(host)
            .logs
            .lock()
            .unwrap()
            .push((level, s(target).to_string(), s(msg).to_string()));
    }
}

unsafe extern "C" fn get_service(
    _host: *mut c_void,
    _token: u64,
    id: FishStr,
    min_version: u32,
    out: *mut *const c_void,
) -> FishStatus {
    // SAFETY: test strings are valid.
    let id = unsafe { s(id) };
    if id != SERVICE_LOADER {
        return FishStatus::NOT_FOUND;
    }
    if min_version > 1 {
        return FishStatus::VERSION_MISMATCH;
    }
    // SAFETY: `out` is valid.
    unsafe { *out = (&raw const LOADER_SERVICE).cast() };
    FishStatus::OK
}

unsafe extern "C" fn event_define(
    host: *mut c_void,
    _t: u64,
    id: FishStr,
    combine: u8,
) -> FishStatus {
    // SAFETY: test strings and host are valid.
    let (f, id) = unsafe { (fake(host), s(id)) };
    if !fish_abi::ident::is_valid_identifier(id) {
        return FishStatus::INVALID_ARGUMENT;
    }
    let mut ev = f.events.lock().unwrap();
    match ev.get(id) {
        Some(e) if e.combine != combine => FishStatus::ALREADY_EXISTS,
        Some(_) => FishStatus::OK,
        None => {
            ev.insert(
                id.to_string(),
                FakeEvent {
                    combine,
                    listeners: vec![],
                },
            );
            FishStatus::OK
        }
    }
}

unsafe extern "C" fn event_lookup(_host: *mut c_void, _id: FishStr, _out: *mut u32) -> FishStatus {
    FishStatus::UNSUPPORTED
}

unsafe extern "C" fn event_register(
    host: *mut c_void,
    _t: u64,
    id: FishStr,
    _phase: FishStr,
    listener: FishListener,
    user: *mut c_void,
    _drop_user: Option<DropUserFn>,
) -> FishStatus {
    // SAFETY: test strings and host are valid.
    let (f, id) = unsafe { (fake(host), s(id)) };
    let mut ev = f.events.lock().unwrap();
    let Some(e) = ev.get_mut(id) else {
        return FishStatus::NOT_FOUND;
    };
    e.listeners.push(Listener {
        f: listener,
        user: user as usize,
    });
    FishStatus::OK
}

unsafe extern "C" fn event_phase_order(
    host: *mut c_void,
    _t: u64,
    id: FishStr,
    first: FishStr,
    second: FishStr,
) -> FishStatus {
    // SAFETY: test strings and host are valid.
    unsafe {
        fake(host)
            .orders
            .lock()
            .unwrap()
            .push((s(id).into(), s(first).into(), s(second).into()));
    }
    FishStatus::OK
}

unsafe extern "C" fn event_invoke(
    host: *mut c_void,
    _t: u64,
    id: FishStr,
    payload: *mut c_void,
    out: *mut c_void,
) -> FishStatus {
    // SAFETY: test strings and host are valid.
    let (f, id) = unsafe { (fake(host), s(id)) };
    let (combine, calls): (u8, Vec<(FishListener, usize)>) = {
        let ev = f.events.lock().unwrap();
        let Some(e) = ev.get(id) else {
            return FishStatus::NOT_FOUND;
        };
        (
            e.combine,
            e.listeners.iter().map(|l| (l.f, l.user)).collect(),
        )
    };
    let mut result = FishStatus::OK;
    for (func, user) in calls {
        // SAFETY: listener and user came from `event_register`.
        let st = unsafe { func(user as *mut c_void, payload, out) };
        if !st.is_ok() {
            result = st;
        }
        // SAFETY: first-non-pass events pass an `out` whose first byte is the action.
        if combine == 1 && !out.is_null() && unsafe { *out.cast::<u8>() } != 0 {
            break;
        }
    }
    result
}

unsafe extern "C" fn event_invoke_index(
    _host: *mut c_void,
    _t: u64,
    _index: u32,
    _payload: *mut c_void,
    _out: *mut c_void,
) -> FishStatus {
    FishStatus::UNSUPPORTED
}

// --- fake loader service ------------------------------------------------------------------

unsafe extern "C" fn mod_count(_host: *mut c_void) -> usize {
    2
}

fn info(i: usize) -> FishModInfoV1 {
    let (id, status) = if i == 0 {
        ("alpha", MOD_STATUS_OK)
    } else {
        ("beta", MOD_STATUS_FAILED)
    };
    FishModInfoV1 {
        id: FishStr::from_static(id),
        version: FishStr::from_static("1.2.3"),
        name: FishStr::from_static(if i == 0 { "Alpha" } else { "Beta" }),
        description: FishStr::from_static("desc"),
        status,
    }
}

unsafe extern "C" fn mod_at(_h: *mut c_void, index: usize, out: *mut FishModInfoV1) -> FishStatus {
    if index >= 2 {
        return FishStatus::NOT_FOUND;
    }
    // SAFETY: `out` is valid.
    unsafe { *out = info(index) };
    FishStatus::OK
}

unsafe extern "C" fn find_mod(_h: *mut c_void, id: FishStr, out: *mut FishModInfoV1) -> FishStatus {
    // SAFETY: test strings are valid; `out` is valid.
    unsafe {
        match s(id) {
            "alpha" => *out = info(0),
            "beta" => *out = info(1),
            _ => return FishStatus::NOT_FOUND,
        }
    }
    FishStatus::OK
}

unsafe extern "C" fn environment(_h: *mut c_void) -> u8 {
    SIDE_SERVER
}

unsafe extern "C" fn is_development(_h: *mut c_void) -> u8 {
    1
}

unsafe extern "C" fn game_dir(_h: *mut c_void) -> FishStr {
    FishStr::from_static("/game")
}

unsafe extern "C" fn config_dir(_h: *mut c_void) -> FishStr {
    FishStr::from_static("/game/config")
}

unsafe extern "C" fn share_put(
    host: *mut c_void,
    key: FishStr,
    tag: FishStr,
    value: *mut c_void,
) -> FishStatus {
    // SAFETY: test strings and host are valid.
    let (f, key, tag) = unsafe { (fake(host), s(key), s(tag)) };
    let mut m = f.share.lock().unwrap();
    if m.contains_key(key) {
        return FishStatus::ALREADY_EXISTS;
    }
    m.insert(key.to_string(), (tag.to_string(), value as usize));
    FishStatus::OK
}

unsafe extern "C" fn share_get(
    host: *mut c_void,
    key: FishStr,
    tag: FishStr,
    out: *mut *mut c_void,
) -> FishStatus {
    // SAFETY: test strings, host and `out` are valid.
    unsafe {
        let (f, key, tag) = (fake(host), s(key), s(tag));
        match f.share.lock().unwrap().get(key) {
            None => FishStatus::NOT_FOUND,
            Some((t, _)) if t != tag => FishStatus::TYPE_MISMATCH,
            Some((_, p)) => {
                *out = *p as *mut c_void;
                FishStatus::OK
            }
        }
    }
}

static OBJECT: u32 = 42;

unsafe extern "C" fn entrypoints(
    _h: *mut c_void,
    key: FishStr,
    sink: ObjectSink,
    user: *mut c_void,
) -> FishStatus {
    // SAFETY: test strings are valid; `sink` is the caller's.
    unsafe {
        if s(key) == "fish:thing" {
            sink(
                user,
                FishStr::from_static("alpha"),
                (&raw const OBJECT).cast(),
            );
            FishStatus::OK
        } else {
            FishStatus::NOT_FOUND
        }
    }
}

static LOADER_SERVICE: FishLoaderServiceV1 = FishLoaderServiceV1 {
    header: FishServiceHeader {
        size: size_of::<FishLoaderServiceV1>(),
        version: 1,
    },
    mod_count,
    mod_at,
    find_mod,
    environment,
    is_development,
    game_dir,
    config_dir,
    object_share_put: share_put,
    object_share_get: share_get,
    entrypoints,
};

static FAKE: LazyLock<Fake> = LazyLock::new(|| Fake {
    logs: Mutex::default(),
    events: Mutex::default(),
    orders: Mutex::default(),
    share: Mutex::default(),
});

static TABLE: LazyLock<FishHostV1> = LazyLock::new(|| FishHostV1 {
    size: size_of::<FishHostV1>(),
    abi_major: FISH_ABI_MAJOR,
    abi_minor: FISH_ABI_MINOR,
    host: std::ptr::from_ref(&*FAKE).cast_mut().cast(),
    mod_token: 1,
    log,
    get_service,
    event_define,
    event_lookup,
    event_register,
    event_phase_order,
    event_invoke,
    event_invoke_index,
});

/// Attaches (once) and returns the shared fake host.
pub(crate) fn install() -> &'static Fake {
    crate::__attach_for_tests(&*TABLE);
    &FAKE
}

// --- tests --------------------------------------------------------------------------------

#[repr(C)]
struct Ping {
    size: u32,
    value: u32,
}

// SAFETY: repr(C), starts with `size: u32`.
unsafe impl Payload for Ping {}

fn ping(value: u32) -> Ping {
    Ping {
        size: size_of::<Ping>() as u32,
        value,
    }
}

#[test]
fn notify_listeners_run_in_registration_order_and_can_mutate() {
    install();
    static EV: Event<Ping> = Event::new("fake:notify", Combine::Notify);
    EV.register(|p| p.value += 1).unwrap();
    EV.register(|p| p.value *= 10).unwrap();
    let mut p = ping(1);
    EV.invoke(&mut p).unwrap();
    assert_eq!(p.value, 20);
    assert_eq!(EV.id(), "fake:notify");
}

#[test]
fn first_non_pass_stops_dispatch() {
    install();
    static EV: Event<Ping, ActionResult> = Event::new("fake:first", Combine::FirstNonPass);
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    EV.register(|_| ActionResult::Pass).unwrap();
    EV.register(|_| {
        CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        ActionResult::Consume
    })
    .unwrap();
    EV.register(|_| {
        CALLS.fetch_add(100, std::sync::atomic::Ordering::SeqCst);
        ActionResult::Fail
    })
    .unwrap();
    assert_eq!(EV.invoke(&mut ping(0)).unwrap(), ActionResult::Consume);
    assert_eq!(CALLS.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn panicking_listener_is_caught_and_reported() {
    let f = install();
    static EV: Event<Ping> = Event::new("fake:panics", Combine::Notify);
    EV.register(|p| {
        if p.value == 7 {
            panic!("seven is unlucky");
        }
    })
    .unwrap();
    let err = EV.invoke(&mut ping(7)).unwrap_err();
    assert_eq!(err.status(), Some(FishStatus::PANICKED));
    assert!(err.to_string().contains("PANICKED"));
    assert!(EV.invoke(&mut ping(1)).is_ok());
    let logs = f.logs.lock().unwrap();
    assert!(
        logs.iter()
            .any(|(lvl, _, m)| *lvl == 1 && m.contains("seven is unlucky")),
        "{logs:?}"
    );
}

#[test]
fn undersized_payload_is_refused() {
    install();
    static EV: Event<Ping> = Event::new("fake:small", Combine::Notify);
    EV.register(|_| {}).unwrap();
    let mut p = ping(0);
    p.size = 4;
    let err = EV.invoke(&mut p).unwrap_err();
    assert_eq!(err.status(), Some(FishStatus::INVALID_ARGUMENT));
}

#[test]
fn conflicting_redefinition_and_invalid_ids_fail() {
    install();
    static A: Event<Ping> = Event::new("fake:conflict", Combine::Notify);
    static B: Event<Ping, ActionResult> = Event::new("fake:conflict", Combine::FirstNonPass);
    A.define().unwrap();
    A.define().unwrap();
    assert_eq!(
        B.define().unwrap_err().status(),
        Some(FishStatus::ALREADY_EXISTS)
    );
    static BAD: Event<Ping> = Event::new("NotValid", Combine::Notify);
    assert!(matches!(BAD.define(), Err(Error::InvalidArgument(_))));
    static OK: Event<Ping> = Event::new("fake:badphase", Combine::Notify);
    assert!(matches!(
        OK.register_phase(Phase("bad phase"), |_| {}),
        Err(Error::InvalidArgument(_))
    ));
}

#[test]
fn phase_ordering_reaches_the_host() {
    let f = install();
    static EV: Event<Ping> = Event::new("fake:phases", Combine::Notify);
    EV.add_phase_ordering(Phase("a:early"), Phase(DEFAULT_PHASE))
        .unwrap();
    assert!(f.orders.lock().unwrap().contains(&(
        "fake:phases".into(),
        "a:early".into(),
        DEFAULT_PHASE.into()
    )));
    assert_eq!(Phase::DEFAULT.0, DEFAULT_PHASE);
}

#[test]
fn loader_api() {
    install();
    let l = FishLoader::get();
    let mods = l.mods();
    assert_eq!(mods.len(), 2);
    assert_eq!(mods[0].id, "alpha");
    assert_eq!(mods[0].status, ModStatus::Ok);
    assert_eq!(mods[1].status, ModStatus::Failed);
    assert_eq!(l.find_mod("alpha").unwrap().name, "Alpha");
    assert!(l.find_mod("nope").is_none());
    assert!(l.is_mod_loaded("alpha"));
    assert!(!l.is_mod_loaded("beta"));
    assert!(!l.is_mod_loaded("nope"));
    assert_eq!(l.environment(), Environment::Server);
    assert!(l.is_development());
    assert_eq!(l.game_dir(), std::path::PathBuf::from("/game"));
    assert_eq!(l.config_dir(), std::path::PathBuf::from("/game/config"));
}

#[test]
fn object_share_roundtrip_and_type_check() {
    install();
    static VALUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(5);
    let share = FishLoader::get().object_share();
    share.put("fake:counter", &VALUE).unwrap();
    assert_eq!(
        share.put("fake:counter", &VALUE).unwrap_err().status(),
        Some(FishStatus::ALREADY_EXISTS)
    );
    let got: &std::sync::atomic::AtomicU64 = share.get("fake:counter").unwrap();
    assert_eq!(got.load(std::sync::atomic::Ordering::SeqCst), 5);
    assert_eq!(
        share.get::<String>("fake:counter").unwrap_err().status(),
        Some(FishStatus::TYPE_MISMATCH)
    );
    assert_eq!(
        share.get::<u8>("fake:missing").unwrap_err().status(),
        Some(FishStatus::NOT_FOUND)
    );
    // raw tagged access
    let p = share.get_tagged(
        "fake:counter",
        std::any::type_name::<std::sync::atomic::AtomicU64>(),
    );
    assert!(p.is_ok());
}

#[test]
fn entrypoint_objects_are_collected() {
    install();
    let objs = FishLoader::get().entrypoint_objects::<u32>("fish:thing");
    assert_eq!(objs.len(), 1);
    assert_eq!(objs[0].0, "alpha");
    assert_eq!(*objs[0].1, 42);
    assert!(
        FishLoader::get()
            .entrypoint_objects::<u32>("fish:none")
            .is_empty()
    );
}

#[test]
fn log_bridge_forwards_levels() {
    let f = install();
    log::error!(target: "fake-test", "bridge-error-marker");
    log::warn!(target: "fake-test", "bridge-warn-marker");
    log::info!(target: "fake-test", "bridge-info-marker");
    log::debug!(target: "fake-test", "bridge-debug-marker");
    log::trace!(target: "fake-test", "bridge-trace-marker");
    let logs = f.logs.lock().unwrap();
    for (lvl, marker) in [
        (1, "error"),
        (2, "warn"),
        (3, "info"),
        (4, "debug"),
        (5, "trace"),
    ] {
        let m = format!("bridge-{marker}-marker");
        assert!(
            logs.iter()
                .any(|(l, t, msg)| *l == lvl && t == "fake-test" && *msg == m),
            "{marker}"
        );
    }
}

#[test]
fn services_report_missing_and_old_versions() {
    install();
    let h = crate::host::get().unwrap();
    let mut out: *const c_void = std::ptr::null();
    // SAFETY: valid fake host table.
    let st =
        unsafe { (h.get_service)(h.host, 1, FishStr::from_static("fish:nothing"), 1, &mut out) };
    assert_eq!(st, FishStatus::NOT_FOUND);
    // SAFETY: as above.
    let st =
        unsafe { (h.get_service)(h.host, 1, FishStr::from_static(SERVICE_LOADER), 9, &mut out) };
    assert_eq!(st, FishStatus::VERSION_MISMATCH);
}

mod export {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static RAN: AtomicU32 = AtomicU32::new(0);
    static API: u32 = 99;

    fn init_ok() {
        RAN.fetch_or(1, Ordering::SeqCst);
    }
    fn init_err() -> Result<(), String> {
        Err("it broke".into())
    }
    fn init_panic() {
        panic!("init exploded");
    }
    fn object_api() -> &'static u32 {
        &API
    }

    crate::export_mod! {
        main: [init_ok, init_err],
        "fake:boom": [init_panic],
        "fish:obj": object [object_api],
    }

    #[test]
    fn exported_table_and_trampolines() {
        let f = install();
        // SAFETY: `fish_mod_v1` returns a pointer to a static table.
        let table = unsafe { &*fish_mod_v1() };
        assert_eq!(table.size, size_of::<fish_abi::FishModV1>());
        assert_eq!(
            (table.abi_major, table.abi_minor),
            (FISH_ABI_MAJOR, FISH_ABI_MINOR)
        );
        assert_eq!(table.entrypoint_count, 4);
        // SAFETY: the table points at `entrypoint_count` static entries.
        let entries =
            unsafe { std::slice::from_raw_parts(table.entrypoints, table.entrypoint_count) };
        let text = |s: FishStr| unsafe { s.as_str().unwrap() };
        let listed: Vec<(&str, &str, u8)> = entries
            .iter()
            .map(|e| (text(e.key), text(e.name), e.kind))
            .collect();
        assert_eq!(
            listed,
            [
                ("main", "init_ok", 0),
                ("main", "init_err", 0),
                ("fake:boom", "init_panic", 0),
                ("fish:obj", "object_api", 1),
            ]
        );
        let host = crate::host::get().unwrap();
        // SAFETY: trampolines are valid functions; the host table is live.
        unsafe {
            assert_eq!(entries[0].init.unwrap()(host), FishStatus::OK);
            assert_eq!(RAN.load(Ordering::SeqCst), 1);
            assert_eq!(entries[1].init.unwrap()(host), FishStatus::INTERNAL);
            assert_eq!(entries[2].init.unwrap()(host), FishStatus::PANICKED);
            assert_eq!(
                entries[0].init.unwrap()(std::ptr::null()),
                FishStatus::INVALID_ARGUMENT
            );
            let p = entries[3].object.unwrap()();
            assert_eq!(*p.cast::<u32>(), 99);
        }
        let logs = f.logs.lock().unwrap();
        assert!(
            logs.iter()
                .any(|(_, _, m)| m.contains("init_err failed: it broke"))
        );
        assert!(logs.iter().any(|(_, _, m)| m.contains("init exploded")));
    }
}
