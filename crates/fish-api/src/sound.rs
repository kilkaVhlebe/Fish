//! Sound events: a mod registers the events its resource files describe and plays them.
//!
//! The mod's archive holds `assets/<mod id>/sounds.json` and the `.ogg` files under
//! `assets/<mod id>/sounds/`, exactly as a resource pack does:
//!
//! ```text
//! assets/mymod/sounds.json            {"ruby_chime": {"sounds": ["mymod:ruby_chime"], "subtitle": "..."}}
//! assets/mymod/sounds/ruby_chime.ogg
//! ```
//!
//! ```no_run
//! use fish_api::sound::Sounds;
//!
//! # fn init() -> Result<(), fish_api::Error> {
//! let chime = Sounds::register("mymod:ruby_chime")?;
//! # let _ = chime; Ok(()) }
//! // in an event listener of a game with a window:
//! # fn on_use(client: &fish_api::client::ClientHandle, chime: &fish_api::sound::SoundEvent) -> Result<(), fish_api::Error> {
//! chime.play_at(client, [1.5, 64.0, -3.5], 1.0, 1.0)?;
//! # Ok(()) }
//! ```
//!
//! Register while `pre_launch` or `main` runs (the same freeze as [`registry`](crate::registry)).
//! The namespace of the id must be the id of the registering mod. The loader copies the mod's
//! assets into the content overlay and warns when `sounds.json` does not name the event or an
//! `.ogg` file is missing; the registered events also get a raw id in the Fish registry
//! `fish:sound_events`.
//!
//! Sounds are played on the client of the player's own window; a server has no audio
//! (`UNSUPPORTED`) and a sound that the game does not know (the resources are not there) is
//! `NOT_FOUND`. There is no way yet to play a sound on other players' clients.

use fish_abi::FishStr;
use fish_abi::sounds::{GAIN_MAX, SERVICE_SOUND, SoundServiceV1};

use crate::client::{ClientHandle, service};
use crate::content::finish;
use crate::error::{Error, check};
use crate::host;

/// A registered sound event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundEvent {
    id: String,
}

impl SoundEvent {
    /// The event's id (`mymod:ruby_chime`).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Plays the event on the listener (no position), at `volume` and `pitch` (each in
    /// `(0, 10]`, 1 is normal).
    pub fn play(&self, client: &ClientHandle, volume: f32, pitch: f32) -> Result<(), Error> {
        Sounds::play(client, &self.id, None, volume, pitch)
    }

    /// Plays the event at `pos`, in the world.
    pub fn play_at(
        &self,
        client: &ClientHandle,
        pos: [f64; 3],
        volume: f32,
        pitch: f32,
    ) -> Result<(), Error> {
        Sounds::play(client, &self.id, Some(pos), volume, pitch)
    }
}

/// Sound events.
pub struct Sounds;

impl Sounds {
    /// Registers the sound event `id` (`namespace:path`, the namespace is the mod's id).
    pub fn register(id: &str) -> Result<SoundEvent, Error> {
        let (svc, host_ptr) = service::<SoundServiceV1>(SERVICE_SOUND)?;
        let table = host::get()?;
        // SAFETY: valid service table; the string outlives the call.
        let status = unsafe { (svc.register)(host_ptr, table.mod_token, FishStr::new(id)) };
        finish("registering a sound event", status)?;
        Ok(SoundEvent { id: id.to_string() })
    }

    /// Plays any sound event the game knows, a mod's or a vanilla one
    /// (`minecraft:entity.experience_orb.pickup`): at `pos` when there is one, else on the
    /// listener.
    pub fn play(
        client: &ClientHandle,
        id: &str,
        pos: Option<[f64; 3]>,
        volume: f32,
        pitch: f32,
    ) -> Result<(), Error> {
        let (svc, host_ptr) = service::<SoundServiceV1>(SERVICE_SOUND)?;
        let [x, y, z] = pos.unwrap_or([0.0; 3]);
        // SAFETY: valid service table and host pointer; the string outlives the call.
        let status = unsafe {
            (svc.play)(
                host_ptr,
                client.raw(),
                FishStr::new(id),
                u8::from(pos.is_some()),
                x,
                y,
                z,
                volume,
                pitch,
            )
        };
        check("playing a sound", status)
    }

    /// The largest volume or pitch.
    pub const GAIN_MAX: f32 = GAIN_MAX;
}
