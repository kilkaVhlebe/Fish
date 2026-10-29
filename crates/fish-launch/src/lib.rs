//! `fish-launch`: the launcher wrapper of the Fish instance.
//!
//! The Horizon launcher runs `fish-launch <path to versions/<tag>/horizon-client> <args…>` for
//! an instance whose wrapper is set to it. This crate picks the Fish Edition built for that
//! Horizon version, verifies it, and starts it with the very same arguments.

#![deny(missing_docs)]

pub mod edition;
pub mod home;
pub mod run;

pub use home::{fish_home, fish_home_from};
pub use run::{LaunchError, Plan, main_with, plan};
