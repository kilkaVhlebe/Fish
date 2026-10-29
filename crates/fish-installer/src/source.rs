//! Where editions come from: the release index, its signature, downloads and unpacking.

use std::io::Read;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Release public key built into the installer (`keys/release.pub`); empty until a key exists.
pub const RELEASE_KEY: &str = include_str!("../keys/release.pub");

/// `owner/repo` the default index is fetched from (set at build time; empty = not published yet).
pub const RELEASE_GITHUB: &str = match option_env!("FISH_RELEASE_GITHUB") {
    Some(s) => s,
    None => "",
};

/// One edition in the release index.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct IndexEntry {
    /// Edition id, `<fish version>+horizon.<short commit>`.
    pub id: String,
    /// Platform key.
    pub platform: String,
    /// True for dedicated-server editions.
    #[serde(default)]
    pub server: bool,
    /// Full Horizon commit.
    pub horizon_commit: String,
    /// Launcher-installable release tag of that Horizon build.
    #[serde(default)]
    pub horizon_release_tag: String,
    /// Download URL of the zip.
    pub url: String,
    /// Hex SHA-256 of the zip.
    pub sha256: String,
    /// Size of the zip in bytes.
    pub size: u64,
}

impl IndexEntry {
    /// The Fish version part of the id.
    pub fn fish_version(&self) -> Version {
        let v = self.id.split('+').next().unwrap_or("");
        Version::parse(v).unwrap_or_else(|_| Version::new(0, 0, 0))
    }
}

/// `editions.json`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Index {
    /// Index schema (1).
    pub schema: u32,
    /// Fish version of the release.
    pub fish_version: String,
    /// The editions.
    pub editions: Vec<IndexEntry>,
}

/// Parses an index; only schema 1 is understood.
pub fn parse_index(text: &str) -> Result<Index> {
    let index: Index = serde_json::from_str(text)
        .map_err(|e| Error::Source(format!("editions.json is not valid: {e}")))?;
    if index.schema != 1 {
        return Err(Error::Source(format!(
            "editions.json uses schema {}; this installer understands schema 1 (update fish-installer)",
            index.schema
        )));
    }
    Ok(index)
}

/// The newest client edition for `platform` (or the one named `want_id`).
pub fn select_entry<'a>(
    index: &'a Index,
    platform: &str,
    want_id: Option<&str>,
) -> Option<&'a IndexEntry> {
    index
        .editions
        .iter()
        .filter(|e| !e.server && e.platform == platform)
        .filter(|e| want_id.is_none_or(|w| e.id == w))
        .max_by(|a, b| {
            a.fish_version()
                .cmp(&b.fish_version())
                .then_with(|| a.horizon_commit.cmp(&b.horizon_commit))
        })
}

/// Hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Verifies a minisign signature of `data`.
pub fn verify_signature(data: &[u8], signature: &str, public_key: &str) -> Result<()> {
    let key = minisign_verify::PublicKey::decode(public_key.trim())
        .or_else(|_| {
            minisign_verify::PublicKey::from_base64(
                public_key.trim().lines().last().unwrap_or("").trim(),
            )
        })
        .map_err(|e| Error::Signature(format!("the built-in release key is invalid: {e}")))?;
    let sig = minisign_verify::Signature::decode(signature)
        .map_err(|e| Error::Signature(format!("editions.json.minisig is invalid: {e}")))?;
    key.verify(data, &sig, false)
        .map_err(|e| Error::Signature(format!("editions.json has a bad signature: {e}")))
}

/// Fetches bytes from a URL (replaceable in tests).
pub trait Fetcher {
    /// Downloads `url`.
    fn get(&self, url: &str) -> Result<Vec<u8>>;
}

/// Downloads with `ureq`.
pub struct HttpFetcher;

impl Fetcher for HttpFetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>> {
        let response = ureq::get(url)
            .call()
            .map_err(|e| Error::Source(format!("downloading {url}: {e}")))?;
        let mut body = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(2 << 30)
            .read_to_end(&mut body)
            .map_err(|e| Error::Source(format!("downloading {url}: {e}")))?;
        Ok(body)
    }
}

/// The base URL editions are served from: `FISH_LOADER_RELEASES_URL`, else the GitHub release.
pub fn releases_base(env_url: Option<&str>, github: &str) -> Result<String> {
    if let Some(u) = env_url.filter(|u| !u.is_empty()) {
        return Ok(u.trim_end_matches('/').to_string());
    }
    if github.is_empty() {
        return Err(Error::Source(
            "no release source is configured: pass --edition <zip> or set FISH_LOADER_RELEASES_URL"
                .into(),
        ));
    }
    Ok(format!(
        "https://github.com/{github}/releases/latest/download"
    ))
}

/// Downloads and verifies the index under `base`.
///
/// With `allow_unsigned` the signature is not required. Otherwise a built-in key and a valid
/// `editions.json.minisig` are mandatory.
pub fn load_index(
    fetch: &dyn Fetcher,
    base: &str,
    key: &str,
    allow_unsigned: bool,
) -> Result<Index> {
    let body = fetch.get(&format!("{base}/editions.json"))?;
    if !allow_unsigned {
        if key.trim().is_empty() {
            return Err(Error::Signature(
                "this installer has no release signing key, so it cannot verify editions; use --allow-unsigned (development only)".into(),
            ));
        }
        let sig = fetch.get(&format!("{base}/editions.json.minisig")).map_err(|e| {
            Error::Signature(format!("editions.json.minisig is missing ({e}); use --allow-unsigned only for development"))
        })?;
        verify_signature(&body, &String::from_utf8_lossy(&sig), key)?;
    }
    parse_index(&String::from_utf8_lossy(&body))
}

/// Downloads an edition zip and checks size and SHA-256 against the index entry.
pub fn download_edition(fetch: &dyn Fetcher, entry: &IndexEntry) -> Result<Vec<u8>> {
    let bytes = fetch.get(&entry.url)?;
    if bytes.len() as u64 != entry.size {
        return Err(Error::Integrity(format!(
            "{} is {} bytes, the index says {}",
            entry.url,
            bytes.len(),
            entry.size
        )));
    }
    if !sha256_hex(&bytes).eq_ignore_ascii_case(&entry.sha256) {
        return Err(Error::Integrity(format!(
            "{} does not match its SHA-256 in the index",
            entry.url
        )));
    }
    Ok(bytes)
}

const MAX_ENTRY: u64 = 1 << 30;
const MAX_TOTAL: u64 = 2 << 30;

/// Unpacks an edition zip into `<home>/editions/<id>/` (replacing an older copy) and checks the
/// binary against `fish-edition.json`. Returns the edition folder and its manifest.
pub fn unpack_edition(
    zip_bytes: &[u8],
    home: &Path,
) -> Result<(PathBuf, fish_launch::edition::EditionManifest)> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| Error::Integrity(format!("the edition is not a valid zip: {e}")))?;
    let mut names = Vec::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let f = archive
            .by_index_raw(i)
            .map_err(|e| Error::Integrity(e.to_string()))?;
        let name = f.name().to_string();
        fish_pack::validate_entry_name(&name)
            .map_err(|r| Error::Integrity(format!("unsafe entry {name:?} in the edition: {r}")))?;
        if f.unix_mode().is_some_and(|m| m & 0o170_000 == 0o120_000) {
            return Err(Error::Integrity(format!(
                "the edition contains a symbolic link ({name:?})"
            )));
        }
        if f.size() > MAX_ENTRY {
            return Err(Error::Integrity(format!("{name:?} is too large")));
        }
        total += f.size();
        names.push(name);
    }
    if total > MAX_TOTAL {
        return Err(Error::Integrity("the edition is too large".into()));
    }
    let manifest_text = {
        let mut f = archive
            .by_name("fish-edition.json")
            .map_err(|_| Error::Integrity("the edition has no fish-edition.json".into()))?;
        let mut s = String::new();
        f.read_to_string(&mut s)
            .map_err(|e| Error::Integrity(e.to_string()))?;
        s
    };
    let manifest: fish_launch::edition::EditionManifest = serde_json::from_str(&manifest_text)
        .map_err(|e| Error::Integrity(format!("fish-edition.json is not valid: {e}")))?;
    if manifest.id.is_empty()
        || !manifest
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-' | '_'))
        || manifest.id.starts_with('.')
    {
        return Err(Error::Integrity(format!(
            "edition id {:?} is not a safe folder name",
            manifest.id
        )));
    }
    let editions = home.join("editions");
    let staging = editions.join(format!(".unpacking-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)
        .map_err(|e| Error::io(format!("creating {}", staging.display()), e))?;
    for name in &names {
        if name.ends_with('/') {
            continue;
        }
        let mut f = archive
            .by_name(name)
            .map_err(|e| Error::Integrity(e.to_string()))?;
        let target = staging.join(name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io(format!("creating {}", parent.display()), e))?;
        }
        let mut out = std::fs::File::create(&target)
            .map_err(|e| Error::io(format!("writing {}", target.display()), e))?;
        std::io::copy(&mut (&mut f).take(MAX_ENTRY), &mut out)
            .map_err(|e| Error::io(format!("writing {}", target.display()), e))?;
    }
    let dest = editions.join(&manifest.id);
    let edition = fish_launch::edition::Edition {
        dir: staging.clone(),
        manifest: manifest.clone(),
    };
    if let Err(e) = fish_launch::edition::verify(&edition) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(Error::Integrity(format!(
            "the edition's binary does not match fish-edition.json ({e:?})"
        )));
    }
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|e| Error::io(format!("replacing {}", dest.display()), e))?;
    }
    std::fs::rename(&staging, &dest)
        .map_err(|e| Error::io(format!("installing {}", dest.display()), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = dest.join(&manifest.binary.name);
        let _ = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755));
    }
    Ok((dest, manifest))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Write;

    use super::*;

    fn entry(id: &str, platform: &str, server: bool) -> IndexEntry {
        IndexEntry {
            id: id.into(),
            platform: platform.into(),
            server,
            horizon_commit: "6f8c7a0e5d531d3834c4d0be4659a1db8e7e5c2a".into(),
            horizon_release_tag: "nightly-2026-10-29".into(),
            url: format!("https://x/{id}.zip"),
            sha256: "00".into(),
            size: 0,
        }
    }

    #[test]
    fn index_parsing_and_selection() {
        let text = r#"{"schema":1,"fish_version":"0.2.0","editions":[
            {"id":"0.1.0+horizon.aaaaaaa","platform":"windows-x86_64","horizon_commit":"aaaaaaa","url":"u1","sha256":"1","size":1},
            {"id":"0.2.0+horizon.bbbbbbb","platform":"windows-x86_64","horizon_commit":"bbbbbbb","url":"u2","sha256":"2","size":2},
            {"id":"0.9.0+horizon.ccccccc","platform":"windows-x86_64","server":true,"horizon_commit":"ccccccc","url":"u3","sha256":"3","size":3},
            {"id":"0.9.0+horizon.ddddddd","platform":"linux-x86_64","horizon_commit":"ddddddd","url":"u4","sha256":"4","size":4}]}"#;
        let index = parse_index(text).unwrap();
        let pick = |p: &str, id: Option<&str>| select_entry(&index, p, id).map(|e| e.id.clone());
        assert_eq!(
            pick("windows-x86_64", None).as_deref(),
            Some("0.2.0+horizon.bbbbbbb")
        );
        assert_eq!(
            pick("windows-x86_64", Some("0.1.0+horizon.aaaaaaa")).as_deref(),
            Some("0.1.0+horizon.aaaaaaa")
        );
        assert_eq!(
            pick("linux-x86_64", None).as_deref(),
            Some("0.9.0+horizon.ddddddd")
        );
        assert_eq!(pick("macos-aarch64", None), None);
        assert_eq!(pick("windows-x86_64", Some("nope")), None);
        assert!(
            parse_index("{\"schema\":2,\"fish_version\":\"1\",\"editions\":[]}")
                .unwrap_err()
                .to_string()
                .contains("schema 2")
        );
        assert!(parse_index("nope").is_err());
        assert_eq!(
            entry("bad", "p", false).fish_version(),
            Version::new(0, 0, 0)
        );
    }

    #[test]
    fn release_base_urls() {
        assert_eq!(
            releases_base(Some("http://h/x/"), "").unwrap(),
            "http://h/x"
        );
        assert_eq!(
            releases_base(Some(""), "o/r").unwrap(),
            "https://github.com/o/r/releases/latest/download"
        );
        assert_eq!(
            releases_base(None, "o/r").unwrap(),
            "https://github.com/o/r/releases/latest/download"
        );
        assert!(
            releases_base(None, "")
                .unwrap_err()
                .to_string()
                .contains("--edition")
        );
    }

    struct Fake(HashMap<String, Vec<u8>>);
    impl Fetcher for Fake {
        fn get(&self, url: &str) -> Result<Vec<u8>> {
            self.0
                .get(url)
                .cloned()
                .ok_or_else(|| Error::Source(format!("404 {url}")))
        }
    }

    #[test]
    fn unsigned_index_policy() {
        let body = br#"{"schema":1,"fish_version":"0.1.0","editions":[]}"#.to_vec();
        let fake = Fake(HashMap::from([("b/editions.json".to_string(), body)]));
        assert!(
            load_index(&fake, "b", "", true).is_ok(),
            "--allow-unsigned skips the signature"
        );
        assert!(
            load_index(&fake, "b", "", false)
                .unwrap_err()
                .to_string()
                .contains("no release signing key")
        );
        let err = load_index(&fake, "b", "untrusted comment: x\nRWQ", false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("editions.json.minisig is missing"), "{err}");
        assert!(load_index(&Fake(HashMap::new()), "b", "", true).is_err());
    }

    #[test]
    fn a_signed_index_is_accepted_and_a_forged_one_is_not() {
        let keys = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let key = keys.pk.to_box().unwrap().to_string();
        let body = br#"{"schema":1,"fish_version":"0.1.0","editions":[]}"#.to_vec();
        let sign = |data: &[u8]| {
            minisign::sign(
                Some(&keys.pk),
                &keys.sk,
                std::io::Cursor::new(data.to_vec()),
                None,
                None,
            )
            .unwrap()
            .to_string()
            .into_bytes()
        };
        let ok = Fake(HashMap::from([
            ("b/editions.json".to_string(), body.clone()),
            ("b/editions.json.minisig".to_string(), sign(&body)),
        ]));
        assert_eq!(
            load_index(&ok, "b", &key, false).unwrap().fish_version,
            "0.1.0"
        );
        let forged = Fake(HashMap::from([
            (
                "b/editions.json".to_string(),
                br#"{"schema":1,"fish_version":"6.6.6","editions":[]}"#.to_vec(),
            ),
            ("b/editions.json.minisig".to_string(), sign(&body)),
        ]));
        assert!(
            load_index(&forged, "b", &key, false)
                .unwrap_err()
                .to_string()
                .contains("bad signature")
        );
        // another key's signature is refused too
        let other = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let wrong_key = other.pk.to_box().unwrap().to_string();
        assert!(load_index(&ok, "b", &wrong_key, false).is_err());
    }

    #[test]
    fn signature_verification_good_bad_and_garbage() {
        let keys = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let pk_box = keys.pk.to_box().unwrap().to_string();
        let pk = pk_box.lines().last().unwrap().to_string();
        let good_sig = minisign::sign(
            Some(&keys.pk),
            &keys.sk,
            std::io::Cursor::new(b"test"),
            Some("t"),
            Some("u"),
        )
        .unwrap()
        .to_string();
        let good_sig = good_sig.as_str();
        let pk = pk.as_str();
        assert!(verify_signature(b"test", good_sig, pk).is_ok());
        assert!(
            verify_signature(b"tampered", good_sig, pk)
                .unwrap_err()
                .to_string()
                .contains("bad signature")
        );
        assert!(
            verify_signature(b"test", "garbage", pk)
                .unwrap_err()
                .to_string()
                .contains("minisig is invalid")
        );
        assert!(
            verify_signature(b"test", good_sig, "not a key")
                .unwrap_err()
                .to_string()
                .contains("invalid")
        );
        // a full public key file (comment line + key) is accepted too
        let file = format!("untrusted comment: minisign public key\n{pk}\n");
        assert!(verify_signature(b"test", good_sig, &file).is_ok());
    }

    #[test]
    fn downloads_are_checked_against_size_and_hash() {
        let data = b"zip bytes".to_vec();
        let mut e = entry("0.1.0+horizon.aaaaaaa", "p", false);
        e.sha256 = sha256_hex(&data);
        e.size = data.len() as u64;
        let fake = Fake(HashMap::from([(e.url.clone(), data.clone())]));
        assert_eq!(download_edition(&fake, &e).unwrap(), data);
        let mut wrong_size = e.clone();
        wrong_size.size += 1;
        assert!(
            download_edition(&fake, &wrong_size)
                .unwrap_err()
                .to_string()
                .contains("bytes")
        );
        let mut wrong_hash = e.clone();
        wrong_hash.sha256 = "ab".into();
        assert!(
            download_edition(&fake, &wrong_hash)
                .unwrap_err()
                .to_string()
                .contains("SHA-256")
        );
    }

    fn edition_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let o = zip::write::SimpleFileOptions::default();
            for (n, d) in entries {
                z.start_file(*n, o).unwrap();
                z.write_all(d).unwrap();
            }
            z.finish().unwrap();
        }
        buf
    }

    fn manifest_json(id: &str, bin: &[u8]) -> String {
        format!(
            r#"{{"schema":1,"id":"{id}","fish_version":"0.1.0","horizon":{{"commit":"6f8c7a0e5d"}},"platform":"windows-x86_64","binary":{{"name":"horizon-client","sha256":"{}"}}}}"#,
            sha256_hex(bin)
        )
    }

    #[test]
    fn unpacking_validates_names_ids_and_the_binary() {
        let home = tempfile::tempdir().unwrap();
        let bin = b"BIN".as_slice();
        let good = edition_zip(&[
            ("horizon-client", bin),
            (
                "fish-edition.json",
                manifest_json("0.1.0+horizon.6f8c7a0", bin).as_bytes(),
            ),
            ("LICENSE-FISH", b"MIT"),
        ]);
        let (dir, m) = unpack_edition(&good, home.path()).unwrap();
        assert_eq!(dir, home.path().join("editions/0.1.0+horizon.6f8c7a0"));
        assert_eq!(m.fish_version, "0.1.0");
        assert_eq!(std::fs::read(dir.join("horizon-client")).unwrap(), b"BIN");
        // unpacking again replaces the folder
        std::fs::write(dir.join("stale"), b"x").unwrap();
        unpack_edition(&good, home.path()).unwrap();
        assert!(!dir.join("stale").exists());
        assert!(!home.path().join("editions").read_dir().unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".unpacking")
        }));

        let err = |zip: Vec<u8>| unpack_edition(&zip, home.path()).unwrap_err().to_string();
        assert!(err(b"not a zip".to_vec()).contains("not a valid zip"));
        assert!(err(edition_zip(&[("fish-edition.json", b"{}")])).contains("not valid"));
        assert!(err(edition_zip(&[("horizon-client", bin)])).contains("no fish-edition.json"));
        assert!(
            err(edition_zip(&[
                ("../evil", b"x"),
                ("fish-edition.json", manifest_json("a", bin).as_bytes())
            ]))
            .contains("unsafe entry")
        );
        assert!(
            err(edition_zip(&[
                ("horizon-client", bin),
                (
                    "fish-edition.json",
                    manifest_json("../escape", bin).as_bytes()
                )
            ]))
            .contains("safe folder name")
        );
        assert!(
            err(edition_zip(&[
                ("horizon-client", b"TAMPERED"),
                (
                    "fish-edition.json",
                    manifest_json("0.1.0+horizon.x", bin).as_bytes()
                )
            ]))
            .contains("does not match")
        );
        assert!(
            err(edition_zip(&[(
                "fish-edition.json",
                manifest_json("0.1.0+horizon.y", bin).as_bytes()
            )]))
            .contains("does not match")
        );
    }
}
