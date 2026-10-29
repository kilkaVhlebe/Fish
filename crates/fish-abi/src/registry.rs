//! The `fish:registry` service (phase P8): Fish registries keyed by identifier.
//!
//! Registration is allowed while the `pre_launch` and `main` entrypoints run; afterwards the
//! registries are frozen and `register` fails with `INVALID_ARGUMENT`.

use core::ffi::c_void;

use crate::tables::FishServiceHeader;
use crate::types::{FishStatus, FishStr};

/// Service id.
pub const SERVICE_REGISTRY: &str = "fish:registry";

/// `fish:registry` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RegistryServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Adds `id` to `registry` for mod `token`; writes its raw id to `out` (idempotent per id).
    pub register: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        registry: FishStr,
        id: FishStr,
        out: *mut u32,
    ) -> FishStatus,
    /// Reads the raw id of `id` (`NOT_FOUND` when absent).
    pub raw_id: unsafe extern "C" fn(
        host: *mut c_void,
        registry: FishStr,
        id: FishStr,
        out: *mut u32,
    ) -> FishStatus,
    /// Nonzero once registration is closed.
    pub frozen: unsafe extern "C" fn(host: *mut c_void) -> u8,
}
