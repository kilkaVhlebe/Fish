//! Networking (phase P7): custom payloads between client and server.
//!
//! A payload is a channel id (`namespace:path`) plus bytes. Types are registered per direction
//! with a maximum size; receivers are registered per direction and id. Limits: clientbound
//! payloads are at most [`MAX_S2C`] bytes, serverbound at most [`MAX_C2S`].

use core::ffi::c_void;

use crate::tables::{DropUserFn, FishServiceHeader};
use crate::types::{FishHandle, FishStatus, FishStr};

/// Service id.
pub const SERVICE_NETWORKING: &str = "fish:networking";

/// Direction/phase: server to client, play phase.
pub const S2C_PLAY: u8 = 0;
/// Direction/phase: client to server, play phase.
pub const C2S_PLAY: u8 = 1;
/// Direction/phase: server to client, configuration phase.
pub const S2C_CONFIG: u8 = 2;
/// Direction/phase: client to server, configuration phase.
pub const C2S_CONFIG: u8 = 3;

/// Largest clientbound payload.
pub const MAX_S2C: usize = 1 << 20;
/// Largest serverbound payload.
pub const MAX_C2S: usize = 32_767;

/// The internal channel Fish uses for its handshake (configuration phase, both directions).
pub const HANDSHAKE_CHANNEL: &str = "fish:handshake";
/// The internal channel Fish uses to synchronise registries (configuration phase, server to client).
pub const REGISTRY_SYNC_CHANNEL: &str = "fish:registry_sync";

/// A payload that arrived, as a receiver sees it. Strings and bytes are borrowed for the call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NetPayloadV1 {
    /// `size_of::<NetPayloadV1>()`.
    pub size: u32,
    /// One of the direction constants.
    pub dir: u8,
    /// Padding, zero.
    pub reserved: [u8; 3],
    /// The channel.
    pub channel: FishStr,
    /// The bytes.
    pub data: *const u8,
    /// Length of `data`.
    pub len: usize,
    /// On a server: the sending player's name; empty on a client.
    pub sender: FishStr,
    /// On a server: the sending player's UUID bytes.
    pub sender_uuid: [u8; 16],
    /// The game that received it, valid during the call.
    pub view: FishHandle,
}

/// Receives a payload.
pub type NetRecvFn =
    unsafe extern "C" fn(user: *mut c_void, payload: *const NetPayloadV1) -> FishStatus;

/// Receives one name.
pub type NameSink = unsafe extern "C" fn(user: *mut c_void, name: FishStr);

/// `fish:networking` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NetworkingServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Registers a payload type for `dir` (idempotent for equal `max_size`, `ALREADY_EXISTS` otherwise).
    pub register_type: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        dir: u8,
        id: FishStr,
        max_size: u32,
    ) -> FishStatus,
    /// Registers the receiver of `id` in `dir` (one per id; `ALREADY_EXISTS` otherwise). The type must be registered.
    pub register_receiver: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        dir: u8,
        id: FishStr,
        func: NetRecvFn,
        user: *mut c_void,
        drop_user: Option<DropUserFn>,
    ) -> FishStatus,
    /// Client to server: sends `data` on `id` (`dir` is `C2S_PLAY` or `C2S_CONFIG`).
    pub send_to_server: unsafe extern "C" fn(
        host: *mut c_void,
        dir: u8,
        id: FishStr,
        data: *const u8,
        len: usize,
    ) -> FishStatus,
    /// Server to client: sends to `player`, or to everyone when `player` is empty.
    pub send_to_player: unsafe extern "C" fn(
        host: *mut c_void,
        dir: u8,
        player: FishStr,
        id: FishStr,
        data: *const u8,
        len: usize,
    ) -> FishStatus,
    /// Nonzero when the remote side announced a receiver for `id` in `dir`. On a server `player`
    /// names the client; on a client it is empty.
    pub can_send:
        unsafe extern "C" fn(host: *mut c_void, dir: u8, player: FishStr, id: FishStr) -> u8,
    /// Lists the players connected in the phase of `dir` (server only).
    pub players:
        unsafe extern "C" fn(host: *mut c_void, dir: u8, sink: NameSink, user: *mut c_void),
}
