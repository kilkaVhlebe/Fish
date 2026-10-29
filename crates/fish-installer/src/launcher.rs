//! Talking to `horizon-launcher` through its public command line, and nothing else.
//!
//! Fish never writes launcher-owned files (`instances.json`, `settings.json`, `versions/`,
//! `accounts.json`, `native/`). Every change goes through these commands.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::error::{Error, Result};

/// One instance as `instance list --json` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InstanceInfo {
    /// The launcher's id of the instance.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its game folder.
    pub home: PathBuf,
    /// The Horizon version it is pinned to, if any.
    #[serde(default)]
    pub version: Option<String>,
}

/// What the installer needs from the launcher.
pub trait LauncherCli {
    /// Installed version tags (`versions --installed --json`).
    fn versions_installed(&self) -> Result<Vec<String>>;
    /// Downloads and installs a release (`install <tag>`).
    fn install(&self, tag: &str) -> Result<()>;
    /// All instances (`instance list --json`).
    fn instance_list(&self) -> Result<Vec<InstanceInfo>>;
    /// Creates an instance (`instance create <name>`).
    fn instance_create(&self, name: &str) -> Result<()>;
    /// Pins an instance to an installed version (`instance version <name> <tag>`).
    fn instance_version(&self, name: &str, tag: &str) -> Result<()>;
    /// The instance's wrapper (`instance launch <name> --json`).
    fn instance_wrapper(&self, name: &str) -> Result<String>;
    /// Sets the instance's wrapper; an empty string clears it (`instance launch <name> --wrapper …`).
    fn instance_set_wrapper(&self, name: &str, wrapper: &str) -> Result<()>;
    /// Makes Play start the instance (`instance select <name>`).
    fn instance_select(&self, name: &str) -> Result<()>;
}

/// The real thing: runs the `horizon-launcher` executable.
#[derive(Debug, Clone)]
pub struct ProcessLauncherCli {
    exe: PathBuf,
}

impl ProcessLauncherCli {
    /// Uses the launcher at `exe`.
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        ProcessLauncherCli { exe: exe.into() }
    }

    /// The executable.
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let command = args.join(" ");
        let out = Command::new(&self.exe)
            .args(args)
            .output()
            .map_err(|e| Error::Launcher {
                command: command.clone(),
                message: e.to_string(),
            })?;
        if !out.status.success() {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stderr),
                String::from_utf8_lossy(&out.stdout)
            );
            return Err(Error::Launcher {
                command,
                message: format!("exit status {}: {}", out.status, text.trim()),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn json(&self, args: &[&str]) -> Result<serde_json::Value> {
        let text = self.run(args)?;
        serde_json::from_str(&text).map_err(|e| Error::LauncherOutput {
            command: args.join(" "),
            message: format!("not JSON ({e})"),
        })
    }
}

impl LauncherCli for ProcessLauncherCli {
    fn versions_installed(&self) -> Result<Vec<String>> {
        let args = ["versions", "--installed", "--json"];
        let v = self.json(&args)?;
        v["installed"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .collect()
            })
            .ok_or_else(|| Error::LauncherOutput {
                command: args.join(" "),
                message: "no \"installed\" list".into(),
            })
    }

    fn install(&self, tag: &str) -> Result<()> {
        self.run(&["install", tag]).map(|_| ())
    }

    fn instance_list(&self) -> Result<Vec<InstanceInfo>> {
        let args = ["instance", "list", "--json"];
        let v = self.json(&args)?;
        serde_json::from_value(v).map_err(|e| Error::LauncherOutput {
            command: args.join(" "),
            message: e.to_string(),
        })
    }

    fn instance_create(&self, name: &str) -> Result<()> {
        self.run(&["instance", "create", name]).map(|_| ())
    }

    fn instance_version(&self, name: &str, tag: &str) -> Result<()> {
        self.run(&["instance", "version", name, tag]).map(|_| ())
    }

    fn instance_wrapper(&self, name: &str) -> Result<String> {
        let args = ["instance", "launch", name, "--json"];
        let v = self.json(&args)?;
        v["wrapper"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::LauncherOutput {
                command: args.join(" "),
                message: "no \"wrapper\" field".into(),
            })
    }

    fn instance_set_wrapper(&self, name: &str, wrapper: &str) -> Result<()> {
        self.run(&["instance", "launch", name, "--wrapper", wrapper])
            .map(|_| ())
    }

    fn instance_select(&self, name: &str) -> Result<()> {
        self.run(&["instance", "select", name]).map(|_| ())
    }
}

/// True if `exe` answers `versions --installed --json`.
pub fn is_working_launcher(exe: &Path) -> bool {
    ProcessLauncherCli::new(exe).versions_installed().is_ok()
}

/// Where launcher discovery looks, in order, besides the `--launcher` flag.
pub struct Probes {
    /// Value of `FISH_LOADER_HORIZON_LAUNCHER`.
    pub env: Option<OsString>,
    /// Directories of `PATH`.
    pub path: Vec<PathBuf>,
    /// Executable paths of running `horizon-launcher` processes.
    pub running: Vec<PathBuf>,
    /// The per-user install location.
    pub default_location: Option<PathBuf>,
}

impl Probes {
    /// Probes of this machine.
    pub fn here() -> Probes {
        let running = match sysinfo::System::new() {
            Ok(mut sys) => {
                sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
                sys.processes()
                    .values()
                    .filter(|p| {
                        p.name()
                            .to_string_lossy()
                            .to_lowercase()
                            .starts_with("horizon-launcher")
                    })
                    .filter_map(|p| p.exe().map(Path::to_path_buf))
                    .collect()
            }
            Err(_) => Vec::new(),
        };
        let default_location = std::env::var_os("LOCALAPPDATA").map(|d| {
            PathBuf::from(d)
                .join("Programs/horizon-launcher")
                .join(exe_name())
        });
        Probes {
            env: std::env::var_os("FISH_LOADER_HORIZON_LAUNCHER"),
            path: std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default(),
            running,
            default_location,
        }
    }
}

/// The launcher executable's file name on this platform.
pub fn exe_name() -> &'static str {
    if cfg!(windows) {
        "horizon-launcher.exe"
    } else {
        "horizon-launcher"
    }
}

/// Finds a working launcher: the flag, the environment, `PATH`, a running process, the default
/// location. `works` decides validity (the real check runs the launcher).
pub fn discover(
    flag: Option<&Path>,
    probes: &Probes,
    works: &dyn Fn(&Path) -> bool,
) -> Result<PathBuf> {
    let mut tried: Vec<String> = Vec::new();
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.extend(flag.map(Path::to_path_buf));
    candidates.extend(probes.env.iter().map(PathBuf::from));
    candidates.extend(probes.path.iter().map(|d| d.join(exe_name())));
    candidates.extend(probes.running.iter().cloned());
    candidates.extend(probes.default_location.iter().cloned());
    for c in candidates {
        if c.is_file() && works(&c) {
            return Ok(c);
        }
        tried.push(c.display().to_string());
    }
    Err(Error::LauncherNotFound(if tried.is_empty() {
        "nothing to try".into()
    } else {
        format!("tried {}", tried.join(", "))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_follows_the_documented_order_and_skips_broken_candidates() {
        let dir =
            std::env::temp_dir().join(format!("fish-installer-discover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let make = |name: &str| {
            let p = dir.join(name);
            std::fs::write(&p, b"x").unwrap();
            p
        };
        let (flag, envp, running, default) = (make("flag"), make("env"), make("run"), make("def"));
        let path_dir = dir.join("bin");
        std::fs::create_dir_all(&path_dir).unwrap();
        std::fs::write(path_dir.join(exe_name()), b"x").unwrap();
        let probes = Probes {
            env: Some(envp.clone().into_os_string()),
            path: vec![path_dir.clone()],
            running: vec![running.clone()],
            default_location: Some(default.clone()),
        };
        assert_eq!(discover(Some(&flag), &probes, &|_| true).unwrap(), flag);
        assert_eq!(discover(None, &probes, &|_| true).unwrap(), envp);
        let only = |want: PathBuf| move |p: &Path| p == want;
        assert_eq!(
            discover(Some(&flag), &probes, &only(path_dir.join(exe_name()))).unwrap(),
            path_dir.join(exe_name())
        );
        assert_eq!(
            discover(None, &probes, &only(running.clone())).unwrap(),
            running
        );
        assert_eq!(
            discover(None, &probes, &only(default.clone())).unwrap(),
            default
        );
        let err = discover(None, &probes, &|_| false).unwrap_err().to_string();
        assert!(
            err.contains("no working horizon-launcher") && err.contains("--launcher"),
            "{err}"
        );
        let nothing = Probes {
            env: None,
            path: vec![],
            running: vec![],
            default_location: None,
        };
        assert!(
            discover(None, &nothing, &|_| true)
                .unwrap_err()
                .to_string()
                .contains("nothing to try")
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
