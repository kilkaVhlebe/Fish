//! Fish's installer: gets an edition onto the machine and plugs it into the Horizon launcher.
//!
//! Everything goes through the launcher's public command line (`horizon-launcher instance …`);
//! Fish only sets the instance's *wrapper* to `fish-launch` and never edits launcher files.

#![deny(missing_docs)]

pub mod cli;
pub mod error;
pub mod flow;
#[cfg(feature = "gui")]
pub mod gui;
pub mod launcher;
pub mod source;

pub use error::{Error, Result};
