//! The Fish host runtime.
//!
//! [`boot`] discovers mods (`<game dir>/fish-mods` plus development mods), resolves their
//! dependencies with `fish-metadata`, extracts and loads their native libraries, validates the
//! ABI, and runs the entrypoints (`pre_launch`, then `main`, then `client` or `server`). It never
//! panics the game for a bad mod: failures are logged, reported in the [`BootReport`], and the mod
//! (plus its dependents) stops receiving events.
//!
//! Every [`Loader`] is independent: libraries are extracted into the loader's own
//! `<game dir>/.fish/cache`, so distinct file paths give each loader its own module instances.

#![deny(missing_docs)]

mod block_services;
mod blocks;
mod boot;
mod bus;
mod client_services;
mod command_services;
mod commands;
mod discovery;
mod dispatch;
pub mod edition;
mod gamerule_services;
mod gamerules;
mod handles;
mod host;
mod inner;
mod item_services;
mod items;
mod load;
mod net;
mod net_services;
mod registry_services;
mod report;
pub mod selftest;

pub use boot::{BootConfig, BootError, Loader, ModSummary, boot};
pub use bus::{DEFAULT_PHASE, EventBus, EventIndex};
pub use dispatch::{LoaderDispatch, install_global};
pub use fish_metadata::Side;
pub use inner::{LogRecord, LogSink, ModState};
pub use report::{AbiReport, BootReport, ModReport};
