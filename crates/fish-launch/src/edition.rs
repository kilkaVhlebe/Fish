//! Installed editions and choosing one for a Horizon version.

use std::path::{Path, PathBuf};

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The Horizon build an edition is made from.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HorizonInfo {
    /// Full commit hash.
    pub commit: String,
    /// The launcher-installable release tag (may be empty).
    #[serde(default)]
    pub release_tag: String,
}

/// The edition's binary.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct BinaryInfo {
    /// File name inside the edition folder.
    pub name: String,
    /// Hex SHA-256.
    pub sha256: String,
}

/// `fish-edition.json`, the parts the wrapper needs.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EditionManifest {
    /// Edition id, e.g. `0.1.0+horizon.6f8c7a0`.
    pub id: String,
    /// Fish version.
    pub fish_version: String,
    /// Horizon build.
    pub horizon: HorizonInfo,
    /// Platform key.
    pub platform: String,
    /// True for dedicated-server editions.
    #[serde(default)]
    pub server: bool,
    /// The binary.
    pub binary: BinaryInfo,
}

/// An installed edition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edition {
    /// Its folder under `editions/`.
    pub dir: PathBuf,
    /// Its manifest.
    pub manifest: EditionManifest,
}

impl Edition {
    /// Path of the edition's binary.
    pub fn binary_path(&self) -> PathBuf {
        self.dir.join(&self.manifest.binary.name)
    }
}

/// Installed editions under `<home>/editions`; folders that do not parse are skipped.
pub fn discover(home: &Path) -> Vec<Edition> {
    let Ok(read) = std::fs::read_dir(home.join("editions")) else {
        return Vec::new();
    };
    let mut found: Vec<Edition> = read
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .filter_map(|dir| {
            let text = std::fs::read_to_string(dir.join("fish-edition.json")).ok()?;
            let manifest = serde_json::from_str(&text).ok()?;
            Some(Edition { dir, manifest })
        })
        .collect();
    found.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    found
}

/// The launcher version tag of a game program path: the folder below `versions/`.
///
/// Works for `versions/<tag>/horizon-client` and for the macOS bundle layout
/// `versions/<tag>/Horizon.app/Contents/MacOS/horizon-client`.
pub fn version_tag(program: &Path) -> Option<String> {
    let parts: Vec<_> = program
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let i = parts.iter().rposition(|p| p == "versions")?;
    let tag = parts.get(i + 1)?;
    // the tag folder must contain something below it (the program itself)
    (i + 2 < parts.len()).then(|| tag.clone())
}

/// How a launcher version tag names a Horizon build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagKind {
    /// `nightly-<date>-<short commit>`.
    Nightly(String),
    /// `native-<short commit>-<key>`.
    Native(String),
    /// `vX.Y.Z`.
    Release(String),
}

/// Parses a launcher version tag.
pub fn parse_tag(tag: &str) -> Option<TagKind> {
    let is_short = |s: &str| s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit());
    if let Some(rest) = tag.strip_prefix("nightly-") {
        let short = rest.rsplit('-').next()?;
        return (rest.split('-').count() >= 4 && is_short(short))
            .then(|| TagKind::Nightly(short.to_string()));
    }
    if let Some(rest) = tag.strip_prefix("native-") {
        let short = rest.split('-').next()?;
        return is_short(short).then(|| TagKind::Native(short.to_string()));
    }
    (tag.starts_with('v') && tag.len() > 1).then(|| TagKind::Release(tag.to_string()))
}

/// The edition for `tag` among `editions` (client editions of `platform`); the newest Fish wins.
pub fn select<'a>(editions: &'a [Edition], tag: &str, platform: &str) -> Option<&'a Edition> {
    let kind = parse_tag(tag)?;
    let matches = |e: &Edition| match &kind {
        TagKind::Nightly(short) | TagKind::Native(short) => {
            e.manifest.horizon.commit.starts_with(short.as_str())
                || e.manifest.horizon.release_tag == tag
        }
        TagKind::Release(t) => e.manifest.horizon.release_tag == *t,
    };
    editions
        .iter()
        .filter(|e| !e.manifest.server && e.manifest.platform == platform && matches(e))
        .max_by_key(|e| {
            Version::parse(&e.manifest.fish_version).unwrap_or_else(|_| Version::new(0, 0, 0))
        })
}

/// Why an edition cannot be launched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// The binary file is missing or unreadable.
    Missing,
    /// The binary's SHA-256 differs from the manifest.
    ChecksumMismatch,
}

/// Checks the edition's binary against its recorded SHA-256.
pub fn verify(e: &Edition) -> Result<(), VerifyError> {
    let bytes = std::fs::read(e.binary_path()).map_err(|_| VerifyError::Missing)?;
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest.eq_ignore_ascii_case(&e.manifest.binary.sha256) {
        Ok(())
    } else {
        Err(VerifyError::ChecksumMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edition(
        commit: &str,
        release_tag: &str,
        fish: &str,
        server: bool,
        platform: &str,
    ) -> Edition {
        Edition {
            dir: PathBuf::from("/e"),
            manifest: EditionManifest {
                id: format!("{fish}+horizon.{}", &commit[..7]),
                fish_version: fish.into(),
                horizon: HorizonInfo {
                    commit: commit.into(),
                    release_tag: release_tag.into(),
                },
                platform: platform.into(),
                server,
                binary: BinaryInfo {
                    name: "horizon-client".into(),
                    sha256: "00".into(),
                },
            },
        }
    }

    #[test]
    fn version_tags_come_from_the_folder_below_versions() {
        let t = |p: &str| version_tag(Path::new(p));
        assert_eq!(t("C:/Users/A/AppData/Roaming/horizon-launcher/versions/nightly-2026-10-28-6f8c7a0/horizon-client.exe").as_deref(), Some("nightly-2026-10-28-6f8c7a0"));
        assert_eq!(
            t("/home/a/.local/share/horizon-launcher/versions/v2026.10.13/horizon-client")
                .as_deref(),
            Some("v2026.10.13")
        );
        assert_eq!(t("/Users/a/Library/Application Support/horizon-launcher/versions/native-6f8c7a0-abc123/Horizon.app/Contents/MacOS/horizon-client").as_deref(), Some("native-6f8c7a0-abc123"));
        assert_eq!(t("/opt/horizon/horizon-client"), None);
        assert_eq!(t("/x/versions/horizon-client"), None, "no tag folder");
    }

    #[test]
    fn tags_parse() {
        assert_eq!(
            parse_tag("nightly-2026-10-28-6f8c7a0"),
            Some(TagKind::Nightly("6f8c7a0".into()))
        );
        assert_eq!(
            parse_tag("native-6f8c7a0-1a2b3c4d"),
            Some(TagKind::Native("6f8c7a0".into()))
        );
        assert_eq!(
            parse_tag("v2026.10.13"),
            Some(TagKind::Release("v2026.10.13".into()))
        );
        for bad in [
            "",
            "nightly-",
            "nightly-2026-10-28",
            "nightly-2026-10-28-zzzzzzz",
            "native-xyz-1",
            "latest",
            "v",
        ] {
            assert_eq!(parse_tag(bad), None, "{bad}");
        }
    }

    #[test]
    fn selection_by_commit_prefix_release_tag_platform_and_version() {
        let c = "6f8c7a0e5d531d3834c4d0be4659a1db8e7e5c2a";
        let old = edition(c, "", "0.1.0", false, "windows-x86_64");
        let new = edition(c, "", "0.2.0", false, "windows-x86_64");
        let server = edition(c, "", "0.9.0", true, "windows-x86_64");
        let linux = edition(c, "", "0.9.0", false, "linux-x86_64");
        let other = edition(
            "1111111e5d531d3834c4d0be4659a1db8e7e5c2a",
            "v2026.10.13",
            "0.1.0",
            false,
            "windows-x86_64",
        );
        let all = vec![old, new, server, linux, other];
        let pick = |tag: &str| {
            select(&all, tag, "windows-x86_64").map(|e| e.manifest.fish_version.clone())
        };
        assert_eq!(
            pick("nightly-2026-10-28-6f8c7a0").as_deref(),
            Some("0.2.0"),
            "newest client edition of this platform"
        );
        assert_eq!(pick("native-6f8c7a0-abc").as_deref(), Some("0.2.0"));
        assert_eq!(
            pick("v2026.10.13").as_deref(),
            Some("0.1.0"),
            "release tags match release_tag"
        );
        assert_eq!(pick("nightly-2026-10-29-abcdef0"), None);
        assert_eq!(pick("garbage"), None);
    }

    #[test]
    fn discovery_and_verification() {
        let home = std::env::temp_dir().join(format!("fish-launch-edition-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let dir = home.join("editions/0.1.0+horizon.6f8c7a0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("horizon-client"), b"binary").unwrap();
        let sha: String = Sha256::digest(b"binary")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        std::fs::write(
            dir.join("fish-edition.json"),
            format!("{{\"schema\":1,\"id\":\"0.1.0+horizon.6f8c7a0\",\"fish_version\":\"0.1.0\",\"horizon\":{{\"commit\":\"6f8c7a0e5d\"}},\"platform\":\"windows-x86_64\",\"extra\":1,\"binary\":{{\"name\":\"horizon-client\",\"sha256\":\"{sha}\"}}}}"),
        )
        .unwrap();
        std::fs::create_dir_all(home.join("editions/broken")).unwrap();
        std::fs::write(home.join("editions/broken/fish-edition.json"), "{").unwrap();
        std::fs::create_dir_all(home.join("editions/empty")).unwrap();
        let found = discover(&home);
        assert_eq!(found.len(), 1, "broken and empty folders are skipped");
        assert_eq!(found[0].binary_path(), dir.join("horizon-client"));
        assert_eq!(verify(&found[0]), Ok(()));
        std::fs::write(dir.join("horizon-client"), b"tampered").unwrap();
        assert_eq!(verify(&found[0]), Err(VerifyError::ChecksumMismatch));
        std::fs::remove_file(dir.join("horizon-client")).unwrap();
        assert_eq!(verify(&found[0]), Err(VerifyError::Missing));
        assert!(discover(&home.join("nothing")).is_empty());
        std::fs::remove_dir_all(home).ok();
    }
}
