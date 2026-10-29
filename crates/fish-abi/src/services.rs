//! Service tables.
//!
//! Each Fish API module is a *service*: a `#[repr(C)]` table registered under the id
//! `fish:<module>`. A table starts with a [`FishServiceHeader`]; functions are only ever appended,
//! so a caller checks `header.size` before using a function added after version 1.
//! Every function takes the host's opaque `host` pointer first.

use core::ffi::c_void;

use crate::tables::FishServiceHeader;
use crate::types::{FishStatus, FishStr};

/// Service id of the loader service.
pub const SERVICE_LOADER: &str = "fish:loader";

/// Mod status: all entrypoints ran.
pub const MOD_STATUS_OK: u8 = 0;
/// Mod status: the mod failed and is skipped.
pub const MOD_STATUS_FAILED: u8 = 1;
/// Mod status: the library is loaded but entrypoints have not all run yet.
pub const MOD_STATUS_LOADING: u8 = 2;

/// Side values returned by `environment`.
pub const SIDE_CLIENT: u8 = 0;
/// Side value for a dedicated server.
pub const SIDE_SERVER: u8 = 1;

/// Description of one mod. Strings live in host memory for as long as the loader does.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FishModInfoV1 {
    /// Mod id.
    pub id: FishStr,
    /// Mod version.
    pub version: FishStr,
    /// Display name.
    pub name: FishStr,
    /// Description.
    pub description: FishStr,
    /// One of the `MOD_STATUS_*` values.
    pub status: u8,
}

/// Receives each object entrypoint: `(user, mod_id, object)`.
pub type ObjectSink =
    unsafe extern "C" fn(user: *mut c_void, mod_id: FishStr, object: *const c_void);

/// `fish:loader` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FishLoaderServiceV1 {
    /// Table header (`version == 1`).
    pub header: FishServiceHeader,
    /// Number of mods known to the loader (resolved ones, failed included).
    pub mod_count: unsafe extern "C" fn(host: *mut c_void) -> usize,
    /// Fills `out` with the mod at `index` (load order).
    pub mod_at: unsafe extern "C" fn(
        host: *mut c_void,
        index: usize,
        out: *mut FishModInfoV1,
    ) -> FishStatus,
    /// Fills `out` with the mod called `id`; `NOT_FOUND` if there is none.
    pub find_mod:
        unsafe extern "C" fn(host: *mut c_void, id: FishStr, out: *mut FishModInfoV1) -> FishStatus,
    /// [`SIDE_CLIENT`] or [`SIDE_SERVER`].
    pub environment: unsafe extern "C" fn(host: *mut c_void) -> u8,
    /// Nonzero in development mode.
    pub is_development: unsafe extern "C" fn(host: *mut c_void) -> u8,
    /// The game directory.
    pub game_dir: unsafe extern "C" fn(host: *mut c_void) -> FishStr,
    /// The config directory.
    pub config_dir: unsafe extern "C" fn(host: *mut c_void) -> FishStr,
    /// Publishes `value` under `key` with a type tag; `ALREADY_EXISTS` if the key is taken.
    pub object_share_put: unsafe extern "C" fn(
        host: *mut c_void,
        key: FishStr,
        type_tag: FishStr,
        value: *mut c_void,
    ) -> FishStatus,
    /// Reads a published value; `TYPE_MISMATCH` if the tag differs, `NOT_FOUND` if absent.
    pub object_share_get: unsafe extern "C" fn(
        host: *mut c_void,
        key: FishStr,
        type_tag: FishStr,
        out: *mut *mut c_void,
    ) -> FishStatus,
    /// Calls `sink` for every object entrypoint registered under `key`, in load order.
    pub entrypoints: unsafe extern "C" fn(
        host: *mut c_void,
        key: FishStr,
        sink: ObjectSink,
        user: *mut c_void,
    ) -> FishStatus,
}

// SAFETY: service tables are immutable function-pointer tables.
unsafe impl Sync for FishLoaderServiceV1 {}
// SAFETY: see above.
unsafe impl Send for FishLoaderServiceV1 {}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn layout_is_pinned() {
        assert_eq!(size_of::<FishModInfoV1>(), 72);
        assert_eq!(offset_of!(FishModInfoV1, status), 64);
        assert_eq!(offset_of!(FishLoaderServiceV1, header), 0);
        assert_eq!(offset_of!(FishLoaderServiceV1, mod_count), 16);
        assert_eq!(offset_of!(FishLoaderServiceV1, entrypoints), 16 + 9 * 8);
        assert_eq!(size_of::<FishLoaderServiceV1>(), 16 + 10 * 8);
    }

    #[test]
    fn status_values_are_distinct() {
        assert_ne!(MOD_STATUS_OK, MOD_STATUS_FAILED);
        assert_ne!(MOD_STATUS_OK, MOD_STATUS_LOADING);
        assert_ne!(SIDE_CLIENT, SIDE_SERVER);
        assert_eq!(SERVICE_LOADER, "fish:loader");
    }
}
