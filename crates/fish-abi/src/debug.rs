//! The F3 debug screen event (phase P6).

use core::ffi::c_void;

use crate::types::FishStr;

/// Event id: append lines to the F3 screen (`Notify`).
pub const F3_LINES: &str = "fish:debug/f3_lines";

/// Appends one line to the F3 screen.
pub type AddLineFn = unsafe extern "C" fn(user: *mut c_void, line: FishStr);

/// Payload of [`F3_LINES`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DebugPayload {
    /// `size_of::<DebugPayload>()`.
    pub size: u32,
    /// Appends a line; valid during the callback.
    pub add: AddLineFn,
    /// Passed to `add`.
    pub user: *mut c_void,
}
