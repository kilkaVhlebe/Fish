//! Version requirement sets and loader-provided builtin candidates.

use std::fmt;

use semver::{Version, VersionReq};
use serde::{Deserialize, Deserializer};

use crate::manifest::Manifest;

/// One requirement, or several alternatives (`["a", "b"]` in TOML means *a or b*).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReqSet(Vec<VersionReq>);

impl VersionReqSet {
    /// Parses one or more requirement strings (semver `VersionReq` syntax).
    pub fn parse<S: AsRef<str>>(reqs: &[S]) -> Result<Self, String> {
        if reqs.is_empty() {
            return Err("a requirement list must not be empty".into());
        }
        reqs.iter()
            .map(|r| {
                VersionReq::parse(r.as_ref().trim())
                    .map_err(|e| format!("invalid version requirement {:?}: {e}", r.as_ref()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(VersionReqSet)
    }

    /// True if any alternative matches `v`. Pre-release versions follow semver rules.
    pub fn matches(&self, v: &Version) -> bool {
        self.0.iter().any(|r| r.matches(v))
    }

    /// Like [`matches`](Self::matches), but a pre-release version is also tested as its release
    /// (`0.2.0-nightly.1` satisfies `>=0.2.0`). Used for loader-provided builtins.
    pub fn matches_lenient(&self, v: &Version) -> bool {
        if self.matches(v) {
            return true;
        }
        if v.pre.is_empty() {
            return false;
        }
        let release = Version::new(v.major, v.minor, v.patch);
        self.matches(&release)
    }
}

impl fmt::Display for VersionReqSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, r) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" or ")?;
            }
            write!(f, "{r}")?;
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for VersionReqSet {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OneOrMany {
            One(String),
            Many(Vec<String>),
        }
        let list = match OneOrMany::deserialize(d)? {
            OneOrMany::One(s) => vec![s],
            OneOrMany::Many(v) => v,
        };
        VersionReqSet::parse(&list).map_err(serde::de::Error::custom)
    }
}

/// The candidates the loader itself supplies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Builtins {
    /// Fish version (also used for `fish_api`).
    pub fish: Version,
    /// Minecraft version Horizon implements.
    pub minecraft: Version,
    /// Horizon build version, e.g. `0.2.0-nightly.20261028+6f8c7a0`.
    pub horizon: Version,
}

impl Builtins {
    /// Minecraft version provided by every Horizon this Fish supports.
    pub const MINECRAFT: &'static str = "26.3.0";

    /// Builtins for a Fish and Horizon version.
    pub fn new(fish: Version, horizon: Version) -> Self {
        Builtins {
            fish,
            minecraft: Version::parse(Self::MINECRAFT).expect("constant"),
            horizon,
        }
    }

    /// One synthetic manifest per builtin id.
    pub fn manifests(&self) -> [Manifest; 4] {
        [
            Manifest::builtin("fish", self.fish.clone()),
            Manifest::builtin("fish_api", self.fish.clone()),
            Manifest::builtin("minecraft", self.minecraft.clone()),
            Manifest::builtin("horizon", self.horizon.clone()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn single_and_or() {
        let one = VersionReqSet::parse(&[">=1.2"]).unwrap();
        assert!(one.matches(&v("1.2.0")) && !one.matches(&v("1.1.9")));
        let or = VersionReqSet::parse(&["<1", ">=3"]).unwrap();
        assert!(or.matches(&v("0.5.0")) && or.matches(&v("3.0.0")) && !or.matches(&v("2.0.0")));
        assert_eq!(or.to_string(), "<1 or >=3");
    }

    #[test]
    fn errors() {
        assert!(
            VersionReqSet::parse(&["nonsense"])
                .unwrap_err()
                .contains("invalid version requirement")
        );
        assert!(VersionReqSet::parse::<&str>(&[]).is_err());
    }

    #[test]
    fn lenient_matches_prerelease_as_release() {
        let req = VersionReqSet::parse(&[">=0.2.0"]).unwrap();
        let pre = v("0.2.0-nightly.20261028+6f8c7a0");
        assert!(!req.matches(&pre));
        assert!(req.matches_lenient(&pre));
        assert!(!req.matches_lenient(&v("0.1.0-nightly.1")));
        assert!(!req.matches_lenient(&v("0.1.9")));
    }

    #[test]
    fn deserializes_string_or_array() {
        #[derive(Deserialize)]
        struct T {
            a: VersionReqSet,
            b: VersionReqSet,
        }
        let t: T = toml::from_str("a = '>=1'\nb = ['<1', '>2']").unwrap();
        assert_eq!(t.a.to_string(), ">=1");
        assert_eq!(t.b.to_string(), "<1 or >2");
        assert!(toml::from_str::<T>("a = 1\nb = []").is_err());
    }

    #[test]
    fn builtins_cover_four_ids() {
        let b = Builtins::new(v("0.1.0"), v("0.2.0-nightly.1"));
        let ids: Vec<_> = b.manifests().iter().map(|m| m.id.to_string()).collect();
        assert_eq!(ids, ["fish", "fish_api", "minecraft", "horizon"]);
        assert_eq!(b.minecraft, v("26.3.0"));
    }
}
