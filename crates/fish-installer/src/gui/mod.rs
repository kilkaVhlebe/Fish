//! The installer window. All decisions live in [`state`] (unit tested); [`view`] only draws.

pub mod state;
mod view;

pub use view::run_window;
