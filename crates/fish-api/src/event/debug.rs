//! The F3 debug screen.

use fish_abi::debug::DebugPayload;

use super::{Combine, Event, Payload};

/// The context of [`F3_LINES`]: add lines to the right-hand side of the debug screen.
#[repr(transparent)]
pub struct DebugContext(DebugPayload);

// SAFETY: `repr(transparent)` over an ABI payload that starts with `size: u32`.
unsafe impl Payload for DebugContext {}

impl DebugContext {
    /// Appends a line (at most 32 lines of 200 characters are kept per frame).
    pub fn add_line(&self, text: &str) {
        // SAFETY: `add` and `user` come together from the host for this call.
        unsafe { (self.0.add)(self.0.user, fish_abi::FishStr::new(text)) };
    }
}

/// Fired each frame the F3 screen is drawn; add lines with [`DebugContext::add_line`].
pub static F3_LINES: Event<DebugContext> = Event::new(fish_abi::debug::F3_LINES, Combine::Notify);
