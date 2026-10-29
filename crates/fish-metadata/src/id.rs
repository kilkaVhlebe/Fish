//! Mod identifiers.

use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Deserializer};

/// Ids that only the loader may provide.
const RESERVED: [&str; 4] = ["fish", "fish_api", "horizon", "minecraft"];

/// Why a string is not a valid mod id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    /// The string does not match `^[a-z][a-z0-9_]{1,63}$`.
    Invalid(String),
    /// The id is valid but reserved by Fish.
    Reserved(String),
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdError::Invalid(s) => {
                write!(
                    f,
                    "invalid mod id {s:?}: must match ^[a-z][a-z0-9_]{{1,63}}$"
                )
            }
            IdError::Reserved(s) => write!(f, "id {s} is reserved by Fish"),
        }
    }
}

impl std::error::Error for IdError {}

/// A validated mod id (or a reference to one, for dependency tables).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModId(String);

fn is_well_formed(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=64).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

impl ModId {
    /// Parses an id for a mod being defined; reserved ids are refused.
    pub fn parse(s: &str) -> Result<ModId, IdError> {
        let id = Self::parse_reference(s)?;
        if RESERVED.contains(&s) {
            return Err(IdError::Reserved(s.to_string()));
        }
        Ok(id)
    }

    /// Parses an id that only *refers* to a mod, so reserved ids (`fish`, `minecraft`, …) are fine.
    pub fn parse_reference(s: &str) -> Result<ModId, IdError> {
        if is_well_formed(s) {
            Ok(ModId(s.to_string()))
        } else {
            Err(IdError::Invalid(s.into()))
        }
    }

    /// Id of a loader-provided builtin.
    pub(crate) fn builtin(s: &'static str) -> ModId {
        ModId(s.to_string())
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for ModId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ModId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ModId::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Deserializes a reference id (reserved ids allowed).
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RefId(pub ModId);

impl<'de> Deserialize<'de> for RefId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ModId::parse_reference(&s)
            .map(RefId)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_and_rejects() {
        for ok in ["ab", "example_mod", "a1", &"a".repeat(64)] {
            assert!(ModId::parse(ok).is_ok(), "{ok}");
        }
        for bad in ["", "a", "A1", "1a", "a-b", "a b", "é1", &"a".repeat(65)] {
            assert!(
                matches!(ModId::parse(bad), Err(IdError::Invalid(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn reserved_ids_only_as_references() {
        for r in RESERVED {
            assert_eq!(ModId::parse(r), Err(IdError::Reserved(r.into())));
            assert_eq!(
                ModId::parse(r).unwrap_err().to_string(),
                format!("id {r} is reserved by Fish")
            );
            assert_eq!(ModId::parse_reference(r).unwrap().as_str(), r);
        }
        assert!(ModId::parse_reference("Bad").is_err());
    }

    #[test]
    fn display_and_borrow() {
        let id = ModId::parse("abc").unwrap();
        assert_eq!(id.to_string(), "abc");
        let b: &str = id.borrow();
        assert_eq!(b, "abc");
        assert!(
            IdError::Invalid("X".into())
                .to_string()
                .contains("invalid mod id")
        );
    }
}
