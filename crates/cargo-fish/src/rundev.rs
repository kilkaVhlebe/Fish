//! `cargo fish run`: start the Fish Edition directly with the mod under development.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Everything needed to start the edition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    /// The edition binary.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Environment variables to set.
    pub env: Vec<(String, OsString)>,
}

/// The newest installed client edition for this platform.
pub fn newest_edition(home: &Path) -> Option<PathBuf> {
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    fish_launch::edition::discover(home)
        .into_iter()
        .filter(|e| !e.manifest.server && e.manifest.platform == platform)
        .max_by_key(|e| {
            semver::Version::parse(&e.manifest.fish_version)
                .unwrap_or_else(|_| semver::Version::new(0, 0, 0))
        })
        .map(|e| e.binary_path())
}

/// The launcher's data folder (`<platform data dir>/horizon-launcher`).
pub fn launcher_data_dir(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let home = || {
        var("HOME")
            .or_else(|| var("USERPROFILE"))
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        var("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support"))
    } else {
        var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local/share")))
    };
    base.map(|b| b.join("horizon-launcher"))
}

/// The prepared resources of launcher version `tag`: the one folder under `.local/horizon-client`.
pub fn prepared_resources(launcher_data: &Path, tag: &str) -> Option<PathBuf> {
    let base = launcher_data
        .join("versions")
        .join(tag)
        .join(".local/horizon-client");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.pop()
}

/// Options of `cargo fish run`.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// `--resources`.
    pub resources: Option<PathBuf>,
    /// `--edition` (a folder holding the edition, or the binary).
    pub edition: Option<PathBuf>,
    /// `--name`.
    pub name: String,
}

/// Builds the plan; `mod_file` is the freshly built `.fish`.
pub fn plan(
    opts: &RunOptions,
    mod_file: &Path,
    home: &Path,
    launcher_data: Option<&Path>,
    scratch_home: &Path,
) -> Result<RunPlan> {
    let program = match &opts.edition {
        Some(p) if p.is_dir() => {
            let editions = fish_launch::edition::discover(p.parent().unwrap_or(p));
            let manifest = std::fs::read_to_string(p.join("fish-edition.json"))
                .ok()
                .and_then(|t| {
                    serde_json::from_str::<fish_launch::edition::EditionManifest>(&t).ok()
                });
            drop(editions);
            match manifest {
                Some(m) => p.join(m.binary.name),
                None => bail!("{} has no valid fish-edition.json", p.display()),
            }
        }
        Some(p) => p.clone(),
        None => newest_edition(home).with_context(|| {
            format!(
                "no Fish edition is installed in {} (run fish-installer, or pass --edition)",
                home.display()
            )
        })?,
    };
    let resources = match &opts.resources {
        Some(r) => r.clone(),
        None => {
            let state: Option<serde_json::Value> =
                std::fs::read_to_string(home.join("installer.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok());
            let tag = state
                .as_ref()
                .and_then(|s| s["horizon_tag"].as_str())
                .map(str::to_string);
            tag.as_deref()
                .and_then(|t| launcher_data.and_then(|d| prepared_resources(d, t)))
                .context("cannot find prepared game resources: pass --resources <dir>")?
        }
    };
    Ok(RunPlan {
        program,
        args: vec![
            "--resources".into(),
            resources.into_os_string(),
            "--name".into(),
            opts.name.clone().into(),
        ],
        env: vec![
            (
                "FISH_LOADER_ADD_MODS".into(),
                mod_file.as_os_str().to_os_string(),
            ),
            ("FISH_LOADER_DEV".into(), "1".into()),
            ("FISH_LOADER_HOME".into(), home.as_os_str().to_os_string()),
            (
                "HORIZON_HOME".into(),
                scratch_home.as_os_str().to_os_string(),
            ),
        ],
    })
}

/// Starts the plan and waits; returns the game's exit code.
pub fn execute(plan: &RunPlan) -> Result<i32> {
    let status = std::process::Command::new(&plan.program)
        .args(&plan.args)
        .envs(plan.env.iter().map(|(k, v)| (k, v)))
        .status()
        .with_context(|| format!("starting {}", plan.program.display()))?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_edition(home: &Path, version: &str, server: bool) -> PathBuf {
        let id = format!("{version}+horizon.6f8c7a0");
        let dir = home.join("editions").join(&id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bin"), b"x").unwrap();
        let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        std::fs::write(dir.join("fish-edition.json"), format!(r#"{{"schema":1,"id":"{id}","fish_version":"{version}","horizon":{{"commit":"6f8c7a0e5d"}},"platform":"{platform}","server":{server},"binary":{{"name":"bin","sha256":"00"}}}}"#)).unwrap();
        dir
    }

    #[test]
    fn the_newest_client_edition_is_chosen() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(newest_edition(home.path()), None);
        write_edition(home.path(), "0.1.0", false);
        let newer = write_edition(home.path(), "0.2.0", false);
        write_edition(home.path(), "0.9.0", true);
        assert_eq!(newest_edition(home.path()), Some(newer.join("bin")));
    }

    #[test]
    fn resources_come_from_the_flag_or_the_pinned_launcher_version() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("fish");
        write_edition(&home, "0.1.0", false);
        std::fs::write(
            home.join("installer.json"),
            r#"{"horizon_tag":"nightly-2026-10-28-6f8c7a0"}"#,
        )
        .unwrap();
        let data = tmp.path().join("launcher");
        let res = data.join("versions/nightly-2026-10-28-6f8c7a0/.local/horizon-client/26.3");
        std::fs::create_dir_all(&res).unwrap();
        let mod_file = tmp.path().join("m.fish");
        let opts = RunOptions {
            name: "Dev".into(),
            ..RunOptions::default()
        };

        let p = plan(
            &opts,
            &mod_file,
            &home,
            Some(&data),
            &tmp.path().join("scratch"),
        )
        .unwrap();
        assert_eq!(p.args[0], "--resources");
        assert_eq!(PathBuf::from(&p.args[1]), res);
        assert_eq!(p.args[2..], ["--name", "Dev"]);
        let env = |k: &str| p.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(
            env("FISH_LOADER_ADD_MODS"),
            Some(mod_file.clone().into_os_string())
        );
        assert_eq!(env("FISH_LOADER_DEV"), Some("1".into()));
        assert_eq!(
            env("HORIZON_HOME"),
            Some(tmp.path().join("scratch").into_os_string())
        );
        assert!(p.program.ends_with("bin"));

        let explicit = RunOptions {
            resources: Some("R".into()),
            name: "X".into(),
            ..RunOptions::default()
        };
        assert_eq!(
            plan(&explicit, &mod_file, &home, None, tmp.path())
                .unwrap()
                .args[1],
            "R"
        );
        let err = plan(&opts, &mod_file, &home, None, tmp.path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("--resources"), "{err}");
        let err = plan(
            &explicit,
            &mod_file,
            &tmp.path().join("empty"),
            None,
            tmp.path(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("no Fish edition is installed"), "{err}");
    }

    #[test]
    fn explicit_editions_may_be_folders_or_binaries() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_edition(tmp.path(), "0.1.0", false);
        let opts = |p: PathBuf| RunOptions {
            edition: Some(p),
            resources: Some("R".into()),
            name: "D".into(),
        };
        let m = tmp.path().join("m.fish");
        assert_eq!(
            plan(&opts(dir.clone()), &m, tmp.path(), None, tmp.path())
                .unwrap()
                .program,
            dir.join("bin")
        );
        let file = dir.join("bin");
        assert_eq!(
            plan(&opts(file.clone()), &m, tmp.path(), None, tmp.path())
                .unwrap()
                .program,
            file
        );
        let empty = tmp.path().join("emptydir");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(
            plan(&opts(empty), &m, tmp.path(), None, tmp.path())
                .unwrap_err()
                .to_string()
                .contains("fish-edition.json")
        );
    }

    #[test]
    fn data_dirs_and_prepared_folders() {
        let v = |pairs: &'static [(&'static str, &'static str)]| {
            move |n: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == n)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        let d = launcher_data_dir(v(&[
            ("APPDATA", "/a"),
            ("HOME", "/h"),
            ("XDG_DATA_HOME", "/x"),
        ]))
        .unwrap();
        assert!(d.ends_with("horizon-launcher"));
        assert_eq!(launcher_data_dir(|_| None), None);
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(prepared_resources(tmp.path(), "t"), None);
        let base = tmp.path().join("versions/t/.local/horizon-client");
        std::fs::create_dir_all(base.join("26.2")).unwrap();
        std::fs::create_dir_all(base.join("26.3")).unwrap();
        std::fs::write(base.join("junk.txt"), b"x").unwrap();
        assert_eq!(
            prepared_resources(tmp.path(), "t"),
            Some(base.join("26.3")),
            "the newest folder"
        );
    }
}
