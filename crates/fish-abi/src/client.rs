//! Client-side event payloads and services (phase P4): lifecycle ticks, frames, HUD, raw
//! commands and keys.
//!
//! Payloads start with `size: u32` and carry only plain data and [`FishHandle`]s. A handle is
//! valid only during the callback it arrived in; services resolve it and return
//! [`FishStatus::STALE_HANDLE`] afterwards.

use core::ffi::c_void;

use crate::tables::FishServiceHeader;
use crate::types::{FishHandle, FishStatus, FishStr};

/// Event ids defined by the loader for the client hooks.
pub mod ids {
    /// Start of a client-side tick (window `Game`).
    pub const CLIENT_START_TICK: &str = "fish:lifecycle/client_start_tick";
    /// End of a client-side tick.
    pub const CLIENT_END_TICK: &str = "fish:lifecycle/client_end_tick";
    /// Start of a server tick (integrated or dedicated server).
    pub const SERVER_START_TICK: &str = "fish:lifecycle/server_start_tick";
    /// End of a server tick.
    pub const SERVER_END_TICK: &str = "fish:lifecycle/server_end_tick";
    /// Once a rendered frame.
    pub const FRAME: &str = "fish:client/frame";
    /// The HUD pass.
    pub const HUD_RENDER: &str = "fish:hud/render";
    /// A raw client command (first non-pass).
    pub const RAW_COMMAND: &str = "fish:command_raw/command";
    /// A key or mouse button event.
    pub const KEY: &str = "fish:input/key";
}

/// Service ids.
pub mod services {
    /// `fish:client`.
    pub const CLIENT: &str = "fish:client";
    /// `fish:hud`.
    pub const HUD: &str = "fish:hud";
    /// `fish:input`.
    pub const INPUT: &str = "fish:input";
    /// `fish:command_raw`.
    pub const COMMAND_RAW: &str = "fish:command_raw";
}

/// [`TickPayload::role`]: the window's game.
pub const ROLE_WINDOW: u8 = 0;
/// [`TickPayload::role`]: an integrated server.
pub const ROLE_INTEGRATED_SERVER: u8 = 1;
/// [`TickPayload::role`]: a dedicated server.
pub const ROLE_DEDICATED_SERVER: u8 = 2;

/// Payload of the four tick events.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TickPayload {
    /// `size_of::<TickPayload>()`.
    pub size: u32,
    /// One of the `ROLE_*` values.
    pub role: u8,
    /// Padding, zero.
    pub reserved: [u8; 3],
    /// Ticks seen so far by this role (starts at 1 for the first tick).
    pub tick: u64,
    /// The game, valid during the callback.
    pub view: FishHandle,
}

/// Payload of the frame event.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FramePayload {
    /// `size_of::<FramePayload>()`.
    pub size: u32,
    /// Seconds since the previous frame.
    pub elapsed_secs: f32,
    /// The game, valid during the callback.
    pub view: FishHandle,
}

/// Payload of the HUD event.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct HudPayload {
    /// `size_of::<HudPayload>()`.
    pub size: u32,
    /// The game, valid during the callback.
    pub view: FishHandle,
    /// The HUD draw target, valid during the callback.
    pub hud: FishHandle,
}

/// Payload of the raw command event; the result is an action byte (`Success` = handled).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CommandPayload {
    /// `size_of::<CommandPayload>()`.
    pub size: u32,
    /// The game, valid during the callback.
    pub view: FishHandle,
    /// The command without a leading slash; borrowed for the callback.
    pub command: FishStr,
}

/// Payload of the key event.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KeyPayload {
    /// `size_of::<KeyPayload>()`.
    pub size: u32,
    /// Nonzero when pressed.
    pub pressed: u8,
    /// Padding, zero.
    pub reserved: [u8; 3],
    /// The game, valid during the callback.
    pub view: FishHandle,
    /// Vanilla name, e.g. `key.keyboard.z` or `key.mouse.left`; borrowed for the callback.
    pub key: FishStr,
}

/// The local player, as `fish:client` returns it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerV1 {
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
}

/// Receives the dimension id string of a player query.
pub type StrSink = unsafe extern "C" fn(user: *mut c_void, text: FishStr);

/// `fish:client` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ClientServiceV1 {
    /// Table header (`version == 1`).
    pub header: FishServiceHeader,
    /// The `ROLE_*` of `view`, or `u8::MAX` for a stale handle.
    pub role: unsafe extern "C" fn(host: *mut c_void, view: FishHandle) -> u8,
    /// Fills `out` and calls `dimension` with the dimension id; `NOT_FOUND` outside a world.
    pub player: unsafe extern "C" fn(
        host: *mut c_void,
        view: FishHandle,
        out: *mut PlayerV1,
        dimension: StrSink,
        user: *mut c_void,
    ) -> FishStatus,
    /// Shows a chat line (client only).
    pub show_chat:
        unsafe extern "C" fn(host: *mut c_void, view: FishHandle, text: FishStr) -> FishStatus,
    /// The game directory.
    pub game_dir: unsafe extern "C" fn(
        host: *mut c_void,
        view: FishHandle,
        sink: StrSink,
        user: *mut c_void,
    ) -> FishStatus,
}

/// `fish:hud` service, version 1. Coordinates are GUI pixels.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HudServiceV1 {
    /// Table header (`version == 1`).
    pub header: FishServiceHeader,
    /// Fills a rectangle with `argb`.
    pub fill: unsafe extern "C" fn(
        host: *mut c_void,
        hud: FishHandle,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        argb: u32,
    ) -> FishStatus,
    /// Draws text in opaque `rgb`.
    pub text: unsafe extern "C" fn(
        host: *mut c_void,
        hud: FishHandle,
        text: FishStr,
        x: f32,
        y: f32,
        rgb: u32,
        shadow: u8,
    ) -> FishStatus,
    /// Writes the width of `text` to `out`.
    pub text_width: unsafe extern "C" fn(
        host: *mut c_void,
        hud: FishHandle,
        text: FishStr,
        out: *mut f32,
    ) -> FishStatus,
    /// Writes `[width, height]` to `out`.
    pub gui_size:
        unsafe extern "C" fn(host: *mut c_void, hud: FishHandle, out: *mut [f32; 2]) -> FishStatus,
}

/// `fish:input` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InputServiceV1 {
    /// Table header (`version == 1`).
    pub header: FishServiceHeader,
    /// Nonzero while the named key or mouse button is held (as last reported to the hooks).
    pub is_down: unsafe extern "C" fn(host: *mut c_void, key: FishStr) -> u8,
}

/// `fish:command_raw` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CommandRawServiceV1 {
    /// Table header (`version == 1`).
    pub header: FishServiceHeader,
    /// Shows command feedback to the player (a chat line on the client).
    pub feedback:
        unsafe extern "C" fn(host: *mut c_void, view: FishHandle, text: FishStr) -> FishStatus,
}

// SAFETY: service tables are immutable function-pointer tables.
unsafe impl Sync for ClientServiceV1 {}
// SAFETY: see above.
unsafe impl Sync for HudServiceV1 {}
// SAFETY: see above.
unsafe impl Sync for InputServiceV1 {}
// SAFETY: see above.
unsafe impl Sync for CommandRawServiceV1 {}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn payload_layouts_are_pinned() {
        assert_eq!(size_of::<TickPayload>(), 24);
        assert_eq!(offset_of!(TickPayload, role), 4);
        assert_eq!(offset_of!(TickPayload, tick), 8);
        assert_eq!(offset_of!(TickPayload, view), 16);
        assert_eq!(size_of::<FramePayload>(), 16);
        assert_eq!(size_of::<HudPayload>(), 20);
        assert_eq!(size_of::<CommandPayload>(), 32);
        assert_eq!(offset_of!(CommandPayload, command), 16);
        assert_eq!(size_of::<KeyPayload>(), 32);
        assert_eq!(offset_of!(KeyPayload, key), 16);
        assert_eq!(size_of::<PlayerV1>(), 32);
    }

    #[test]
    fn every_payload_starts_with_its_size() {
        assert_eq!(offset_of!(TickPayload, size), 0);
        assert_eq!(offset_of!(FramePayload, size), 0);
        assert_eq!(offset_of!(HudPayload, size), 0);
        assert_eq!(offset_of!(CommandPayload, size), 0);
        assert_eq!(offset_of!(KeyPayload, size), 0);
    }

    #[test]
    fn event_ids_are_valid_identifiers() {
        for id in [
            ids::CLIENT_START_TICK,
            ids::CLIENT_END_TICK,
            ids::SERVER_START_TICK,
            ids::SERVER_END_TICK,
            ids::FRAME,
            ids::HUD_RENDER,
            ids::RAW_COMMAND,
            ids::KEY,
        ] {
            assert!(crate::ident::is_valid_identifier(id), "{id}");
        }
        for id in [
            services::CLIENT,
            services::HUD,
            services::INPUT,
            services::COMMAND_RAW,
        ] {
            assert!(crate::ident::is_valid_identifier(id), "{id}");
        }
    }
}
