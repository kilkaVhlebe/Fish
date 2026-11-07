//! The `fish:resource` service (phase P9): the resource files of mods and of the game.
//!
//! Every Fish mod whose archive holds `assets/` or `data/` files gets them into the game: the
//! loader copies them into the mod's Horizon content folder, which the game reads like a content
//! mod's (textures, models, sounds, translations, loot tables, recipes, tags). This service lets a
//! mod read its own files from its archive and the files of the game's active resource root (the
//! vanilla resources with the resource packs and the content of mods over them).
//!
//! Paths are archive paths: `assets/<namespace>/...` or `data/<namespace>/...`, `/`-separated,
//! without `.` or `..` components.

use crate::client::StrSink;
use crate::tables::FishServiceHeader;
use crate::types::{FishHandle, FishStatus, FishStr};
use core::ffi::c_void;

/// Service id.
pub const SERVICE_RESOURCE: &str = "fish:resource";

/// The largest file the service hands out, in bytes (larger files are `INVALID_ARGUMENT`).
pub const MAX_FILE: u64 = 64 << 20;

/// Receives the bytes of a file, valid during the call.
pub type BytesSink = unsafe extern "C" fn(user: *mut c_void, data: *const u8, len: usize);

/// `fish:resource` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ResourceServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Reads `path` from the archive of mod `token` into `sink`. `INVALID_ARGUMENT` for a path
    /// outside `assets/` and `data/` or with `.`/`..`, `NOT_FOUND` for a file the archive does
    /// not hold.
    pub read_own: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        path: FishStr,
        sink: BytesSink,
        user: *mut c_void,
    ) -> FishStatus,
    /// Calls `sink` with every file of mod `token`'s archive under `assets/` and `data/` whose
    /// path starts with `prefix`, in archive order.
    pub list_own: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        prefix: FishStr,
        sink: StrSink,
        user: *mut c_void,
    ) -> FishStatus,
    /// Reads `path` from the active resource root of the game `view` (a handle of an event's
    /// payload). `UNSUPPORTED` when the game has no resource root (a dedicated server, the
    /// loader's own tests), `NOT_FOUND` for a missing file.
    pub read_game: unsafe extern "C" fn(
        host: *mut c_void,
        view: FishHandle,
        path: FishStr,
        sink: BytesSink,
        user: *mut c_void,
    ) -> FishStatus,
}

/// True for a path the service accepts: under `assets/` or `data/`, non-empty components, no
/// `.` or `..`, no backslashes.
pub fn valid_path(path: &str) -> bool {
    let Some(rest) = path
        .strip_prefix("assets/")
        .or_else(|| path.strip_prefix("data/"))
    else {
        return false;
    };
    !rest.is_empty()
        && !path.contains('\\')
        && rest
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_below_assets_and_data() {
        for ok in [
            "assets/m/sounds.json",
            "data/m/loot_table/blocks/x.json",
            "assets/m/a.b/c",
        ] {
            assert!(valid_path(ok), "{ok}");
        }
        for bad in [
            "",
            "assets/",
            "other/m/x",
            "assets/../x",
            "data/m/./x",
            "assets/m//x",
            "assets\\m\\x",
            "/assets/m/x",
        ] {
            assert!(!valid_path(bad), "{bad}");
        }
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn the_table_is_function_pointers_after_the_header() {
        assert_eq!(
            size_of::<ResourceServiceV1>(),
            size_of::<FishServiceHeader>() + 3 * size_of::<usize>()
        );
        const { assert!(MAX_FILE >= 1 << 20) };
    }
}
