//! Fish mod metadata: the `fish.mod.toml` model and parser, version requirements, and the
//! dependency resolver that decides which mods load and in which order.
//!
//! # Resolution
//!
//! [`resolve`] sorts all candidates (so input order never matters), drops those whose
//! `environment` excludes the running [`Side`], keeps only the best copy of every duplicated
//! installed mod, then runs a deterministic backtracking search over mod ids in ascending order,
//! trying the highest version first. Hard constraints: every `depends` is satisfied, no `breaks`
//! matches, at most one candidate owns an id or alias, and every id that has an
//! [`LoadCondition::Always`] candidate is selected. [`LoadCondition::IfPossible`] candidates (nested
//! archives) are dropped when they cannot be satisfied. The search is bounded by [`STEP_LIMIT`].

#![deny(missing_docs)]

mod id;
mod manifest;
mod resolve;
mod version;

pub use id::{IdError, ModId};
pub use manifest::{
    DepMap, Environment, Library, Manifest, ManifestError, SUPPORTED_SCHEMA, Side, parse,
};
pub use resolve::{
    Candidate, LoadCondition, Origin, Problem, Reason, Resolution, ResolutionError, ResolvedMod,
    STEP_LIMIT, resolve, resolve_with_limit,
};
pub use version::{Builtins, VersionReqSet};
