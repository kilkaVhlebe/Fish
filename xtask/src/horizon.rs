//! Fetching Horizon's source, installing horizon-mixins, and staging the edition workspace.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use walkdir::WalkDir;

use crate::config::FishConfig;

/// Crates compiled into the edition, in dependency order.
pub const EDITION_CRATES: [&str; 5] = [
    "fish-abi",
    "fish-metadata",
    "fish-pack",
    "fish-hooks",
    "fish-loader",
];

/// `.horizon/` of the repository.
pub fn horizon_dir(root: &Path) -> PathBuf {
    root.join(".horizon")
}

/// Where the pristine source of `commit` lives.
pub fn source_dir(root: &Path, commit: &str) -> PathBuf {
    horizon_dir(root).join("source").join(commit)
}

/// Where the working copies of `commit` live.
pub fn work_dir(root: &Path, commit: &str) -> PathBuf {
    horizon_dir(root).join("work").join(commit)
}

/// Unpacks a `.tar.gz` of a GitHub archive into `dest`, stripping the top folder and skipping
/// everything that is not a plain file or directory (links are refused by horizon-mixins).
pub fn extract_tar_gz(reader: impl Read, dest: &Path) -> Result<usize> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(reader));
    let mut files = 0;
    for entry in archive.entries().context("reading the archive")? {
        let mut entry = entry.context("reading an archive entry")?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            continue;
        }
        let path = entry.path()?.into_owned();
        let mut parts = path.components();
        parts.next(); // the archive's top folder
        let mut target = dest.to_path_buf();
        for c in parts {
            match c {
                Component::Normal(n) => target.push(n),
                Component::CurDir => {}
                _ => bail!("unsafe path in archive: {}", path.display()),
            }
        }
        if target == dest {
            continue;
        }
        if kind.is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            entry
                .unpack(&target)
                .with_context(|| format!("unpacking {}", path.display()))?;
            files += 1;
        }
    }
    Ok(files)
}

/// Downloads and unpacks the pinned Horizon commit; a no-op if `.horizon-revision` matches.
pub fn fetch(root: &Path, cfg: &FishConfig) -> Result<PathBuf> {
    let commit = &cfg.horizon_commit;
    let dest = source_dir(root, commit);
    let stamp = dest.join(".horizon-revision");
    if fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == commit) {
        eprintln!("horizon {commit}: source already present");
        return Ok(dest);
    }
    let url = format!("https://codeload.github.com/horizon-rust/horizon/tar.gz/{commit}");
    eprintln!("downloading {url}");
    let response = ureq::get(&url)
        .call()
        .with_context(|| format!("downloading {url}"))?;
    let partial = dest.with_extension("partial");
    if partial.exists() {
        fs::remove_dir_all(&partial)?;
    }
    fs::create_dir_all(&partial)?;
    let files = extract_tar_gz(response.into_body().into_reader(), &partial)?;
    fs::write(partial.join(".horizon-revision"), format!("{commit}\n"))?;
    if dest.exists() {
        fs::remove_dir_all(&dest)?;
    }
    fs::rename(&partial, &dest)?;
    eprintln!("horizon {commit}: unpacked {files} files");
    Ok(dest)
}

/// Path of the installed `horizon-mixins` binary.
pub fn mixins_bin(root: &Path, rev: &str) -> PathBuf {
    let name = if cfg!(windows) {
        "horizon-mixins.exe"
    } else {
        "horizon-mixins"
    };
    horizon_dir(root)
        .join("tools")
        .join(rev)
        .join("bin")
        .join(name)
}

/// Installs horizon-mixins at the pinned revision and checks that it runs.
pub fn mixins_install(root: &Path, cfg: &FishConfig) -> Result<PathBuf> {
    let bin = mixins_bin(root, &cfg.mixins_rev);
    if !bin.is_file() {
        let tools = horizon_dir(root).join("tools").join(&cfg.mixins_rev);
        crate::run(
            root,
            "cargo",
            &[
                "install",
                "--locked",
                "--git",
                &cfg.mixins_git,
                "--rev",
                &cfg.mixins_rev,
                "horizon-mixins-cli",
                "--root",
                &tools.display().to_string(),
            ],
        )?;
    }
    let out = Command::new(&bin)
        .arg("--version")
        .output()
        .context("running horizon-mixins")?;
    if !out.status.success() {
        bail!("{} --version failed", bin.display());
    }
    eprintln!("{}", String::from_utf8_lossy(&out.stdout).trim());
    Ok(bin)
}

/// Declarative edits to the staged Horizon workspace (`horizon/cargo-edits.toml`).
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CargoEdits {
    /// Workspace members to add.
    #[serde(default)]
    pub members: Vec<String>,
    /// Dependencies to add to crates.
    #[serde(default, rename = "dependency")]
    pub dependencies: Vec<DependencyEdit>,
}

/// One dependency to add.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DependencyEdit {
    /// The crate directory, relative to the workspace.
    #[serde(rename = "crate")]
    pub krate: String,
    /// Dependency name.
    pub name: String,
    /// Path of the dependency, relative to that crate.
    pub path: String,
}

/// Adds `members` to a workspace manifest; a duplicate is an error naming it.
pub fn add_members(workspace_toml: &str, members: &[String]) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = workspace_toml
        .parse()
        .context("parsing the workspace Cargo.toml")?;
    let array = doc["workspace"]["members"]
        .as_array_mut()
        .context("workspace.members is not an array")?;
    for m in members {
        if array.iter().any(|v| v.as_str() == Some(m)) {
            bail!("workspace member {m} already exists");
        }
        array.push(m.as_str());
    }
    Ok(doc.to_string())
}

/// Adds a path dependency to a crate manifest; a duplicate is an error naming it.
pub fn add_dependency(crate_toml: &str, name: &str, path: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut =
        crate_toml.parse().context("parsing a crate Cargo.toml")?;
    let deps = doc["dependencies"]
        .as_table_mut()
        .context("the crate has no [dependencies]")?;
    if deps.contains_key(name) {
        bail!("dependency {name} already exists");
    }
    let mut inline = toml_edit::InlineTable::new();
    inline.insert("path", path.into());
    deps.insert(name, toml_edit::Item::Value(inline.into()));
    Ok(doc.to_string())
}

/// Replaces `*.workspace = true` keys and `[lints] workspace = true` with concrete values from
/// Fish's workspace manifest, and drops dev-dependencies (tests are not part of the edition).
pub fn rewrite_manifest(crate_toml: &str, workspace_toml: &str) -> Result<String> {
    let ws: toml_edit::DocumentMut = workspace_toml
        .parse()
        .context("parsing the Fish workspace manifest")?;
    let mut doc: toml_edit::DocumentMut = crate_toml.parse().context("parsing a crate manifest")?;
    let package_values = ws["workspace"]["package"]
        .as_table()
        .context("workspace.package is missing")?;
    if let Some(package) = doc["package"].as_table_mut() {
        let keys: Vec<String> = package.iter().map(|(k, _)| k.to_string()).collect();
        for key in keys {
            let inherits = package[&key]
                .as_table_like()
                .is_some_and(|t| t.get("workspace").and_then(|w| w.as_bool()) == Some(true));
            if inherits {
                let value = package_values
                    .get(&key)
                    .with_context(|| format!("workspace.package has no {key}"))?
                    .clone();
                package.insert(&key, value);
            }
        }
    }
    let lints_inherit = doc
        .get("lints")
        .and_then(|l| l.as_table_like())
        .is_some_and(|t| t.get("workspace").and_then(|w| w.as_bool()) == Some(true));
    if lints_inherit {
        doc.remove("lints");
        if let Some(lints) = ws["workspace"].get("lints") {
            doc.insert("lints", lints.clone());
        }
    }
    doc.remove("dev-dependencies");
    Ok(doc.to_string())
}

/// Copies `from` to `to`, creating directories; returns the number of files.
pub fn copy_tree(from: &Path, to: &Path) -> Result<usize> {
    let mut n = 0;
    for entry in WalkDir::new(from) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(from)?;
        let target = to.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Builds `.horizon/work/<commit>/src` from the pristine source plus the edition crates.
pub fn stage(root: &Path, cfg: &FishConfig) -> Result<PathBuf> {
    let commit = &cfg.horizon_commit;
    let pristine = source_dir(root, commit);
    if !pristine.join(".horizon-revision").is_file() {
        bail!("no source for {commit}: run `cargo xtask horizon fetch` first");
    }
    let work = work_dir(root, commit);
    let src = work.join("src");
    if src.exists() {
        fs::remove_dir_all(&src).with_context(|| format!("removing {}", src.display()))?;
    }
    let files = copy_tree(&pristine, &src)?;
    eprintln!("staged {files} files of Horizon {commit}");

    let ws_text = fs::read_to_string(root.join("Cargo.toml"))?;
    for name in EDITION_CRATES {
        let from = root.join("crates").join(name);
        let to = src.join("fish/crates").join(name);
        copy_tree(&from.join("src"), &to.join("src"))?;
        let manifest = rewrite_manifest(&fs::read_to_string(from.join("Cargo.toml"))?, &ws_text)
            .with_context(|| format!("rewriting {name}/Cargo.toml"))?;
        fs::write(to.join("Cargo.toml"), manifest)?;
    }

    let edits: CargoEdits =
        toml::from_str(&fs::read_to_string(root.join("horizon/cargo-edits.toml"))?)
            .context("parsing horizon/cargo-edits.toml")?;
    let ws_path = src.join("Cargo.toml");
    fs::write(
        &ws_path,
        add_members(&fs::read_to_string(&ws_path)?, &edits.members)?,
    )?;
    for dep in &edits.dependencies {
        let path = src.join(&dep.krate).join("Cargo.toml");
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        fs::write(&path, add_dependency(&text, &dep.name, &dep.path)?)?;
    }

    // Adds only the missing lock entries; then everything must resolve from the lock.
    crate::run_silent(
        &src,
        "cargo",
        &["metadata", "--format-version", "1", "--quiet"],
    )?;
    crate::run(&src, "cargo", &["fetch", "--locked"])?;
    Ok(src)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WS: &str = "[workspace]\nmembers = [\"x\"]\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"MIT\"\nrust-version = \"1.99\"\n[workspace.lints.rust]\nunsafe_op_in_unsafe_fn = \"deny\"\n[workspace.lints.clippy]\nall = \"warn\"\n";

    #[test]
    fn manifest_rewrite_inlines_workspace_values() {
        let crate_toml = "[package]\nname = \"fish-x\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\nrust-version.workspace = true\n\n[lints]\nworkspace = true\n\n[dependencies]\nfish-abi = { path = \"../fish-abi\" }\n\n[dev-dependencies]\ntempfile = \"3\"\n";
        let out = rewrite_manifest(crate_toml, WS).unwrap();
        let doc: toml::Table = out.parse().unwrap();
        let pkg = &doc["package"];
        assert_eq!(pkg["version"].as_str(), Some("0.1.0"));
        assert_eq!(pkg["edition"].as_str(), Some("2024"));
        assert_eq!(pkg["license"].as_str(), Some("MIT"));
        assert_eq!(pkg["rust-version"].as_str(), Some("1.99"));
        assert_eq!(
            doc["lints"]["rust"]["unsafe_op_in_unsafe_fn"].as_str(),
            Some("deny")
        );
        assert_eq!(doc["lints"]["clippy"]["all"].as_str(), Some("warn"));
        assert!(doc["lints"].get("workspace").is_none());
        assert_eq!(
            doc["dependencies"]["fish-abi"]["path"].as_str(),
            Some("../fish-abi")
        );
        assert!(doc.get("dev-dependencies").is_none());
        assert!(
            !out.contains("workspace = true") && !out.contains("workspace=true"),
            "{out}"
        );
    }

    #[test]
    fn manifest_rewrite_reports_missing_workspace_keys() {
        let err = rewrite_manifest("[package]\nname = \"x\"\nauthors.workspace = true\n", WS)
            .unwrap_err();
        assert!(err.to_string().contains("authors"), "{err}");
        // a crate without inherited keys or lints is left alone
        let plain = "[package]\nname = \"x\"\nversion = \"9.9.9\"\n";
        assert!(rewrite_manifest(plain, WS).unwrap().contains("9.9.9"));
    }

    #[test]
    fn members_and_dependencies_are_added_once() {
        let ws = "[workspace]\nmembers = [\n    \"crates/a\",\n]\n";
        let out = add_members(ws, &["fish/crates/x".into(), "fish/crates/y".into()]).unwrap();
        assert!(
            out.contains("fish/crates/x")
                && out.contains("fish/crates/y")
                && out.contains("crates/a")
        );
        assert!(
            add_members(&out, &["fish/crates/x".into()])
                .unwrap_err()
                .to_string()
                .contains("fish/crates/x")
        );
        assert!(add_members("[package]\n", &[]).is_err());

        let krate = "[package]\nname = \"c\"\n\n[dependencies]\nlog = \"0.4\"\n";
        let out = add_dependency(krate, "fish-loader", "../../fish/crates/fish-loader").unwrap();
        let doc: toml::Table = out.parse().unwrap();
        assert_eq!(
            doc["dependencies"]["fish-loader"]["path"].as_str(),
            Some("../../fish/crates/fish-loader")
        );
        assert_eq!(
            doc["dependencies"]["log"].as_str(),
            Some("0.4"),
            "existing entries are untouched"
        );
        assert!(
            add_dependency(&out, "fish-loader", "x")
                .unwrap_err()
                .to_string()
                .contains("fish-loader")
        );
        assert!(add_dependency("[package]\nname = \"c\"\n", "a", "b").is_err());
    }

    #[test]
    fn cargo_edits_file_parses_and_rejects_unknown_keys() {
        let e: CargoEdits = toml::from_str(
            "members = [\"a\"]\n[[dependency]]\ncrate = \"c\"\nname = \"n\"\npath = \"p\"\n",
        )
        .unwrap();
        assert_eq!(e.members, ["a"]);
        assert_eq!(
            e.dependencies[0],
            DependencyEdit {
                krate: "c".into(),
                name: "n".into(),
                path: "p".into()
            }
        );
        assert!(toml::from_str::<CargoEdits>("bogus = 1").is_err());
    }

    #[test]
    fn tar_extraction_strips_the_top_folder_and_skips_links() {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        let mut buf = Vec::new();
        {
            let gz = GzEncoder::new(&mut buf, Compression::fast());
            let mut b = tar::Builder::new(gz);
            let add_file = |b: &mut tar::Builder<_>, path: &str, data: &[u8]| {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                b.append_data(&mut h, path, data).unwrap();
            };
            add_file(&mut b, "top/a.txt", b"A");
            add_file(&mut b, "top/sub/b.txt", b"B");
            let mut link = tar::Header::new_gnu();
            link.set_entry_type(tar::EntryType::Symlink);
            link.set_size(0);
            link.set_cksum();
            b.append_link(&mut link, "top/link", "a.txt").unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let dir = std::env::temp_dir().join(format!("fish-xtask-tar-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(extract_tar_gz(&buf[..], &dir).unwrap(), 2);
        assert_eq!(fs::read(dir.join("a.txt")).unwrap(), b"A");
        assert_eq!(fs::read(dir.join("sub/b.txt")).unwrap(), b"B");
        assert!(!dir.join("link").exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn paths_are_derived_from_the_commit() {
        let root = Path::new("/r");
        assert_eq!(source_dir(root, "abc"), Path::new("/r/.horizon/source/abc"));
        assert_eq!(work_dir(root, "abc"), Path::new("/r/.horizon/work/abc"));
        assert!(mixins_bin(root, "rev").starts_with("/r/.horizon/tools/rev/bin"));
    }
}
