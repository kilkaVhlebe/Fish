//! `cargo fish build`: compile the library and pack the `.fish` archive.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use fish_metadata::Manifest;
use fish_pack::PackSpec;

/// Options of a build.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// Build with the release profile.
    pub release: bool,
    /// Platform key to build for (default: this machine).
    pub target_platform: Option<String>,
    /// Cargo target directory (default: cargo's choice).
    pub target_dir: Option<PathBuf>,
}

/// A finished build.
#[derive(Debug, Clone)]
pub struct BuiltMod {
    /// Path of the `.fish` archive.
    pub path: PathBuf,
    /// Mod id.
    pub id: String,
    /// Mod version.
    pub version: String,
}

/// Rust target triple for a Fish platform key, `None` for an unknown key.
pub fn triple_for(platform: &str) -> Option<&'static str> {
    Some(match platform {
        "windows-x86_64" => "x86_64-pc-windows-msvc",
        "windows-aarch64" => "aarch64-pc-windows-msvc",
        "linux-x86_64" => "x86_64-unknown-linux-gnu",
        "linux-aarch64" => "aarch64-unknown-linux-gnu",
        "macos-x86_64" => "x86_64-apple-darwin",
        "macos-aarch64" => "aarch64-apple-darwin",
        _ => return None,
    })
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// Facts from `cargo metadata`.
struct Workspace {
    root: PathBuf,
    target_dir: PathBuf,
}

fn workspace(project: &Path, target_dir: Option<&Path>) -> Result<Workspace> {
    if !project.join("Cargo.toml").is_file() {
        // Content-only mods are not Cargo projects.
        return Ok(Workspace {
            root: project.to_path_buf(),
            target_dir: project.join("target"),
        });
    }
    let mut cmd = Command::new(cargo());
    if let Some(dir) = target_dir {
        cmd.env("CARGO_TARGET_DIR", dir);
    }
    let out = cmd
        .current_dir(project)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .context("running cargo metadata")?;
    if !out.status.success() {
        bail!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).context("parsing cargo metadata")?;
    let get = |k: &str| {
        v[k].as_str()
            .map(PathBuf::from)
            .with_context(|| format!("cargo metadata has no {k}"))
    };
    Ok(Workspace {
        root: get("workspace_root")?,
        target_dir: get("target_directory")?,
    })
}

/// Refuses `panic = "abort"` for the profile being built.
pub fn check_panic_setting(
    workspace_toml: &str,
    release: bool,
    env_value: Option<&str>,
) -> Result<()> {
    let profile = if release { "release" } else { "dev" };
    let from_toml = workspace_toml.parse::<toml::Table>().ok().and_then(|t| {
        t.get("profile")?
            .get(profile)?
            .get("panic")?
            .as_str()
            .map(str::to_string)
    });
    let setting = env_value.map(str::to_string).or(from_toml);
    if setting.as_deref() == Some("abort") {
        bail!(
            "panic = \"abort\" is set for the {profile} profile; Fish must catch panics at the FFI \
             boundary, so mods have to be built with panic = \"unwind\""
        );
    }
    Ok(())
}

/// Picks the cdylib that belongs to `project` out of `cargo build --message-format=json` output.
pub fn find_artifact(json_lines: &str, project: &Path) -> Option<PathBuf> {
    let project = project.canonicalize().ok()?;
    json_lines
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["reason"] == "compiler-artifact")
        .filter(|v| {
            v["target"]["crate_types"]
                .as_array()
                .is_some_and(|t| t.iter().any(|c| c == "cdylib"))
        })
        .filter(|v| {
            v["manifest_path"]
                .as_str()
                .and_then(|m| Path::new(m).canonicalize().ok())
                .is_some_and(|m| m.starts_with(&project))
        })
        .flat_map(|v| v["filenames"].as_array().cloned().unwrap_or_default())
        .filter_map(|f| f.as_str().map(PathBuf::from))
        .find(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("dll" | "so" | "dylib")
            )
        })
}

fn compile(
    project: &Path,
    manifest: &Manifest,
    platform: &str,
    opts: &BuildOptions,
) -> Result<PathBuf> {
    let ws = workspace(project, opts.target_dir.as_deref())?;
    let env_name = format!(
        "CARGO_PROFILE_{}_PANIC",
        if opts.release { "RELEASE" } else { "DEV" }
    );
    let env_value = std::env::var(env_name).ok();
    let root_toml = std::fs::read_to_string(ws.root.join("Cargo.toml")).unwrap_or_default();
    check_panic_setting(&root_toml, opts.release, env_value.as_deref())?;

    let mut cmd = Command::new(cargo());
    cmd.current_dir(project)
        .args(["build", "--message-format=json-render-diagnostics"]);
    if opts.release {
        cmd.arg("--release");
    }
    if platform != fish_pack::platform() {
        let triple = triple_for(platform).with_context(|| {
            format!(
                "unknown platform {platform:?} (try windows-x86_64, linux-x86_64, macos-aarch64)"
            )
        })?;
        cmd.args(["--target", triple]);
    }
    let out = cmd
        .stderr(Stdio::inherit())
        .output()
        .context("running cargo build")?;
    if !out.status.success() {
        bail!("cargo build failed");
    }
    let artifact = find_artifact(&String::from_utf8_lossy(&out.stdout), project).context(
        "cargo built no cdylib for this project; add `[lib] crate-type = [\"cdylib\"]` to Cargo.toml",
    )?;
    let lib = manifest
        .library
        .as_ref()
        .expect("caller checked the library");
    let expected = fish_pack::library_file_name(&lib.name, platform);
    let actual = artifact
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if actual != expected {
        bail!(
            "cargo built {actual}, but fish.mod.toml [library] name = {:?} expects {expected}; \
             make the crate's library name match",
            lib.name
        );
    }
    Ok(artifact)
}

/// Builds the mod in `project` and writes `target/fish/<id>-<version>.fish`.
pub fn build(project: &Path, opts: &BuildOptions) -> Result<BuiltMod> {
    let manifest_path = project.join("fish.mod.toml");
    let text = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "reading {} (is this a Fish mod project?)",
            manifest_path.display()
        )
    })?;
    let manifest = fish_metadata::parse(&text)
        .map_err(|e| anyhow::anyhow!("{}: {e}", manifest_path.display()))?;
    let platform = opts
        .target_platform
        .clone()
        .unwrap_or_else(fish_pack::platform);

    let mut spec = PackSpec::new(text);
    if manifest.library.is_some() {
        let artifact = compile(project, &manifest, &platform, opts)?;
        let name = artifact
            .file_name()
            .expect("artifact has a file name")
            .to_string_lossy()
            .into_owned();
        spec.add_file(&format!("bin/{platform}/{name}"), &artifact)?;
    }
    for dir in ["assets", "data"] {
        let path = project.join(dir);
        if path.is_dir() {
            spec.add_dir(dir, &path)?;
        }
    }
    for rel in manifest.icon.iter().chain(manifest.nested.iter()) {
        let file = project.join(rel);
        if !file.is_file() {
            bail!(
                "fish.mod.toml refers to {rel}, but {} does not exist",
                file.display()
            );
        }
        spec.add_file(rel, &file)?;
    }
    let ws = workspace(project, opts.target_dir.as_deref())?;
    let out = ws
        .target_dir
        .join("fish")
        .join(format!("{}-{}.fish", manifest.id, manifest.version));
    fish_pack::write(spec, &out)?;
    Ok(BuiltMod {
        path: out,
        id: manifest.id.to_string(),
        version: manifest.version.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples() {
        assert_eq!(triple_for("windows-x86_64"), Some("x86_64-pc-windows-msvc"));
        assert_eq!(triple_for("macos-aarch64"), Some("aarch64-apple-darwin"));
        assert_eq!(
            triple_for("linux-aarch64"),
            Some("aarch64-unknown-linux-gnu")
        );
        assert_eq!(triple_for("beos-x86"), None);
        for p in ["windows-aarch64", "linux-x86_64", "macos-x86_64"] {
            assert!(triple_for(p).is_some());
        }
    }

    #[test]
    fn panic_abort_is_refused() {
        let abort = "[profile.release]\npanic = \"abort\"\n";
        let err = check_panic_setting(abort, true, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("release profile") && err.contains("panic = \"unwind\""));
        assert!(
            check_panic_setting(abort, false, None).is_ok(),
            "only the built profile matters"
        );
        assert!(check_panic_setting("[profile.dev]\npanic = \"abort\"", false, None).is_err());
        assert!(check_panic_setting("[profile.release]\npanic = \"unwind\"", true, None).is_ok());
        assert!(check_panic_setting("", true, None).is_ok());
        assert!(check_panic_setting("not toml ((", true, None).is_ok());
        // the environment overrides the file
        assert!(check_panic_setting("", true, Some("abort")).is_err());
        assert!(check_panic_setting(abort, true, Some("unwind")).is_ok());
    }

    #[test]
    fn artifact_selection_ignores_other_crates_and_import_libs() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("Cargo.toml"), "").unwrap();
        let manifest = project
            .join("Cargo.toml")
            .display()
            .to_string()
            .replace('\\', "\\\\");
        let other = dir.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("Cargo.toml"), "").unwrap();
        let other_manifest = other
            .join("Cargo.toml")
            .display()
            .to_string()
            .replace('\\', "\\\\");
        let lines = format!(
            "{{\"reason\":\"compiler-message\"}}\nnot json\n\
             {{\"reason\":\"compiler-artifact\",\"manifest_path\":\"{other_manifest}\",\"target\":{{\"crate_types\":[\"cdylib\"]}},\"filenames\":[\"/o/other.dll\"]}}\n\
             {{\"reason\":\"compiler-artifact\",\"manifest_path\":\"{manifest}\",\"target\":{{\"crate_types\":[\"rlib\"]}},\"filenames\":[\"/o/dep.rlib\"]}}\n\
             {{\"reason\":\"compiler-artifact\",\"manifest_path\":\"{manifest}\",\"target\":{{\"crate_types\":[\"cdylib\"]}},\"filenames\":[\"/o/mine.dll.lib\",\"/o/mine.dll\",\"/o/mine.pdb\"]}}\n"
        );
        assert_eq!(
            find_artifact(&lines, &project),
            Some(PathBuf::from("/o/mine.dll"))
        );
        assert_eq!(find_artifact("", &project), None);
    }
}
