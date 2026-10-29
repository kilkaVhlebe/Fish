//! Command registration (phase P6): the `fish:commands` service and the execution context.
//!
//! A mod builds a Brigadier-style tree in its own memory, serialises it to JSON (see the tree
//! format below) and registers it once with a single executor callback. The host parses command
//! lines, checks permissions, converts arguments and calls the executor with the index of the
//! node that executes.
//!
//! Tree node: `{"name": "x", "type": "literal", "level": 2, "exec": 0, "redirect": "root",
//! "suggests": ["a"], "children": [...]}`. `type` is `literal` or one of `word`, `string`, `greedy`,
//! `integer` (`min`/`max`), `double` (`min`/`max`), `bool`, `player`, `entity`, `block_pos`. `level`,
//! `exec`, `redirect` and `suggests` are optional.

use core::ffi::c_void;

use crate::tables::{DropUserFn, FishServiceHeader};
use crate::types::{FishStatus, FishStr};

/// Service id.
pub const SERVICE_COMMANDS: &str = "fish:commands";

/// Event ids.
pub mod ids {
    /// Fired once after every mod's entrypoints ran: register server (shared) commands now.
    pub const REGISTER: &str = "fish:command/register";
    /// Fired once after every mod's entrypoints ran: register client commands now.
    pub const REGISTER_CLIENT: &str = "fish:command/register_client";
}

/// Tree kind: commands the server runs (also in single player).
pub const KIND_SERVER: u8 = 0;
/// Tree kind: commands the client runs itself.
pub const KIND_CLIENT: u8 = 1;

/// [`CommandArgV1::kind`] values.
pub mod arg {
    /// A single word.
    pub const WORD: u8 = 0;
    /// A word or a quoted string.
    pub const STRING: u8 = 1;
    /// The rest of the line.
    pub const GREEDY: u8 = 2;
    /// A 32-bit integer.
    pub const INTEGER: u8 = 3;
    /// A 64-bit float.
    pub const DOUBLE: u8 = 4;
    /// `true` or `false`.
    pub const BOOL: u8 = 5;
    /// A player name.
    pub const PLAYER: u8 = 6;
    /// An entity selector or name, unresolved.
    pub const ENTITY: u8 = 7;
    /// Three block coordinates, delivered resolved as `x y z`.
    pub const BLOCK_POS: u8 = 8;
}

/// Payload of the two register events.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CommandRegisterPayload {
    /// `size_of::<CommandRegisterPayload>()`.
    pub size: u32,
    /// `KIND_SERVER` or `KIND_CLIENT`.
    pub kind: u8,
    /// `SIDE_CLIENT` or `SIDE_SERVER`: the side Fish runs on.
    pub side: u8,
    /// Padding, zero.
    pub reserved: [u8; 2],
}

/// One parsed argument, as the executor sees it. Strings are borrowed for the call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CommandArgV1 {
    /// The argument's name in the tree.
    pub name: FishStr,
    /// An `arg::*` value.
    pub kind: u8,
    /// The value as text (quotes removed; `block_pos` resolved).
    pub value: FishStr,
}

/// Reports a line of output: `error` nonzero for a failure message.
pub type CommandOutputFn = unsafe extern "C" fn(out: *mut c_void, text: FishStr, error: u8);

/// What the executor is called with.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CommandCtxV1 {
    /// `size_of::<CommandCtxV1>()`.
    pub size: u32,
    /// Nonzero for a player source.
    pub is_player: u8,
    /// Permission level of the source.
    pub level: u8,
    /// Padding, zero.
    pub reserved: [u8; 2],
    /// The source's name.
    pub source_name: FishStr,
    /// The source's UUID bytes.
    pub source_uuid: [u8; 16],
    /// Where the source stands (valid when `has_pos` is nonzero).
    pub pos: [f64; 3],
    /// Nonzero when `pos` is valid.
    pub has_pos: u8,
    /// Padding, zero.
    pub reserved2: [u8; 7],
    /// The parsed arguments, in tree order.
    pub args: *const CommandArgV1,
    /// Number of arguments.
    pub arg_count: usize,
    /// Passed to `output`.
    pub out: *mut c_void,
    /// Emits feedback or an error line.
    pub output: CommandOutputFn,
    /// Where the executor stores its result value.
    pub result: *mut i32,
}

/// The executor of a registered tree: `index` is the node's `exec`.
pub type CommandExecFn =
    unsafe extern "C" fn(user: *mut c_void, index: u32, ctx: *const CommandCtxV1) -> FishStatus;

/// `fish:commands` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CommandsServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Registers one root command tree for the mod `token`. `user` is dropped with `drop_user`.
    pub register: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        kind: u8,
        tree: FishStr,
        exec: CommandExecFn,
        user: *mut c_void,
        drop_user: Option<DropUserFn>,
    ) -> FishStatus,
}
