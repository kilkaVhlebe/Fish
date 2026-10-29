//! Fish registries: identifiers mapped to raw ids, shared with the server in multiplayer.
//!
//! Register while your `pre_launch` or `main` entrypoint runs; later calls fail with
//! `"registries are frozen"`. Raw ids follow registration order (mod load order, then call order) and are
//! renumbered to the server's on join, so never store a raw id.

use fish_abi::registry::{RegistryServiceV1, SERVICE_REGISTRY};
use fish_abi::{FishStatus, FishStr};

use crate::client::service;
use crate::error::{Error, check};
use crate::host;

/// Adds `id` (`namespace:path`) to `registry` and returns its raw id. Registering twice is fine.
pub fn register(registry: &str, id: &str) -> Result<u32, Error> {
    let (svc, host_ptr) = service::<RegistryServiceV1>(SERVICE_REGISTRY)?;
    let table = host::get()?;
    let mut raw = 0u32;
    // SAFETY: valid service table; the strings and `raw` outlive the call.
    let status = unsafe {
        (svc.register)(
            host_ptr,
            table.mod_token,
            FishStr::new(registry),
            FishStr::new(id),
            &mut raw,
        )
    };
    if status == FishStatus::INVALID_ARGUMENT && is_frozen()? {
        return Err(Error::InvalidArgument("registries are frozen".to_string()));
    }
    check("registering an entry", status)?;
    Ok(raw)
}

/// The current raw id of `id`, or `None` when it is not registered.
pub fn raw_id(registry: &str, id: &str) -> Result<Option<u32>, Error> {
    let (svc, host_ptr) = service::<RegistryServiceV1>(SERVICE_REGISTRY)?;
    let mut raw = 0u32;
    // SAFETY: as in `register`.
    let status =
        unsafe { (svc.raw_id)(host_ptr, FishStr::new(registry), FishStr::new(id), &mut raw) };
    match status {
        FishStatus::OK => Ok(Some(raw)),
        FishStatus::NOT_FOUND => Ok(None),
        other => Err(Error::host("reading a raw id", other)),
    }
}

/// True once registration is closed.
pub fn is_frozen() -> Result<bool, Error> {
    let (svc, host_ptr) = service::<RegistryServiceV1>(SERVICE_REGISTRY)?;
    // SAFETY: valid service table.
    Ok(unsafe { (svc.frozen)(host_ptr) } != 0)
}
