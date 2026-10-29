//! The host table handed to mods and the `fish:loader` service.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

use fish_abi::services::{
    FishLoaderServiceV1, FishModInfoV1, ObjectSink, SIDE_CLIENT, SIDE_SERVER,
};
use fish_abi::{
    DropUserFn, FISH_ABI_MAJOR, FISH_ABI_MINOR, FishHostV1, FishListener, FishServiceHeader,
    FishStatus, FishStr,
};
use fish_metadata::Side;

use crate::bus::Registration;
use crate::inner::Inner;

/// Runs `f`; a panic becomes `default` (panics must never unwind into a mod).
fn guard<T>(default: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(default)
}

/// # Safety
/// `host` must be the `Inner` pointer the host table was built with.
unsafe fn inner<'a>(host: *mut c_void) -> &'a Inner {
    // SAFETY: guaranteed by the caller; the loader outlives every mod call.
    unsafe { &*host.cast::<Inner>() }
}

/// # Safety
/// `s` must be valid per the ABI.
unsafe fn text<'a>(s: FishStr) -> Option<&'a str> {
    // SAFETY: guaranteed by the caller.
    unsafe { s.as_str() }.ok()
}

/// Builds the host table for the mod with `token`.
pub(crate) fn host_table(inner: *const Inner, token: u64) -> FishHostV1 {
    FishHostV1 {
        size: size_of::<FishHostV1>(),
        abi_major: FISH_ABI_MAJOR,
        abi_minor: FISH_ABI_MINOR,
        host: inner.cast_mut().cast(),
        mod_token: token,
        log: cb_log,
        get_service: cb_get_service,
        event_define: cb_event_define,
        event_lookup: cb_event_lookup,
        event_register: cb_event_register,
        event_phase_order: cb_event_phase_order,
        event_invoke: cb_event_invoke,
        event_invoke_index: cb_event_invoke_index,
    }
}

fn level_of(l: u8) -> log::Level {
    match l {
        1 => log::Level::Error,
        2 => log::Level::Warn,
        3 => log::Level::Info,
        4 => log::Level::Debug,
        _ => log::Level::Trace,
    }
}

unsafe extern "C" fn cb_log(
    host: *mut c_void,
    token: u64,
    level: u8,
    _target: FishStr,
    msg: FishStr,
) {
    guard((), || {
        // SAFETY: ABI contract.
        let (inner, msg) = unsafe { (inner(host), text(msg)) };
        let target = match inner.record(token) {
            Some(rec) => format!("fish/{}", rec.manifest.id),
            None => "fish/unknown".to_string(),
        };
        inner.emit(
            level_of(level),
            &target,
            msg.unwrap_or("<invalid utf-8>").to_string(),
        );
    });
}

unsafe extern "C" fn cb_get_service(
    host: *mut c_void,
    _token: u64,
    id: FishStr,
    min_version: u32,
    out: *mut *const c_void,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let (inner, id) = unsafe { (inner(host), text(id)) };
        let Some(id) = id else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if out.is_null() {
            return FishStatus::INVALID_ARGUMENT;
        }
        let services = inner.services.read().expect("services");
        match services.get(id) {
            None => FishStatus::NOT_FOUND,
            Some(s) if s.version < min_version => FishStatus::VERSION_MISMATCH,
            Some(s) => {
                // SAFETY: `out` checked non-null.
                unsafe { *out = s.ptr as *const c_void };
                FishStatus::OK
            }
        }
    })
}

unsafe extern "C" fn cb_event_define(
    host: *mut c_void,
    _token: u64,
    id: FishStr,
    combine: u8,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let (inner, id) = unsafe { (inner(host), text(id)) };
        match id {
            Some(id) => inner.bus.define(id, combine),
            None => FishStatus::INVALID_ARGUMENT,
        }
    })
}

unsafe extern "C" fn cb_event_lookup(host: *mut c_void, id: FishStr, out: *mut u32) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let (inner, id) = unsafe { (inner(host), text(id)) };
        let (Some(id), false) = (id, out.is_null()) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        match inner.bus.lookup(id) {
            Some(i) => {
                // SAFETY: `out` checked non-null.
                unsafe { *out = i.0 };
                FishStatus::OK
            }
            None => FishStatus::NOT_FOUND,
        }
    })
}

unsafe extern "C" fn cb_event_register(
    host: *mut c_void,
    token: u64,
    id: FishStr,
    phase: FishStr,
    listener: FishListener,
    user: *mut c_void,
    drop_user: Option<DropUserFn>,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let (inner, id, phase) = unsafe { (inner(host), text(id), text(phase)) };
        let (Some(id), Some(phase)) = (id, phase) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let Some(rec) = inner.record(token) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        inner.bus.register(Registration {
            id,
            owner: token,
            failed: rec.failed.clone(),
            phase,
            func: listener,
            user,
            drop_user,
        })
    })
}

unsafe extern "C" fn cb_event_phase_order(
    host: *mut c_void,
    _token: u64,
    id: FishStr,
    first: FishStr,
    second: FishStr,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let (inner, id, a, b) = unsafe { (inner(host), text(id), text(first), text(second)) };
        match (id, a, b) {
            (Some(id), Some(a), Some(b)) => inner.bus.phase_order(id, a, b),
            _ => FishStatus::INVALID_ARGUMENT,
        }
    })
}

unsafe extern "C" fn cb_event_invoke(
    host: *mut c_void,
    _token: u64,
    id: FishStr,
    payload: *mut c_void,
    out: *mut c_void,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let (inner, id) = unsafe { (inner(host), text(id)) };
        match id {
            // SAFETY: payload/out validity is the calling mod's contract.
            Some(id) => unsafe { inner.bus.invoke_id(id, payload, out) },
            None => FishStatus::INVALID_ARGUMENT,
        }
    })
}

unsafe extern "C" fn cb_event_invoke_index(
    host: *mut c_void,
    _token: u64,
    index: u32,
    payload: *mut c_void,
    out: *mut c_void,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract; payload/out validity is the calling mod's contract.
        unsafe {
            inner(host)
                .bus
                .invoke(crate::bus::EventIndex(index), payload, out)
        }
    })
}

// --- fish:loader service -------------------------------------------------------------------

unsafe extern "C" fn svc_mod_count(host: *mut c_void) -> usize {
    // SAFETY: ABI contract.
    guard(0, || {
        unsafe { inner(host) }.mods.read().expect("mods").len()
    })
}

unsafe extern "C" fn svc_mod_at(
    host: *mut c_void,
    index: usize,
    out: *mut FishModInfoV1,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let inner = unsafe { inner(host) };
        let mods = inner.mods.read().expect("mods");
        match (mods.get(index), out.is_null()) {
            (Some(rec), false) => {
                // SAFETY: `out` checked non-null.
                unsafe { *out = rec.info() };
                FishStatus::OK
            }
            (None, _) => FishStatus::NOT_FOUND,
            (_, true) => FishStatus::INVALID_ARGUMENT,
        }
    })
}

unsafe extern "C" fn svc_find_mod(
    host: *mut c_void,
    id: FishStr,
    out: *mut FishModInfoV1,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let (inner, id) = unsafe { (inner(host), text(id)) };
        let (Some(id), false) = (id, out.is_null()) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let mods = inner.mods.read().expect("mods");
        match mods.iter().find(|m| m.manifest.id.as_str() == id) {
            Some(rec) => {
                // SAFETY: `out` checked non-null.
                unsafe { *out = rec.info() };
                FishStatus::OK
            }
            None => FishStatus::NOT_FOUND,
        }
    })
}

unsafe extern "C" fn svc_environment(host: *mut c_void) -> u8 {
    // SAFETY: ABI contract.
    match unsafe { inner(host) }.side {
        Side::Client => SIDE_CLIENT,
        Side::Server => SIDE_SERVER,
    }
}

unsafe extern "C" fn svc_is_development(host: *mut c_void) -> u8 {
    // SAFETY: ABI contract.
    u8::from(unsafe { inner(host) }.dev)
}

unsafe extern "C" fn svc_game_dir(host: *mut c_void) -> FishStr {
    // SAFETY: ABI contract; the string lives as long as the loader.
    FishStr::new(&unsafe { inner(host) }.game_dir)
}

unsafe extern "C" fn svc_config_dir(host: *mut c_void) -> FishStr {
    // SAFETY: ABI contract; the string lives as long as the loader.
    FishStr::new(&unsafe { inner(host) }.config_dir)
}

unsafe extern "C" fn svc_share_put(
    host: *mut c_void,
    key: FishStr,
    tag: FishStr,
    value: *mut c_void,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let (inner, key, tag) = unsafe { (inner(host), text(key), text(tag)) };
        let (Some(key), Some(tag)) = (key, tag) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if key.is_empty() || tag.is_empty() || value.is_null() {
            return FishStatus::INVALID_ARGUMENT;
        }
        let mut share = inner.share.lock().expect("share");
        if share.contains_key(key) {
            return FishStatus::ALREADY_EXISTS;
        }
        share.insert(key.to_string(), (tag.to_string(), value as usize));
        FishStatus::OK
    })
}

unsafe extern "C" fn svc_share_get(
    host: *mut c_void,
    key: FishStr,
    tag: FishStr,
    out: *mut *mut c_void,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let (inner, key, tag) = unsafe { (inner(host), text(key), text(tag)) };
        let (Some(key), Some(tag), false) = (key, tag, out.is_null()) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        match inner.share_get(key, tag) {
            Ok(p) => {
                // SAFETY: `out` checked non-null.
                unsafe { *out = p };
                FishStatus::OK
            }
            Err(st) => st,
        }
    })
}

unsafe extern "C" fn svc_entrypoints(
    host: *mut c_void,
    key: FishStr,
    sink: ObjectSink,
    user: *mut c_void,
) -> FishStatus {
    guard(FishStatus::PANICKED, || {
        // SAFETY: ABI contract.
        let (inner, key) = unsafe { (inner(host), text(key)) };
        let Some(key) = key else {
            return FishStatus::INVALID_ARGUMENT;
        };
        for rec in inner.records() {
            if rec.is_failed() {
                continue;
            }
            let object = {
                let entries = rec.entries.lock().expect("entries");
                entries.iter().find(|e| e.key == key).and_then(|e| e.object)
            };
            if let Some(object) = object {
                // SAFETY: `object` is the mod's own entrypoint function.
                let ptr = unsafe { object() };
                if !ptr.is_null() {
                    // SAFETY: the caller's sink contract.
                    unsafe { sink(user, FishStr::new(rec.manifest.id.as_str()), ptr) };
                }
            }
        }
        FishStatus::OK
    })
}

impl Inner {
    /// Reads a shared value, checking the tag.
    pub(crate) fn share_get(&self, key: &str, tag: &str) -> Result<*mut c_void, FishStatus> {
        let share = self.share.lock().expect("share");
        match share.get(key) {
            None => Err(FishStatus::NOT_FOUND),
            Some((t, _)) if t != tag => Err(FishStatus::TYPE_MISMATCH),
            Some((_, p)) => Ok(*p as *mut c_void),
        }
    }
}

/// The `fish:loader` service table.
pub(crate) static LOADER_SERVICE: FishLoaderServiceV1 = FishLoaderServiceV1 {
    header: FishServiceHeader {
        size: size_of::<FishLoaderServiceV1>(),
        version: 1,
    },
    mod_count: svc_mod_count,
    mod_at: svc_mod_at,
    find_mod: svc_find_mod,
    environment: svc_environment,
    is_development: svc_is_development,
    game_dir: svc_game_dir,
    config_dir: svc_config_dir,
    object_share_put: svc_share_put,
    object_share_get: svc_share_get,
    entrypoints: svc_entrypoints,
};
