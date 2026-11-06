//! The `fish:sound` service (phase P8): sound events registered by mods.
//!
//! A sound event is an identifier (`namespace:path`, the namespace is the registering mod's id)
//! that the mod's own resources describe: `assets/<mod id>/sounds.json` names the event and its
//! files, and the files (`.ogg`) sit under `assets/<mod id>/sounds/`. The loader copies them into
//! the same content overlay as the blocks and items of the mod, and Horizon reads that overlay's
//! `sounds.json` into its sound tables. Registering the event tells the loader the mod wants it:
//! the loader checks the files when it writes the content, gives the event a raw id in the Fish
//! registry [`REGISTRY`] (synchronised like any registry entry) and lets the mod play it.
//!
//! Registration is allowed while the `pre_launch` and `main` entrypoints run (the same freeze rule
//! as the registries).

use core::ffi::c_void;

use crate::tables::FishServiceHeader;
use crate::types::{FishHandle, FishStatus, FishStr};

/// Service id.
pub const SERVICE_SOUND: &str = "fish:sound";

/// The Fish registry holding the sound events of mods.
pub const REGISTRY: &str = "fish:sound_events";

/// The largest volume or pitch a mod may ask for.
pub const GAIN_MAX: f32 = 10.0;

/// `fish:sound` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SoundServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Registers the sound event `id` of mod `token`. `INVALID_ARGUMENT` for a bad id (the
    /// namespace must be the mod's id, `minecraft:` is refused) and after the registries froze;
    /// `ALREADY_EXISTS` when the event exists.
    pub register: unsafe extern "C" fn(host: *mut c_void, token: u64, id: FishStr) -> FishStatus,
    /// Plays the sound event `id` (any event Horizon knows, a mod's or a vanilla one) in the game
    /// `view`: at the block position `x`, `y`, `z` when `positional` is nonzero, else on the
    /// listener. `volume` and `pitch` are in `(0, `[`GAIN_MAX`]`]`. `INVALID_ARGUMENT` for a bad id
    /// or number, `NOT_FOUND` when the game does not know the event and `UNSUPPORTED` where there
    /// is no audio (a server).
    pub play: unsafe extern "C" fn(
        host: *mut c_void,
        view: FishHandle,
        id: FishStr,
        positional: u8,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
    ) -> FishStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn the_table_is_function_pointers_after_the_header() {
        assert_eq!(
            size_of::<SoundServiceV1>(),
            size_of::<FishServiceHeader>() + 2 * size_of::<usize>()
        );
    }

    #[test]
    fn the_registry_has_the_fish_namespace() {
        assert!(REGISTRY.starts_with("fish:") && GAIN_MAX > 1.0);
    }
}
