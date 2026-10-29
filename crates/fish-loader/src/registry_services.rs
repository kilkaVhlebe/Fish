//! Host side of `fish:registry`.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;

use fish_abi::registry::RegistryServiceV1;
use fish_abi::{FishServiceHeader, FishStatus, FishStr};

use crate::inner::Inner;

fn guard(f: impl FnOnce() -> FishStatus) -> FishStatus {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(FishStatus::PANICKED)
}

unsafe extern "C" fn register(
    host: *mut c_void,
    token: u64,
    registry: FishStr,
    id: FishStr,
    out: *mut u32,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`.
        let inner = unsafe { &*host.cast::<Inner>() };
        // SAFETY: valid strings for the call.
        let (Ok(registry), Ok(id)) = (unsafe { registry.as_str() }, unsafe { id.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if out.is_null()
            || !fish_abi::ident::is_valid_identifier(registry)
            || !fish_abi::ident::is_valid_identifier(id)
        {
            return FishStatus::INVALID_ARGUMENT;
        }
        if inner.net.frozen.load(Ordering::Acquire) {
            inner.warn(format!(
                "registries are frozen: {id} was not added to {registry}"
            ));
            return FishStatus::INVALID_ARGUMENT;
        }
        let Some(rec) = inner.record(token) else {
            return FishStatus::NOT_FOUND;
        };
        let synced = rec.manifest.environment == fish_metadata::Environment::Any;
        let raw = inner
            .net
            .registries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .register(registry, id, synced, token);
        // SAFETY: `out` checked non-null.
        unsafe { *out = raw };
        FishStatus::OK
    })
}

unsafe extern "C" fn raw_id(
    host: *mut c_void,
    registry: FishStr,
    id: FishStr,
    out: *mut u32,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let inner = unsafe { &*host.cast::<Inner>() };
        // SAFETY: valid strings for the call.
        let (Ok(registry), Ok(id)) = (unsafe { registry.as_str() }, unsafe { id.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let raw = inner
            .net
            .registries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .raw(registry, id);
        match raw {
            Some(r) if !out.is_null() => {
                // SAFETY: `out` checked non-null.
                unsafe { *out = r };
                FishStatus::OK
            }
            Some(_) => FishStatus::INVALID_ARGUMENT,
            None => FishStatus::NOT_FOUND,
        }
    })
}

unsafe extern "C" fn frozen(host: *mut c_void) -> u8 {
    // SAFETY: ABI contract.
    u8::from(
        unsafe { &*host.cast::<Inner>() }
            .net
            .frozen
            .load(Ordering::Acquire),
    )
}

/// The `fish:registry` table.
pub(crate) static REGISTRY: RegistryServiceV1 = RegistryServiceV1 {
    header: FishServiceHeader {
        size: size_of::<RegistryServiceV1>(),
        version: 1,
    },
    register,
    raw_id,
    frozen,
};
