//! `cargo xtask horizon pin`: choose the Horizon commit Fish builds against.

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde_json::Value;

const REPO: &str = "horizon-rust/horizon";

/// A dated Nightly release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatedRelease {
    /// Release tag, `nightly-YYYY-MM-DD`.
    pub tag: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// The commit the release was built from.
    pub commit: String,
}

/// The newest release whose tag is `nightly-YYYY-MM-DD`.
pub fn pick_latest_dated(releases: &Value) -> Option<DatedRelease> {
    let re = Regex::new(r"^nightly-(\d{4}-\d{2}-\d{2})$").expect("static regex");
    releases
        .as_array()?
        .iter()
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            let date = re.captures(tag)?[1].to_string();
            let commit = r["target_commitish"].as_str()?.to_string();
            (commit.len() == 40).then(|| DatedRelease {
                tag: tag.to_string(),
                date,
                commit,
            })
        })
        .max_by(|a, b| a.date.cmp(&b.date))
}

/// The `nightly-source/<date>-<short>` tag that points at `commit`.
pub fn source_tag_for(date: &str, commit: &str, refs: &Value) -> Option<String> {
    refs.as_array()?.iter().find_map(|r| {
        let name = r["ref"].as_str()?.strip_prefix("refs/tags/")?;
        let sha = r["object"]["sha"].as_str()?;
        (name.starts_with(&format!("nightly-source/{date}-")) && sha == commit)
            .then(|| name.to_string())
    })
}

/// Rewrites `[horizon]` in `fish.toml`, keeping everything else (comments, order) as it was.
pub fn update_fish_toml(
    text: &str,
    commit: &str,
    source_tag: &str,
    release_tag: &str,
) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("parsing fish.toml")?;
    let horizon = doc["horizon"]
        .as_table_mut()
        .context("fish.toml has no [horizon] table")?;
    for (key, value) in [
        ("commit", commit),
        ("source_tag", source_tag),
        ("release_tag", release_tag),
    ] {
        let item = horizon
            .get_mut(key)
            .with_context(|| format!("[horizon] has no {key}"))?;
        let decor = item.as_value().map(|v| v.decor().clone());
        *item = toml_edit::value(value);
        if let (Some(decor), Some(v)) = (decor, item.as_value_mut()) {
            *v.decor_mut() = decor;
        }
    }
    Ok(doc.to_string())
}

/// True if the native hooks file at that commit has the hook point Fish needs.
pub fn has_native_mods(native_hooks_source: &str) -> bool {
    native_hooks_source.contains("fn native_mods")
}

fn get_json(url: &str) -> Result<Value> {
    let mut req = ureq::get(url)
        .header("User-Agent", "fish-xtask")
        .header("Accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let mut resp = req.call().with_context(|| format!("GET {url}"))?;
    let text = resp
        .body_mut()
        .read_to_string()
        .with_context(|| format!("reading {url}"))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {url}"))
}

/// What a pin resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// Full commit.
    pub commit: String,
    /// `nightly-source/<date>-<short>`.
    pub source_tag: String,
    /// `nightly-YYYY-MM-DD`.
    pub release_tag: String,
}

/// Resolves the newest dated Nightly and checks that Fish can hook it.
pub fn resolve_latest_dated() -> Result<Pin> {
    let releases = get_json(&format!(
        "https://api.github.com/repos/{REPO}/releases?per_page=100"
    ))?;
    let release = pick_latest_dated(&releases)
        .context("no dated Nightly release (nightly-YYYY-MM-DD) found")?;
    let refs = get_json(&format!(
        "https://api.github.com/repos/{REPO}/git/matching-refs/tags/nightly-source/{}",
        release.date
    ))?;
    let source_tag = source_tag_for(&release.date, &release.commit, &refs).with_context(|| {
        format!(
            "{} has no nightly-source/{}-* tag for {}",
            release.tag, release.date, release.commit
        )
    })?;
    let url = format!(
        "https://raw.githubusercontent.com/{REPO}/{}/crates/horizon-client/src/game/native_hooks.rs",
        release.commit
    );
    let source = ureq::get(&url)
        .header("User-Agent", "fish-xtask")
        .call()
        .with_context(|| format!("GET {url}"))?
        .body_mut()
        .read_to_string()
        .with_context(|| format!("reading {url}"))?;
    if !has_native_mods(&source) {
        bail!(
            "{} has no native_mods hook point; Fish cannot be built on it",
            release.commit
        );
    }
    Ok(Pin {
        commit: release.commit,
        source_tag,
        release_tag: release.tag,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_newest_dated_nightly_is_picked() {
        let releases = json!([
            {"tag_name": "launcher@0.2.24", "target_commitish": "2b294bbe3a8a3d2fc223320a40b047c239ed8906"},
            {"tag_name": "nightly", "target_commitish": "6f8c7a0e5d531d3834c4d0be4659a1db8e7e5c2a"},
            {"tag_name": "nightly-2026-10-27", "target_commitish": "795490271d26b1ccc5b0dbd04c64dfb413b6128a"},
            {"tag_name": "nightly-2026-10-28", "target_commitish": "9669600c389ad6d205db4ec0bb061935f2a38d0c"},
            {"tag_name": "nightly-2026-10-29", "target_commitish": "main"},
            {"tag_name": "v0.2.97", "target_commitish": "a3acb5f0d2f59d2d5ff21a5aa000809ba1e35e2f"}
        ]);
        let r = pick_latest_dated(&releases).unwrap();
        assert_eq!(r.tag, "nightly-2026-10-28");
        assert_eq!(r.date, "2026-10-28");
        assert_eq!(r.commit, "9669600c389ad6d205db4ec0bb061935f2a38d0c");
        assert_eq!(pick_latest_dated(&json!([])), None);
        assert_eq!(pick_latest_dated(&json!({"message": "rate limited"})), None);
    }

    #[test]
    fn the_source_tag_must_name_the_same_commit() {
        let refs = json!([
            {"ref": "refs/tags/nightly-source/2026-10-28-6f8c7a0", "object": {"sha": "6f8c7a0e5d531d3834c4d0be4659a1db8e7e5c2a"}},
            {"ref": "refs/tags/nightly-source/2026-10-28-9669600", "object": {"sha": "9669600c389ad6d205db4ec0bb061935f2a38d0c"}},
            {"ref": "refs/tags/nightly-source/2026-10-27-7954902", "object": {"sha": "795490271d26b1ccc5b0dbd04c64dfb413b6128a"}}
        ]);
        let commit = "9669600c389ad6d205db4ec0bb061935f2a38d0c";
        assert_eq!(
            source_tag_for("2026-10-28", commit, &refs).as_deref(),
            Some("nightly-source/2026-10-28-9669600")
        );
        assert_eq!(
            source_tag_for(
                "2026-10-28",
                "0000000000000000000000000000000000000000",
                &refs
            ),
            None
        );
        assert_eq!(source_tag_for("2026-10-29", commit, &refs), None);
        assert_eq!(source_tag_for("2026-10-28", commit, &json!("nope")), None);
    }

    #[test]
    fn fish_toml_is_updated_in_place() {
        let text = "[fish]\nversion = \"0.1.0\"\n\n[horizon]\ncommit = \"old\"                        # pinned\nsource_tag = \"old-tag\"\nrelease_line = \"0.2\"\nrelease_tag = \"\"                       # set by pin\n\n[mixins]\nrev = \"r\"\n";
        let out = update_fish_toml(
            text,
            "new",
            "nightly-source/2026-10-29-abc",
            "nightly-2026-10-29",
        )
        .unwrap();
        let doc: toml::Table = out.parse().unwrap();
        assert_eq!(doc["horizon"]["commit"].as_str(), Some("new"));
        assert_eq!(
            doc["horizon"]["source_tag"].as_str(),
            Some("nightly-source/2026-10-29-abc")
        );
        assert_eq!(
            doc["horizon"]["release_tag"].as_str(),
            Some("nightly-2026-10-29")
        );
        assert_eq!(
            doc["horizon"]["release_line"].as_str(),
            Some("0.2"),
            "other keys stay"
        );
        assert_eq!(doc["mixins"]["rev"].as_str(), Some("r"));
        assert!(
            out.contains("# pinned") && out.contains("# set by pin"),
            "comments survive:\n{out}"
        );
        assert!(update_fish_toml("[fish]\n", "a", "b", "c").is_err());
        assert!(
            update_fish_toml("[horizon]\ncommit = \"x\"\n", "a", "b", "c")
                .unwrap_err()
                .to_string()
                .contains("source_tag")
        );
    }

    #[test]
    fn the_hook_point_check() {
        assert!(has_native_mods(
            "pub(super) fn native_mods(&mut self, mods: &mut Vec<NativeMod>) {}"
        ));
        assert!(!has_native_mods("fn native_tick() {}"));
    }
}
