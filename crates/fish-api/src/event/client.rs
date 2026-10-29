//! Client frame events.

use fish_abi::client::ids;

use super::{Combine, Event};
use crate::client::FrameContext;

/// Fired once per rendered frame, in a world or not.
pub static FRAME: Event<FrameContext> = Event::new(ids::FRAME, Combine::Notify);
