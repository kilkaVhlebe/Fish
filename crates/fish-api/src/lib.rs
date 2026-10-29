//! Safe Rust API for writing Fish mods.
//!
//! A mod is a `cdylib` that calls [`export_mod!`] once. The loader then calls the declared
//! entrypoints with a host table; this crate wraps that table:
//!
//! * [`event`]: typed events with phases (Fabric `Event<T>` parity),
//! * [`loader`]: mods, directories, environment, object share, entrypoint objects,
//! * [`Identifier`], [`ActionResult`], [`TypedActionResult`],
//! * a `log` bridge installed automatically before the first entrypoint runs.
//!
//! Build mods with `panic = "unwind"` (the default): a panic must be catchable at the boundary.

#![deny(missing_docs)]

pub mod client;
pub mod command;
pub mod event;
pub mod game;
pub mod loader;
pub mod networking;

mod error;
mod host;
mod identifier;
mod macros;

pub use error::Error;
#[doc(hidden)]
pub use fish_abi as abi;
pub use identifier::{ActionResult, Identifier, IdentifierError, TypedActionResult};
pub use loader::FishLoader;
pub use macros::IntoInitResult;
#[doc(hidden)]
pub use macros::{__object, __run_init};

/// ABI version this crate was built for.
pub const ABI_VERSION: (u16, u16) = (abi::FISH_ABI_MAJOR, abi::FISH_ABI_MINOR);

/// The attached host table, for diagnostic mods that exercise the raw ABI (null if none).
#[doc(hidden)]
pub fn __host_table() -> *const abi::FishHostV1 {
    host::get().map_or(std::ptr::null(), std::ptr::from_ref)
}

/// Attaches `host` for tests that drive the API without a loader.
#[doc(hidden)]
pub fn __attach_for_tests(host: *const abi::FishHostV1) {
    host::attach(host);
    host::install_logger();
}

#[cfg(test)]
mod fake;
#[cfg(test)]
mod fake_client;
