//! Tags: named sets of ids (Fabric `fabric-tag-api-v1`, `fabric-convention-tags-v2`).
//!
//! A [`TagKey`] names a tag in a registry (`item`, `block`, `entity_type`, `fluid`, or any Fish registry).
//! Membership is decided by data (`data/<ns>/tags/<registry>/<path>.json`), not by code; this module
//! gives mods checked names for the convention tags other mods agree on.

use std::fmt;

use crate::identifier::{Identifier, IdentifierError};

/// The name of a tag in one registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TagKey {
    registry: &'static str,
    id: &'static str,
}

impl TagKey {
    /// A key for tag `id` (`namespace:path`) of `registry`.
    pub const fn new(registry: &'static str, id: &'static str) -> TagKey {
        TagKey { registry, id }
    }

    /// The registry the tag belongs to.
    pub const fn registry(&self) -> &'static str {
        self.registry
    }

    /// The tag id as text.
    pub const fn id_str(&self) -> &'static str {
        self.id
    }

    /// The tag id.
    pub fn id(&self) -> Result<Identifier, IdentifierError> {
        self.id.parse()
    }

    /// Where the tag's JSON lives inside a pack: `data/<ns>/tags/<registry>/<path>.json`.
    pub fn data_path(&self) -> String {
        let (ns, path) = self.id.split_once(':').unwrap_or(("minecraft", self.id));
        format!("data/{ns}/tags/{}/{path}.json", self.registry)
    }
}

impl fmt::Display for TagKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{} ({})", self.id, self.registry)
    }
}

/// Fabric convention tags (namespace `c`), generated from `c_tags.txt`.
pub mod c {
    use super::TagKey;

    include!(concat!(env!("OUT_DIR"), "/c_tags.rs"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_name_their_registry_and_data_path() {
        assert_eq!(c::ITEM_INGOTS_IRON.registry(), "item");
        assert_eq!(c::ITEM_INGOTS_IRON.id_str(), "c:ingots/iron");
        assert_eq!(
            c::ITEM_INGOTS_IRON.data_path(),
            "data/c/tags/item/ingots/iron.json"
        );
        assert_eq!(c::BLOCK_ORES_COAL.to_string(), "#c:ores/coal (block)");
        assert_eq!(
            TagKey::new("fish:gems", "mymod:rare").data_path(),
            "data/mymod/tags/fish:gems/rare.json"
        );
        assert_eq!(c::FLUID_WATER.id().unwrap().to_string(), "c:water");
    }

    #[test]
    fn every_convention_tag_is_a_valid_unique_identifier() {
        let mut seen = std::collections::HashSet::new();
        assert!(c::ALL.len() > 50);
        for t in c::ALL {
            assert!(t.id().is_ok(), "{t}");
            assert!(seen.insert((t.registry(), t.id_str())), "{t} twice");
            assert!(t.id_str().starts_with("c:"));
        }
    }
}
