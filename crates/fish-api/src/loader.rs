//! Access to the loader: mods, directories, environment, object share, entrypoint objects.

use std::ffi::c_void;
use std::path::PathBuf;

use fish_abi::services::{
    FishLoaderServiceV1, FishModInfoV1, MOD_STATUS_FAILED, MOD_STATUS_LOADING, SERVICE_LOADER,
    SIDE_SERVER,
};
use fish_abi::{FishStatus, FishStr};

use crate::error::{Error, check};
use crate::host;

/// Which side the game runs as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    /// A client (including its integrated server).
    Client,
    /// A dedicated server.
    Server,
}

/// Where a mod is in its life cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModStatus {
    /// Library loaded, entrypoints still running.
    Loading,
    /// Loaded and initialised.
    Ok,
    /// Failed; its listeners are skipped.
    Failed,
}

/// A mod known to the loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModInfo {
    /// Mod id.
    pub id: String,
    /// Mod version (SemVer).
    pub version: String,
    /// Display name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Current status.
    pub status: ModStatus,
}

/// Entry to the loader API. Obtain it with [`FishLoader::get`].
#[derive(Clone, Copy)]
pub struct FishLoader {
    svc: &'static FishLoaderServiceV1,
    host: *mut c_void,
}

// SAFETY: the service table is immutable and the host pointer is only handed back to the host,
// whose callbacks are thread-safe by contract.
unsafe impl Send for FishLoader {}
// SAFETY: see above.
unsafe impl Sync for FishLoader {}

/// Text of a host string, which lives as long as the loader.
///
/// # Safety
/// `s` must come from the host and be valid UTF-8 for `'static` (the ABI promises both).
unsafe fn host_str(s: FishStr) -> String {
    // SAFETY: guaranteed by the caller.
    unsafe { s.as_str() }
        .unwrap_or("<invalid utf-8>")
        .to_string()
}

fn info_from(raw: &FishModInfoV1) -> ModInfo {
    // SAFETY: strings in `FishModInfoV1` are host memory valid for the loader's lifetime.
    unsafe {
        ModInfo {
            id: host_str(raw.id),
            version: host_str(raw.version),
            name: host_str(raw.name),
            description: host_str(raw.description),
            status: match raw.status {
                MOD_STATUS_FAILED => ModStatus::Failed,
                MOD_STATUS_LOADING => ModStatus::Loading,
                _ => ModStatus::Ok,
            },
        }
    }
}

fn empty_info() -> FishModInfoV1 {
    FishModInfoV1 {
        id: FishStr::EMPTY,
        version: FishStr::EMPTY,
        name: FishStr::EMPTY,
        description: FishStr::EMPTY,
        status: 0,
    }
}

impl FishLoader {
    /// The loader handle.
    ///
    /// # Panics
    /// Outside a mod entrypoint (no host attached) or if the host lacks the loader service.
    pub fn get() -> FishLoader {
        Self::try_get().unwrap_or_else(|e| panic!("FishLoader::get(): {e}"))
    }

    /// Like [`get`](Self::get) but returns an error instead of panicking.
    pub fn try_get() -> Result<FishLoader, Error> {
        let h = host::get()?;
        let mut out: *const c_void = std::ptr::null();
        // SAFETY: valid host table; the id outlives the call; `out` is a valid out pointer.
        let status = unsafe {
            (h.get_service)(
                h.host,
                h.mod_token,
                FishStr::from_static(SERVICE_LOADER),
                1,
                &mut out,
            )
        };
        check("looking up fish:loader", status)?;
        if out.is_null() {
            return Err(Error::Unsupported("the fish:loader service"));
        }
        // SAFETY: the host returned a pointer to its static `FishLoaderServiceV1`.
        let svc = unsafe { &*out.cast::<FishLoaderServiceV1>() };
        Ok(FishLoader { svc, host: h.host })
    }

    /// All mods in load order.
    pub fn mods(&self) -> Vec<ModInfo> {
        // SAFETY: valid service table and host pointer.
        let n = unsafe { (self.svc.mod_count)(self.host) };
        (0..n)
            .filter_map(|i| {
                let mut raw = empty_info();
                // SAFETY: as above; `raw` is a valid out pointer.
                let st = unsafe { (self.svc.mod_at)(self.host, i, &mut raw) };
                st.is_ok().then(|| info_from(&raw))
            })
            .collect()
    }

    /// Looks up a mod by id.
    pub fn find_mod(&self, id: &str) -> Option<ModInfo> {
        let mut raw = empty_info();
        // SAFETY: valid service table; the id outlives the call; `raw` is a valid out pointer.
        let st = unsafe { (self.svc.find_mod)(self.host, FishStr::new(id), &mut raw) };
        st.is_ok().then(|| info_from(&raw))
    }

    /// True if mod `id` is loaded and not failed.
    pub fn is_mod_loaded(&self, id: &str) -> bool {
        self.find_mod(id)
            .is_some_and(|m| m.status != ModStatus::Failed)
    }

    /// The side the game runs as.
    pub fn environment(&self) -> Environment {
        // SAFETY: valid service table and host pointer.
        if unsafe { (self.svc.environment)(self.host) } == SIDE_SERVER {
            Environment::Server
        } else {
            Environment::Client
        }
    }

    /// True in development mode (`FISH_LOADER_DEV=1`).
    pub fn is_development(&self) -> bool {
        // SAFETY: valid service table and host pointer.
        unsafe { (self.svc.is_development)(self.host) != 0 }
    }

    /// The game directory.
    pub fn game_dir(&self) -> PathBuf {
        // SAFETY: valid service table; host string.
        PathBuf::from(unsafe { host_str((self.svc.game_dir)(self.host)) })
    }

    /// The config directory (`<game dir>/config`).
    pub fn config_dir(&self) -> PathBuf {
        // SAFETY: valid service table; host string.
        PathBuf::from(unsafe { host_str((self.svc.config_dir)(self.host)) })
    }

    /// The object share, for exchanging data with other mods.
    pub fn object_share(&self) -> ObjectShare {
        ObjectShare { loader: *self }
    }

    /// Object entrypoints registered under `key` by other mods, as `(mod id, object)`.
    ///
    /// The type `T` must be the type the providing mod documents for this key.
    pub fn entrypoint_objects<T: 'static>(&self, key: &str) -> Vec<(String, &'static T)> {
        unsafe extern "C" fn sink(user: *mut c_void, mod_id: FishStr, object: *const c_void) {
            // SAFETY: `user` is the `Vec<(String, usize)>` passed below; the id is host memory.
            unsafe {
                let v = &mut *user.cast::<Vec<(String, usize)>>();
                v.push((host_str(mod_id), object as usize));
            }
        }
        let mut found: Vec<(String, usize)> = Vec::new();
        // SAFETY: valid service table; `found` outlives the call and is only touched by `sink`.
        let st = unsafe {
            (self.svc.entrypoints)(self.host, FishStr::new(key), sink, (&raw mut found).cast())
        };
        if st != FishStatus::OK {
            return Vec::new();
        }
        found
            .into_iter()
            // SAFETY: the providing mod promises `T` for this key (documented contract).
            .map(|(id, p)| (id, unsafe { &*(p as *const T) }))
            .collect()
    }
}

/// Data exchange between mods, keyed by string with a type tag checked on every read.
#[derive(Clone, Copy)]
pub struct ObjectShare {
    loader: FishLoader,
}

impl ObjectShare {
    /// Publishes `value` under `key`, tagged with `T`'s type name.
    pub fn put<T: Send + Sync + 'static>(&self, key: &str, value: &'static T) -> Result<(), Error> {
        // SAFETY: `value` is `'static`, so the pointer stays valid for every reader.
        unsafe {
            self.put_tagged(
                key,
                std::any::type_name::<T>(),
                std::ptr::from_ref(value).cast_mut().cast(),
            )
        }
    }

    /// Publishes a raw pointer with an explicit tag (for cross-language sharing).
    ///
    /// # Safety
    /// `value` must stay valid, and match what `tag` promises, for as long as the loader runs.
    pub unsafe fn put_tagged(&self, key: &str, tag: &str, value: *mut c_void) -> Result<(), Error> {
        // SAFETY: valid service table; strings outlive the call.
        let st = unsafe {
            (self.loader.svc.object_share_put)(
                self.loader.host,
                FishStr::new(key),
                FishStr::new(tag),
                value,
            )
        };
        check("publishing to the object share", st)
    }

    /// Reads the value under `key`, which must have been published as a `T`.
    pub fn get<T: Send + Sync + 'static>(&self, key: &str) -> Result<&'static T, Error> {
        let p = self.get_tagged(key, std::any::type_name::<T>())?;
        // SAFETY: the tag matched, so the publisher stored a `&'static T`.
        Ok(unsafe { &*p.cast::<T>() })
    }

    /// Reads a raw pointer, checking the explicit tag.
    pub fn get_tagged(&self, key: &str, tag: &str) -> Result<*mut c_void, Error> {
        let mut out: *mut c_void = std::ptr::null_mut();
        // SAFETY: valid service table; strings outlive the call; `out` is a valid out pointer.
        let st = unsafe {
            (self.loader.svc.object_share_get)(
                self.loader.host,
                FishStr::new(key),
                FishStr::new(tag),
                &mut out,
            )
        };
        check("reading from the object share", st)?;
        Ok(out)
    }
}
