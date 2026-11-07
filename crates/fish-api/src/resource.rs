//! Resource files: a mod's own `assets/` and `data/`, and the game's active resources (Fabric's
//! resource loader).
//!
//! Every file under `assets/` and `data/` in a mod's archive reaches the game: the loader puts it
//! into the mod's content folder, which Horizon reads like a content mod's (textures, models,
//! sounds, translations, loot tables, recipes, tags), whether or not the mod registers blocks or
//! items. A mod reads its own files at any time, and the game's resources (vanilla, resource
//! packs and the content of mods over them) through the game of an event:
//!
//! ```no_run
//! use fish_api::event::lifecycle;
//! use fish_api::resource::Resources;
//!
//! # fn init() -> Result<(), fish_api::Error> {
//! let sounds = Resources::own_text("assets/mymod/sounds.json")?;
//! let lang_files = Resources::list_own("assets/mymod/lang/")?;
//! # let _ = (sounds, lang_files);
//! lifecycle::resources::RELOADED.register(|c| {
//!     // the resource packs changed: read what the game uses now
//!     let text = Resources::game_text(&c.client(), "assets/minecraft/lang/en_us.json");
//!     log::info!("en_us.json is {} bytes", text.map(|t| t.len()).unwrap_or(0));
//! })?;
//! # Ok(()) }
//! ```
//!
//! Paths are archive paths, `assets/<namespace>/...` or `data/<namespace>/...`, without `.` or
//! `..` components; files are at most 64 MiB.

use std::ffi::c_void;

use fish_abi::FishStr;
use fish_abi::resources::{ResourceServiceV1, SERVICE_RESOURCE};

use crate::client::{ClientHandle, service};
use crate::error::{Error, check};
use crate::host;

unsafe extern "C" fn collect_bytes(user: *mut c_void, data: *const u8, len: usize) {
    // SAFETY: `user` is the caller's `Vec<u8>`; the host's bytes are valid for `len` bytes.
    unsafe {
        (*user.cast::<Vec<u8>>()).extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

unsafe extern "C" fn collect_name(user: *mut c_void, name: FishStr) {
    // SAFETY: `user` is the caller's `Vec<String>`; the host's string is valid for the call.
    unsafe {
        if let Ok(name) = name.as_str() {
            (*user.cast::<Vec<String>>()).push(name.to_string());
        }
    }
}

fn utf8(bytes: Vec<u8>, path: &str) -> Result<String, Error> {
    String::from_utf8(bytes)
        .map_err(|_| Error::InvalidArgument(format!("{path} is not UTF-8 text")))
}

/// Resource files.
pub struct Resources;

impl Resources {
    /// The bytes of `path` in this mod's archive (`assets/mymod/sounds.json`). `NOT_FOUND` for a
    /// file the archive does not hold.
    pub fn own(path: &str) -> Result<Vec<u8>, Error> {
        let (svc, host_ptr) = service::<ResourceServiceV1>(SERVICE_RESOURCE)?;
        let table = host::get()?;
        let mut out = Vec::new();
        // SAFETY: valid service table; the path and the output outlive the call.
        let status = unsafe {
            (svc.read_own)(
                host_ptr,
                table.mod_token,
                FishStr::new(path),
                collect_bytes,
                (&raw mut out).cast(),
            )
        };
        check("reading a resource of the mod", status)?;
        Ok(out)
    }

    /// [`Resources::own`] as UTF-8 text.
    pub fn own_text(path: &str) -> Result<String, Error> {
        utf8(Resources::own(path)?, path)
    }

    /// The paths of this mod's resource files that start with `prefix` (`assets/mymod/lang/`), in
    /// archive order.
    pub fn list_own(prefix: &str) -> Result<Vec<String>, Error> {
        let (svc, host_ptr) = service::<ResourceServiceV1>(SERVICE_RESOURCE)?;
        let table = host::get()?;
        let mut out = Vec::new();
        // SAFETY: valid service table; the prefix and the output outlive the call.
        let status = unsafe {
            (svc.list_own)(
                host_ptr,
                table.mod_token,
                FishStr::new(prefix),
                collect_name,
                (&raw mut out).cast(),
            )
        };
        check("listing the resources of the mod", status)?;
        Ok(out)
    }

    /// The bytes of `path` in the game's active resources, through the game of an event.
    /// `UNSUPPORTED` where the game has none (a dedicated server).
    pub fn game(client: &ClientHandle, path: &str) -> Result<Vec<u8>, Error> {
        let (svc, host_ptr) = service::<ResourceServiceV1>(SERVICE_RESOURCE)?;
        let mut out = Vec::new();
        // SAFETY: valid service table; the path and the output outlive the call.
        let status = unsafe {
            (svc.read_game)(
                host_ptr,
                client.raw(),
                FishStr::new(path),
                collect_bytes,
                (&raw mut out).cast(),
            )
        };
        check("reading a resource of the game", status)?;
        Ok(out)
    }

    /// [`Resources::game`] as UTF-8 text.
    pub fn game_text(client: &ClientHandle, path: &str) -> Result<String, Error> {
        utf8(Resources::game(client, path)?, path)
    }
}
