//! Particle types: the server-side half of a particle.
//!
//! ```no_run
//! use fish_api::particle::Particles;
//!
//! # fn init() -> Result<(), fish_api::Error> {
//! let spark = Particles::register("mymod:spark")?;
//! // The raw id is the number payloads use; ask for it while the game runs.
//! let raw = spark.raw_id()?;
//! # let _ = raw; Ok(()) }
//! ```
//!
//! Register while `pre_launch` or `main` runs (the same freeze as [`registry`](crate::registry)).
//! The namespace of the id must be the id of the registering mod. A type gets a raw id in the Fish
//! registry `fish:particle_types`, synchronised with a server for mods that run on both sides.
//!
//! Horizon's particles are a closed set it renders itself, so a type of a mod has no client
//! factory yet: nothing is drawn for it and there is no call that spawns one (both wait for the
//! rendering phase). The mod's `assets/<mod id>/particles/<path>.json` is copied into the content
//! overlay and the loader warns when it is missing.

use fish_abi::particles::{ParticleServiceV1, REGISTRY, SERVICE_PARTICLE};
use fish_abi::{FishStatus, FishStr};

use crate::client::service;
use crate::content::finish;
use crate::error::Error;
use crate::host;

/// A registered particle type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticleType {
    id: String,
}

impl ParticleType {
    /// The type's id (`mymod:spark`).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The type's raw id in the Fish registry `fish:particle_types` (renumbered to the server's
    /// on joining one: do not store it).
    pub fn raw_id(&self) -> Result<u32, Error> {
        crate::registry::raw_id(REGISTRY, &self.id)?
            .ok_or_else(|| Error::host("reading a raw id", FishStatus::NOT_FOUND))
    }
}

/// Particle types.
pub struct Particles;

impl Particles {
    /// Registers the particle type `id` (`namespace:path`, the namespace is the mod's id).
    pub fn register(id: &str) -> Result<ParticleType, Error> {
        Self::register_with(id, false)
    }

    /// Registers the particle type `id`; `always_show` asks the client to draw it even when the
    /// player asked for minimal particles.
    pub fn register_with(id: &str, always_show: bool) -> Result<ParticleType, Error> {
        let (svc, host_ptr) = service::<ParticleServiceV1>(SERVICE_PARTICLE)?;
        let table = host::get()?;
        // SAFETY: valid service table; the string outlives the call.
        let status = unsafe {
            (svc.register)(
                host_ptr,
                table.mod_token,
                FishStr::new(id),
                u8::from(always_show),
            )
        };
        finish("registering a particle type", status)?;
        Ok(ParticleType { id: id.to_string() })
    }
}
