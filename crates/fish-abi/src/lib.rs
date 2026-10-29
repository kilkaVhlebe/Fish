//! The stable C ABI between the Fish loader (host) and Fish mods.
//!
//! Everything here is `#[repr(C)]` and `no_std` with zero dependencies, so a mod written in any
//! language can implement it. The rules:
//!
//! * **Versioning.** While [`FISH_ABI_MAJOR`] is 0 the host and mod *minor* versions must be equal;
//!   from 1.0 the host minor must be at least the mod minor and majors must match
//!   ([`is_compatible`]).
//! * **Tables grow by appending.** Every table starts with its `size`; callers check `size`
//!   before using a field added after the first version.
//! * **Memory** is freed only by the module that allocated it.
//! * **Panics** never cross the boundary; every `extern "C"` function catches them and returns
//!   [`FishStatus::PANICKED`].
//! * **Handles** ([`FishHandle`]) are valid only during the callback that received them.

#![no_std]
#![deny(missing_docs)]

pub mod client;
pub mod commands;
pub mod debug;
pub mod game;
pub mod ident;
pub mod networking;
pub mod services;
mod tables;
mod types;

pub use tables::*;
pub use types::*;

/// ABI major version.
pub const FISH_ABI_MAJOR: u16 = 0;
/// ABI minor version.
pub const FISH_ABI_MINOR: u16 = 1;

/// Name of the symbol every mod library exports (`extern "C" fn() -> *const FishModV1`).
pub const MOD_SYMBOL: &str = "fish_mod_v1";
/// [`MOD_SYMBOL`] as a NUL-terminated byte string for `dlsym`-style lookups.
pub const MOD_SYMBOL_C: &[u8] = b"fish_mod_v1\0";

/// True if a mod built for ABI `mod_*` can run on a host serving ABI `host_*`.
///
/// Major 0 is unstable: minors must be equal. From major 1 the host minor must be at least the
/// mod minor. Different majors are never compatible.
pub const fn is_compatible(
    host_major: u16,
    host_minor: u16,
    mod_major: u16,
    mod_minor: u16,
) -> bool {
    if host_major != mod_major {
        return false;
    }
    if host_major == 0 {
        host_minor == mod_minor
    } else {
        host_minor >= mod_minor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_rules() {
        // major 0: exact minor
        assert!(is_compatible(0, 1, 0, 1));
        assert!(!is_compatible(0, 2, 0, 1));
        assert!(!is_compatible(0, 1, 0, 2));
        // major 1+: host minor >= mod minor
        assert!(is_compatible(1, 3, 1, 3));
        assert!(is_compatible(1, 3, 1, 0));
        assert!(!is_compatible(1, 0, 1, 3));
        // majors never mix
        assert!(!is_compatible(1, 0, 0, 0));
        assert!(!is_compatible(0, 0, 1, 0));
    }

    #[test]
    fn symbol_constants_agree() {
        assert_eq!(
            MOD_SYMBOL.as_bytes(),
            &MOD_SYMBOL_C[..MOD_SYMBOL_C.len() - 1]
        );
        assert_eq!(MOD_SYMBOL_C[MOD_SYMBOL_C.len() - 1], 0);
    }
}
