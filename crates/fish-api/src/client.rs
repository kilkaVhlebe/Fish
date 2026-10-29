//! Client-side API: the game handle, the HUD, raw commands and keys.
//!
//! Event listeners receive context types (`TickContext`, `HudRenderContext`, …). Their handles
//! are valid only while the listener runs; using one afterwards returns an error.

use std::ffi::c_void;
use std::path::PathBuf;

use fish_abi::client::{
    ClientServiceV1, CommandPayload, CommandRawServiceV1, FramePayload, HudPayload, HudServiceV1,
    InputServiceV1, KeyPayload, PlayerV1, ROLE_DEDICATED_SERVER, ROLE_INTEGRATED_SERVER,
    TickPayload, services as ids,
};
use fish_abi::{FishHandle, FishStatus, FishStr};

use crate::error::{Error, check};
use crate::event::Payload;
use crate::host;

/// Which kind of game a listener runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The window's game.
    Window,
    /// The integrated server of a window.
    IntegratedServer,
    /// A dedicated server.
    DedicatedServer,
}

/// The local player.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
    /// Yaw in degrees.
    pub yaw: f32,
    /// Pitch in degrees.
    pub pitch: f32,
    /// Dimension id, e.g. `minecraft:overworld`.
    pub dimension: String,
}

pub(crate) fn service<T>(id: &'static str) -> Result<(&'static T, *mut c_void), Error> {
    let h = host::get()?;
    let mut out: *const c_void = std::ptr::null();
    // SAFETY: valid host table; the id is static; `out` is a valid out pointer.
    let st = unsafe { (h.get_service)(h.host, h.mod_token, FishStr::from_static(id), 1, &mut out) };
    check("looking up a service", st)?;
    if out.is_null() {
        return Err(Error::Unsupported(id));
    }
    // SAFETY: the host returned a pointer to its static table for `id`, which is a `T`.
    Ok((unsafe { &*out.cast::<T>() }, h.host))
}

unsafe extern "C" fn collect(user: *mut c_void, text: FishStr) {
    // SAFETY: `user` is the `String` passed by the caller; the host string is valid UTF-8.
    unsafe {
        if let Ok(s) = text.as_str() {
            (*user.cast::<String>()).push_str(s);
        }
    }
}

/// A handle to the game that raised an event; valid only during the callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientHandle(FishHandle);

impl ClientHandle {
    pub(crate) fn from_raw(handle: FishHandle) -> ClientHandle {
        ClientHandle(handle)
    }

    /// The game's role.
    pub fn role(&self) -> Result<Role, Error> {
        let (svc, host) = service::<ClientServiceV1>(ids::CLIENT)?;
        // SAFETY: valid service table and host pointer.
        match unsafe { (svc.role)(host, self.0) } {
            ROLE_INTEGRATED_SERVER => Ok(Role::IntegratedServer),
            ROLE_DEDICATED_SERVER => Ok(Role::DedicatedServer),
            u8::MAX => Err(Error::host(
                "reading the game role",
                FishStatus::STALE_HANDLE,
            )),
            _ => Ok(Role::Window),
        }
    }

    /// The player, or `None` outside a world.
    pub fn player(&self) -> Result<Option<Player>, Error> {
        let (svc, host) = service::<ClientServiceV1>(ids::CLIENT)?;
        let mut raw = PlayerV1::default();
        let mut dimension = String::new();
        // SAFETY: valid service table; `raw` and `dimension` outlive the call and the sink only
        // appends to `dimension`.
        let st =
            unsafe { (svc.player)(host, self.0, &mut raw, collect, (&raw mut dimension).cast()) };
        match st {
            FishStatus::OK => Ok(Some(Player {
                x: raw.x,
                y: raw.y,
                z: raw.z,
                yaw: raw.yaw,
                pitch: raw.pitch,
                dimension,
            })),
            FishStatus::NOT_FOUND => Ok(None),
            other => Err(Error::host("reading the player", other)),
        }
    }

    /// Shows a line in the player's chat.
    pub fn show_chat(&self, text: &str) -> Result<(), Error> {
        let (svc, host) = service::<ClientServiceV1>(ids::CLIENT)?;
        // SAFETY: valid service table; the text outlives the call.
        check("showing chat", unsafe {
            (svc.show_chat)(host, self.0, FishStr::new(text))
        })
    }

    /// The game directory.
    pub fn game_dir(&self) -> Result<PathBuf, Error> {
        let (svc, host) = service::<ClientServiceV1>(ids::CLIENT)?;
        let mut dir = String::new();
        // SAFETY: valid service table; `dir` outlives the call.
        let st = unsafe { (svc.game_dir)(host, self.0, collect, (&raw mut dir).cast()) };
        check("reading the game directory", st)?;
        Ok(PathBuf::from(dir))
    }
}

/// Drawing on the HUD, in GUI pixels; valid only during the HUD event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawContext(FishHandle);

impl DrawContext {
    /// Fills `rect` (`[x, y, width, height]`) with `argb`.
    pub fn fill(&self, rect: [f32; 4], argb: u32) -> Result<(), Error> {
        let (svc, host) = service::<HudServiceV1>(ids::HUD)?;
        let [x, y, w, h] = rect;
        // SAFETY: valid service table and host pointer.
        check("filling a rectangle", unsafe {
            (svc.fill)(host, self.0, x, y, w, h, argb)
        })
    }

    /// Draws `text` at `(x, y)` in opaque `rgb`.
    pub fn text(&self, text: &str, x: f32, y: f32, rgb: u32, shadow: bool) -> Result<(), Error> {
        let (svc, host) = service::<HudServiceV1>(ids::HUD)?;
        // SAFETY: valid service table; the text outlives the call.
        check("drawing text", unsafe {
            (svc.text)(
                host,
                self.0,
                FishStr::new(text),
                x,
                y,
                rgb,
                u8::from(shadow),
            )
        })
    }

    /// The width of `text`.
    pub fn text_width(&self, text: &str) -> Result<f32, Error> {
        let (svc, host) = service::<HudServiceV1>(ids::HUD)?;
        let mut out = 0.0f32;
        // SAFETY: valid service table; the text and `out` outlive the call.
        check("measuring text", unsafe {
            (svc.text_width)(host, self.0, FishStr::new(text), &mut out)
        })?;
        Ok(out)
    }

    /// The GUI size `[width, height]`.
    pub fn gui_size(&self) -> Result<[f32; 2], Error> {
        let (svc, host) = service::<HudServiceV1>(ids::HUD)?;
        let mut out = [0.0f32; 2];
        // SAFETY: valid service table; `out` outlives the call.
        check("reading the GUI size", unsafe {
            (svc.gui_size)(host, self.0, &mut out)
        })?;
        Ok(out)
    }
}

macro_rules! context {
    ($(#[$m:meta])* $name:ident($payload:ty)) => {
        $(#[$m])*
        #[repr(transparent)]
        pub struct $name($payload);

        // SAFETY: `repr(transparent)` over an ABI payload that starts with `size: u32`.
        unsafe impl Payload for $name {}
    };
}

context! {
    /// Payload of the tick events.
    TickContext(TickPayload)
}
context! {
    /// Payload of the frame event.
    FrameContext(FramePayload)
}
context! {
    /// Payload of the HUD render event.
    HudRenderContext(HudPayload)
}
context! {
    /// Payload of the raw command event.
    RawCommandContext(CommandPayload)
}
context! {
    /// Payload of the key event.
    KeyContext(KeyPayload)
}

impl TickContext {
    /// The game this tick belongs to.
    pub fn client(&self) -> ClientHandle {
        ClientHandle(self.0.view)
    }

    /// Ticks this role has run, counting this one (the first tick is 1).
    pub fn tick(&self) -> u64 {
        self.0.tick
    }

    /// Which kind of game ticked.
    pub fn role(&self) -> Role {
        match self.0.role {
            ROLE_INTEGRATED_SERVER => Role::IntegratedServer,
            ROLE_DEDICATED_SERVER => Role::DedicatedServer,
            _ => Role::Window,
        }
    }
}

impl FrameContext {
    /// The game.
    pub fn client(&self) -> ClientHandle {
        ClientHandle(self.0.view)
    }

    /// Seconds since the previous frame.
    pub fn elapsed_secs(&self) -> f32 {
        self.0.elapsed_secs
    }
}

impl HudRenderContext {
    /// The game.
    pub fn client(&self) -> ClientHandle {
        ClientHandle(self.0.view)
    }

    /// The HUD draw target.
    pub fn draw(&self) -> DrawContext {
        DrawContext(self.0.hud)
    }
}

impl RawCommandContext {
    /// The game.
    pub fn client(&self) -> ClientHandle {
        ClientHandle(self.0.view)
    }

    /// The command, without a leading slash.
    pub fn command(&self) -> &str {
        // SAFETY: the host passes a valid UTF-8 string that lives for the callback; the returned
        // borrow is tied to `&self`, which cannot outlive the callback.
        unsafe { self.0.command.as_str() }.unwrap_or("")
    }

    /// Shows command feedback to the player.
    pub fn feedback(&self, text: &str) -> Result<(), Error> {
        let (svc, host) = service::<CommandRawServiceV1>(ids::COMMAND_RAW)?;
        // SAFETY: valid service table; the text outlives the call.
        check("showing command feedback", unsafe {
            (svc.feedback)(host, self.0.view, FishStr::new(text))
        })
    }
}

impl KeyContext {
    /// The game.
    pub fn client(&self) -> ClientHandle {
        ClientHandle(self.0.view)
    }

    /// The vanilla key name, e.g. `key.keyboard.z`.
    pub fn key(&self) -> &str {
        // SAFETY: as in `RawCommandContext::command`.
        unsafe { self.0.key.as_str() }.unwrap_or("")
    }

    /// True on press, false on release.
    pub fn pressed(&self) -> bool {
        self.0.pressed != 0
    }
}

/// Whether the named key or mouse button is currently held (as last reported to the key event).
pub fn is_key_down(key: &str) -> Result<bool, Error> {
    let (svc, host) = service::<InputServiceV1>(ids::INPUT)?;
    // SAFETY: valid service table; the key outlives the call.
    Ok(unsafe { (svc.is_down)(host, FishStr::new(key)) } != 0)
}

/// HUD events.
pub mod hud {
    use super::HudRenderContext;
    use crate::event::{Combine, Event};

    /// Fired once per frame while the HUD is drawn.
    pub static RENDER: Event<HudRenderContext> =
        Event::new(fish_abi::client::ids::HUD_RENDER, Combine::Notify);
}

/// Raw client command events (the unparsed text the player typed).
pub mod command {
    use super::RawCommandContext;
    use crate::ActionResult;
    use crate::event::{Combine, Event};

    /// A command typed in chat. Return `Success` to mark it handled; first non-pass wins.
    pub static RAW_COMMAND: Event<RawCommandContext, ActionResult> =
        Event::new(fish_abi::client::ids::RAW_COMMAND, Combine::FirstNonPass);
}

/// Key events.
pub mod input {
    use super::KeyContext;
    use crate::event::{Combine, Event};

    /// A key or mouse button went down or up.
    pub static KEY: Event<KeyContext> = Event::new(fish_abi::client::ids::KEY, Combine::Notify);
}
