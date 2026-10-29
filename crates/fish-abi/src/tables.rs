//! The mod export table, the host table, and service/payload headers.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};

use crate::types::{FishStatus, FishStr};

/// Entrypoint kind: `init(host) -> status`.
pub const ENTRYPOINT_KIND_INIT: u8 = 0;
/// Entrypoint kind: `object() -> *const c_void`, a mod-defined API object for other mods.
pub const ENTRYPOINT_KIND_OBJECT: u8 = 1;

/// Event combine mode: every listener runs.
pub const COMBINE_NOTIFY: u8 = 0;
/// Event combine mode: stop at the first listener that does not return `PASS`.
pub const COMBINE_FIRST_NON_PASS: u8 = 1;

/// Log level values (same numbering as the `log` crate's `Level`).
pub mod level {
    /// Errors.
    pub const ERROR: u8 = 1;
    /// Warnings.
    pub const WARN: u8 = 2;
    /// Informational messages.
    pub const INFO: u8 = 3;
    /// Debug messages.
    pub const DEBUG: u8 = 4;
    /// Trace messages.
    pub const TRACE: u8 = 5;
}

/// `init` entrypoint signature.
pub type InitFn = unsafe extern "C" fn(host: *const FishHostV1) -> FishStatus;
/// `object` entrypoint signature.
pub type ObjectFn = unsafe extern "C" fn() -> *const c_void;
/// Signature of the exported [`MOD_SYMBOL`](crate::MOD_SYMBOL).
pub type ModExportFn = unsafe extern "C" fn() -> *const FishModV1;

/// One named entrypoint of a mod.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FishEntrypointV1 {
    /// Entrypoint key as in the manifest (`main`, `client`, `fish:modmenu`, …).
    pub key: FishStr,
    /// Name the manifest refers to.
    pub name: FishStr,
    /// [`ENTRYPOINT_KIND_INIT`] or [`ENTRYPOINT_KIND_OBJECT`].
    pub kind: u8,
    /// Set when `kind == ENTRYPOINT_KIND_INIT`.
    pub init: Option<InitFn>,
    /// Set when `kind == ENTRYPOINT_KIND_OBJECT`.
    pub object: Option<ObjectFn>,
}

/// The static table returned by a mod's `fish_mod_v1`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FishModV1 {
    /// `size_of::<FishModV1>()` as seen by the mod.
    pub size: usize,
    /// ABI major the mod was built for.
    pub abi_major: u16,
    /// ABI minor the mod was built for.
    pub abi_minor: u16,
    /// Pointer to `entrypoint_count` entries; must stay valid for the life of the library.
    pub entrypoints: *const FishEntrypointV1,
    /// Number of entries.
    pub entrypoint_count: usize,
}

// SAFETY: the table and everything it points to are immutable statics in the mod.
unsafe impl Sync for FishModV1 {}
// SAFETY: same as above.
unsafe impl Send for FishModV1 {}
// SAFETY: entrypoints hold only static strings and function pointers.
unsafe impl Sync for FishEntrypointV1 {}
// SAFETY: same as above.
unsafe impl Send for FishEntrypointV1 {}

/// `log` callback.
pub type LogFn =
    unsafe extern "C" fn(host: *mut c_void, token: u64, level: u8, target: FishStr, msg: FishStr);
/// `get_service` callback; writes the service table pointer to `out`.
pub type GetServiceFn = unsafe extern "C" fn(
    host: *mut c_void,
    token: u64,
    id: FishStr,
    min_version: u32,
    out: *mut *const c_void,
) -> FishStatus;
/// `event_define` callback.
pub type EventDefineFn =
    unsafe extern "C" fn(host: *mut c_void, token: u64, id: FishStr, combine: u8) -> FishStatus;
/// `event_lookup` callback; writes the event index to `out`.
pub type EventLookupFn =
    unsafe extern "C" fn(host: *mut c_void, id: FishStr, out: *mut u32) -> FishStatus;
/// An event listener. `payload` and `out` are event specific.
pub type FishListener =
    unsafe extern "C" fn(user: *mut c_void, payload: *mut c_void, out: *mut c_void) -> FishStatus;
/// Destructor for a listener's `user` pointer, called when the listener is dropped.
pub type DropUserFn = unsafe extern "C" fn(user: *mut c_void);
/// `event_register` callback.
pub type EventRegisterFn = unsafe extern "C" fn(
    host: *mut c_void,
    token: u64,
    id: FishStr,
    phase: FishStr,
    listener: FishListener,
    user: *mut c_void,
    drop_user: Option<DropUserFn>,
) -> FishStatus;
/// `event_phase_order` callback: `first` runs before `second`.
pub type EventPhaseOrderFn = unsafe extern "C" fn(
    host: *mut c_void,
    token: u64,
    id: FishStr,
    first: FishStr,
    second: FishStr,
) -> FishStatus;
/// `event_invoke` callback.
pub type EventInvokeFn = unsafe extern "C" fn(
    host: *mut c_void,
    token: u64,
    id: FishStr,
    payload: *mut c_void,
    out: *mut c_void,
) -> FishStatus;
/// `event_invoke_index` callback (pre-resolved index from `event_lookup`).
pub type EventInvokeIndexFn = unsafe extern "C" fn(
    host: *mut c_void,
    token: u64,
    index: u32,
    payload: *mut c_void,
    out: *mut c_void,
) -> FishStatus;

/// The table the host passes to every entrypoint. Every function takes `host` first and
/// the calling mod's `mod_token` where it acts on behalf of the mod.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FishHostV1 {
    /// `size_of::<FishHostV1>()` as seen by the host.
    pub size: usize,
    /// ABI major served.
    pub abi_major: u16,
    /// ABI minor served.
    pub abi_minor: u16,
    /// Opaque host pointer; pass back to every callback.
    pub host: *mut c_void,
    /// Identifies the calling mod to the host.
    pub mod_token: u64,
    /// Writes a log record.
    pub log: LogFn,
    /// Looks up a service table by id and minimum version.
    pub get_service: GetServiceFn,
    /// Defines a mod-owned event.
    pub event_define: EventDefineFn,
    /// Resolves an event id to an index.
    pub event_lookup: EventLookupFn,
    /// Registers a listener.
    pub event_register: EventRegisterFn,
    /// Orders two phases of an event.
    pub event_phase_order: EventPhaseOrderFn,
    /// Invokes an event by id.
    pub event_invoke: EventInvokeFn,
    /// Invokes an event by index.
    pub event_invoke_index: EventInvokeIndexFn,
}

// SAFETY: a host table is immutable after creation; the host guarantees thread-safe callbacks.
unsafe impl Sync for FishHostV1 {}
// SAFETY: see above.
unsafe impl Send for FishHostV1 {}

/// Header every service table starts with. Functions are only ever appended, so callers check
/// `size` before using a function added after version 1 of a service.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FishServiceHeader {
    /// Size of the whole table in bytes.
    pub size: usize,
    /// Service version.
    pub version: u32,
}

/// Header every event payload starts with; fields are only ever appended.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FishPayloadHeader {
    /// Size of the whole payload in bytes.
    pub size: u32,
}

// Layout guards: an accidental layout change must fail to compile.
const PTR: usize = size_of::<usize>();
const _: () = assert!(size_of::<FishStr>() == 2 * PTR);
const _: () = assert!(size_of::<crate::FishBytes>() == 2 * PTR);
const _: () = assert!(size_of::<FishStatus>() == 4);
const _: () = assert!(size_of::<crate::FishHandle>() == 8);
const _: () = assert!(size_of::<crate::ActionResult>() == 1);
const _: () = assert!(size_of::<FishPayloadHeader>() == 4);
const _: () = assert!(size_of::<Option<InitFn>>() == PTR);

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
const _: () = {
    assert!(size_of::<FishStr>() == 16);
    assert!(size_of::<FishServiceHeader>() == 16);

    assert!(size_of::<FishEntrypointV1>() == 56);
    assert!(offset_of!(FishEntrypointV1, key) == 0);
    assert!(offset_of!(FishEntrypointV1, name) == 16);
    assert!(offset_of!(FishEntrypointV1, kind) == 32);
    assert!(offset_of!(FishEntrypointV1, init) == 40);
    assert!(offset_of!(FishEntrypointV1, object) == 48);

    assert!(size_of::<FishModV1>() == 32);
    assert!(offset_of!(FishModV1, size) == 0);
    assert!(offset_of!(FishModV1, abi_major) == 8);
    assert!(offset_of!(FishModV1, abi_minor) == 10);
    assert!(offset_of!(FishModV1, entrypoints) == 16);
    assert!(offset_of!(FishModV1, entrypoint_count) == 24);

    assert!(size_of::<FishHostV1>() == 96);
    assert!(offset_of!(FishHostV1, size) == 0);
    assert!(offset_of!(FishHostV1, abi_major) == 8);
    assert!(offset_of!(FishHostV1, abi_minor) == 10);
    assert!(offset_of!(FishHostV1, host) == 16);
    assert!(offset_of!(FishHostV1, mod_token) == 24);
    assert!(offset_of!(FishHostV1, log) == 32);
    assert!(offset_of!(FishHostV1, get_service) == 40);
    assert!(offset_of!(FishHostV1, event_define) == 48);
    assert!(offset_of!(FishHostV1, event_lookup) == 56);
    assert!(offset_of!(FishHostV1, event_register) == 64);
    assert!(offset_of!(FishHostV1, event_phase_order) == 72);
    assert!(offset_of!(FishHostV1, event_invoke) == 80);
    assert!(offset_of!(FishHostV1, event_invoke_index) == 88);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_the_documented_values() {
        assert_eq!(ENTRYPOINT_KIND_INIT, 0);
        assert_eq!(ENTRYPOINT_KIND_OBJECT, 1);
        assert_eq!(COMBINE_NOTIFY, 0);
        assert_eq!(COMBINE_FIRST_NON_PASS, 1);
        assert_eq!(
            [
                level::ERROR,
                level::WARN,
                level::INFO,
                level::DEBUG,
                level::TRACE
            ],
            [1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn headers_are_prefixes() {
        assert_eq!(offset_of!(FishServiceHeader, size), 0);
        assert_eq!(offset_of!(FishServiceHeader, version), size_of::<usize>());
        assert_eq!(offset_of!(FishPayloadHeader, size), 0);
    }
}
