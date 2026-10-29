//! `fish.toml` loading and consistency checks.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use regex::Regex;

/// Parsed subset of `fish.toml` used by xtask.
#[derive(Debug, Clone)]
pub struct FishConfig {
    pub version: String,
    pub abi: (u16, u16),
    pub horizon_commit: String,
    pub horizon_source_tag: String,
    pub horizon_release_line: String,
    pub horizon_release_tag: String,
    pub mixins_git: String,
    pub mixins_rev: String,
    pub coverage: BTreeMap<String, f64>,
    pub coverage_exclude: Vec<String>,
}

fn toml_int(v: &toml::Value, what: &str) -> Result<u16> {
    let n = v
        .as_integer()
        .ok_or_else(|| anyhow!("{what} must be an integer"))?;
    u16::try_from(n).with_context(|| format!("{what} out of range"))
}

/// Parses `fish.toml` text.
pub fn parse(text: &str) -> Result<FishConfig> {
    let doc: toml::Table = text.parse().context("fish.toml is not valid TOML")?;
    let fish = doc
        .get("fish")
        .and_then(|v| v.as_table())
        .ok_or_else(|| anyhow!("missing [fish]"))?;
    let version = fish
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing fish.version"))?
        .to_string();
    let abi = fish
        .get("abi")
        .and_then(|v| v.as_table())
        .ok_or_else(|| anyhow!("missing fish.abi"))?;
    let abi = (
        toml_int(
            abi.get("major")
                .ok_or_else(|| anyhow!("missing abi.major"))?,
            "abi.major",
        )?,
        toml_int(
            abi.get("minor")
                .ok_or_else(|| anyhow!("missing abi.minor"))?,
            "abi.minor",
        )?,
    );
    let mut coverage = BTreeMap::new();
    let mut coverage_exclude = Vec::new();
    if let Some(cov) = doc.get("coverage").and_then(|v| v.as_table()) {
        for (k, v) in cov {
            if k == "exclude" {
                coverage_exclude = v
                    .as_array()
                    .ok_or_else(|| anyhow!("coverage.exclude must be an array"))?
                    .iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect();
            } else {
                let gate = v
                    .as_float()
                    .or_else(|| v.as_integer().map(|i| i as f64))
                    .ok_or_else(|| anyhow!("coverage.{k} must be a number"))?;
                coverage.insert(k.clone(), gate);
            }
        }
    }
    let text_of = |section: &str, key: &str| -> String {
        doc.get(section)
            .and_then(|s| s.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    Ok(FishConfig {
        version,
        abi,
        horizon_commit: text_of("horizon", "commit"),
        horizon_source_tag: text_of("horizon", "source_tag"),
        horizon_release_line: text_of("horizon", "release_line"),
        horizon_release_tag: text_of("horizon", "release_tag"),
        mixins_git: text_of("mixins", "git"),
        mixins_rev: text_of("mixins", "rev"),
        coverage,
        coverage_exclude,
    })
}

/// Extracts `[workspace.package].version` from a workspace manifest.
pub fn workspace_version(cargo_toml: &str) -> Result<String> {
    let doc: toml::Table = cargo_toml.parse().context("Cargo.toml is not valid TOML")?;
    doc.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("workspace.package.version missing"))
}

/// Extracts `FISH_ABI_MAJOR`/`FISH_ABI_MINOR` constants from `fish-abi` source.
pub fn abi_constants(src: &str) -> Result<(u16, u16)> {
    let get = |name: &str| -> Result<u16> {
        let re = Regex::new(&format!(r"pub const {name}: u16 = (\d+);"))?;
        let caps = re
            .captures(src)
            .ok_or_else(|| anyhow!("{name} not found in fish-abi"))?;
        Ok(caps[1].parse()?)
    };
    Ok((get("FISH_ABI_MAJOR")?, get("FISH_ABI_MINOR")?))
}

/// Verifies `fish.toml` against the workspace and (when present) the `fish-abi` crate.
pub fn check(root: &Path) -> Result<Vec<String>> {
    let cfg =
        parse(&std::fs::read_to_string(root.join("fish.toml")).context("reading fish.toml")?)?;
    let mut errors = Vec::new();
    let ws = workspace_version(&std::fs::read_to_string(root.join("Cargo.toml"))?)?;
    if ws != cfg.version {
        errors.push(format!(
            "fish.toml version {} != workspace version {ws}",
            cfg.version
        ));
    }
    let abi_src = root.join("crates/fish-abi/src/lib.rs");
    if abi_src.exists() {
        let found = abi_constants(&std::fs::read_to_string(abi_src)?)?;
        if found != cfg.abi {
            errors.push(format!(
                "fish.toml abi {:?} != fish-abi constants {found:?}",
                cfg.abi
            ));
        }
    }
    Ok(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FISH: &str = "[fish]\nversion=\"0.1.0\"\nabi={major=0,minor=1}\n[horizon]\ncommit=\"abc\"\nsource_tag=\"nightly-source/2026-10-28-abc\"\nrelease_line=\"0.2\"\n[mixins]\ngit=\"g\"\nrev=\"r\"\n[coverage]\na=95\nb=87.5\nexclude=[\"x/*\"]\n";

    #[test]
    fn parses_fish_toml() {
        let c = parse(FISH).unwrap();
        assert_eq!(c.version, "0.1.0");
        assert_eq!(c.abi, (0, 1));
        assert_eq!(c.coverage["a"], 95.0);
        assert_eq!(c.coverage["b"], 87.5);
        assert_eq!(c.coverage_exclude, vec!["x/*"]);
        assert_eq!(
            (c.horizon_commit.as_str(), c.horizon_release_line.as_str()),
            ("abc", "0.2")
        );
        assert_eq!((c.mixins_git.as_str(), c.mixins_rev.as_str()), ("g", "r"));
        assert_eq!(c.horizon_release_tag, "");
        assert!(parse("[fish]\nversion=\"1\"\n").is_err());
        assert!(parse("nope").is_err());
    }

    #[test]
    fn reads_workspace_version_and_abi() {
        assert_eq!(
            workspace_version("[workspace.package]\nversion=\"2.0.0\"").unwrap(),
            "2.0.0"
        );
        assert!(workspace_version("[workspace]\n").is_err());
        let src = "pub const FISH_ABI_MAJOR: u16 = 0;\npub const FISH_ABI_MINOR: u16 = 3;";
        assert_eq!(abi_constants(src).unwrap(), (0, 3));
        assert!(abi_constants("").is_err());
    }

    #[test]
    fn check_detects_mismatch() {
        let dir = std::env::temp_dir().join(format!("fish-xtask-cfg-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("crates/fish-abi/src")).unwrap();
        std::fs::write(dir.join("fish.toml"), FISH).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace.package]\nversion=\"0.1.0\"",
        )
        .unwrap();
        std::fs::write(
            dir.join("crates/fish-abi/src/lib.rs"),
            "pub const FISH_ABI_MAJOR: u16 = 0;\npub const FISH_ABI_MINOR: u16 = 1;",
        )
        .unwrap();
        assert!(check(&dir).unwrap().is_empty());
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace.package]\nversion=\"0.2.0\"",
        )
        .unwrap();
        std::fs::write(
            dir.join("crates/fish-abi/src/lib.rs"),
            "pub const FISH_ABI_MAJOR: u16 = 0;\npub const FISH_ABI_MINOR: u16 = 2;",
        )
        .unwrap();
        assert_eq!(check(&dir).unwrap().len(), 2);
        std::fs::remove_dir_all(dir).ok();
    }
}
