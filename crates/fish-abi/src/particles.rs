//! The `fish:particle` service (phase P8): particle types registered by mods.
//!
//! This is the server-side half of a particle type: its identifier (`namespace:path`, the
//! namespace is the registering mod's id) and a raw id in the Fish registry [`REGISTRY`], which is
//! synchronised with a server like every registry entry, so a payload can name a particle by number.
//! Horizon's particles are a closed set that it renders itself: a type of a mod has no client
//! factory yet (that waits for the rendering phase, P10), so nothing is drawn for it and there is
//! no call that spawns one. The mod's `assets/<mod id>/particles/<path>.json` (the texture list of
//! a particle, the resource pack format) is copied into the content overlay beside its blocks and
//! items, and the loader warns when it is missing.
//!
//! Registration is allowed while the `pre_launch` and `main` entrypoints run (the same freeze rule
//! as the registries).

use core::ffi::c_void;

use crate::tables::FishServiceHeader;
use crate::types::{FishStatus, FishStr};

/// Service id.
pub const SERVICE_PARTICLE: &str = "fish:particle";

/// The Fish registry holding the particle types of mods.
pub const REGISTRY: &str = "fish:particle_types";

/// `fish:particle` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ParticleServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Registers the particle type `id` of mod `token`. `always_show` nonzero asks the client to
    /// draw the particle even when the player asked for minimal particles (vanilla's
    /// `overrideLimiter`). `INVALID_ARGUMENT` for a bad id (the namespace must be the mod's id,
    /// `minecraft:` is refused) and after the registries froze; `ALREADY_EXISTS` when the type
    /// exists.
    pub register: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        id: FishStr,
        always_show: u8,
    ) -> FishStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn the_table_is_one_function_pointer_after_the_header() {
        assert_eq!(
            size_of::<ParticleServiceV1>(),
            size_of::<FishServiceHeader>() + size_of::<usize>()
        );
    }

    #[test]
    fn the_registry_has_the_fish_namespace() {
        assert!(REGISTRY.starts_with("fish:"));
        assert!(SERVICE_PARTICLE.ends_with("particle"));
    }
}
