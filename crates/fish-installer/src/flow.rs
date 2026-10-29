//! The installer's flows: install, repair, update, uninstall, status.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::launcher::{InstanceInfo, LauncherCli};
use crate::source::{self, Fetcher};

/// Receives progress lines.
pub trait Reporter {
    /// A step is starting or finished.
    fn step(&self, message: &str);
}

/// Reporter that prints to stderr.
pub struct StderrReporter;

impl Reporter for StderrReporter {
    fn step(&self, message: &str) {
        eprintln!("{message}");
    }
}

/// What the installer remembers (`<home>/installer.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Always 1.
    pub schema: u32,
    /// The launcher executable used.
    pub launcher: String,
    /// The instance Fish was set up in.
    pub instance: String,
    /// Installed edition id.
    pub edition: String,
    /// The launcher version tag the instance is pinned to.
    pub horizon_tag: String,
    /// Seconds since 1970.
    pub installed_at: u64,
}

/// Reads `installer.json`.
pub fn read_state(home: &Path) -> Option<State> {
    serde_json::from_str(&std::fs::read_to_string(home.join("installer.json")).ok()?).ok()
}

fn write_state(home: &Path, state: &State) -> Result<()> {
    std::fs::create_dir_all(home)
        .map_err(|e| Error::io(format!("creating {}", home.display()), e))?;
    let path = home.join("installer.json");
    let text = serde_json::to_string_pretty(state).expect("state serialises");
    std::fs::write(&path, text).map_err(|e| Error::io(format!("writing {}", path.display()), e))
}

/// Which flow runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// First install (or re-install of the newest edition).
    Install,
    /// Reinstall the edition that is recorded, repairing everything around it.
    Repair,
    /// Install the newest edition and re-pin the instance.
    Update,
}

/// Settings of a run.
#[derive(Debug, Clone)]
pub struct Options {
    /// Fish home.
    pub home: PathBuf,
    /// Instance name.
    pub instance: String,
    /// Make Play start the instance.
    pub select: bool,
    /// Replace a foreign wrapper.
    pub force: bool,
    /// Accept an unsigned index.
    pub allow_unsigned: bool,
    /// Platform key editions must have.
    pub platform: String,
    /// A local edition zip instead of the release index.
    pub edition_zip: Option<PathBuf>,
    /// Install exactly this edition id from the index (default: the newest).
    pub edition_id: Option<String>,
    /// Base URL override (`FISH_LOADER_RELEASES_URL`).
    pub releases_url: Option<String>,
    /// Launcher executable recorded in the state.
    pub launcher_exe: String,
}

/// The `fish-launch` executable to install.
pub enum LaunchBinary {
    /// Bytes bundled into the installer.
    Bytes(Vec<u8>),
    /// A file (next to the installer).
    File(PathBuf),
}

impl LaunchBinary {
    fn bytes(&self) -> Result<Vec<u8>> {
        match self {
            LaunchBinary::Bytes(b) => Ok(b.clone()),
            LaunchBinary::File(p) => std::fs::read(p).map_err(|e| {
                Error::io(
                    format!(
                        "reading fish-launch from {} (build it with cargo build -p fish-launch)",
                        p.display()
                    ),
                    e,
                )
            }),
        }
    }
}

/// Everything a run needs.
pub struct Installer<'a> {
    /// The launcher.
    pub launcher: &'a dyn LauncherCli,
    /// Network access.
    pub fetcher: &'a dyn Fetcher,
    /// Progress output.
    pub reporter: &'a dyn Reporter,
    /// Settings.
    pub options: Options,
    /// The wrapper executable.
    pub launch: LaunchBinary,
    /// The installer executable to copy into `bin/`.
    pub self_exe: Option<PathBuf>,
    /// Release public key (empty: none).
    pub key: &'a str,
    /// `owner/repo` of the default release source.
    pub github: &'a str,
}

/// The executable name of fish-launch.
pub fn launch_exe_name() -> &'static str {
    if cfg!(windows) {
        "fish-launch.exe"
    } else {
        "fish-launch"
    }
}

/// The wrapper string for an instance: the path in double quotes.
pub fn quote_wrapper(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('"', "\\\""))
}

/// True if `wrapper` runs a `fish-launch` executable.
pub fn is_fish_wrapper(wrapper: &str) -> bool {
    let first = wrapper.trim().trim_start_matches('"');
    let first = first.split('"').next().unwrap_or("");
    let stem = Path::new(&first.replace('\\', "/"))
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned());
    stem.as_deref() == Some("fish-launch")
}

fn same_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Installer<'_> {
    fn say(&self, m: &str) {
        self.reporter.step(m);
    }

    fn edition_bytes(&self, kind: Kind) -> Result<(Vec<u8>, String)> {
        if let Some(zip) = &self.options.edition_zip {
            self.say(&format!("Reading edition {}", zip.display()));
            let bytes = std::fs::read(zip)
                .map_err(|e| Error::io(format!("reading {}", zip.display()), e))?;
            return Ok((bytes, String::new()));
        }
        let base = source::releases_base(self.options.releases_url.as_deref(), self.github)?;
        self.say(&format!("Fetching the edition index from {base}"));
        let index = source::load_index(self.fetcher, &base, self.key, self.options.allow_unsigned)?;
        let want = self.options.edition_id.clone().or_else(|| match kind {
            Kind::Repair => read_state(&self.options.home).map(|s| s.edition),
            _ => None,
        });
        let entry = source::select_entry(&index, &self.options.platform, want.as_deref())
            .ok_or_else(|| {
                Error::NoEdition(format!(
                    "the release has no client edition for {}{}",
                    self.options.platform,
                    want.map(|w| format!(" with id {w}")).unwrap_or_default()
                ))
            })?;
        self.say(&format!("Downloading {} ({} bytes)", entry.id, entry.size));
        Ok((
            source::download_edition(self.fetcher, entry)?,
            entry.horizon_release_tag.clone(),
        ))
    }

    fn install_binaries(&self) -> Result<()> {
        let bin = self.options.home.join("bin");
        std::fs::create_dir_all(&bin)
            .map_err(|e| Error::io(format!("creating {}", bin.display()), e))?;
        let launch = bin.join(launch_exe_name());
        std::fs::write(&launch, self.launch.bytes()?).map_err(|e| {
            Error::io(
                format!(
                    "writing {} (is the game running? close it and retry)",
                    launch.display()
                ),
                e,
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&launch, std::fs::Permissions::from_mode(0o755));
        }
        if let Some(me) = &self.self_exe {
            let name = me
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_else(|| "fish-installer".into());
            let dest = bin.join(name);
            if dest != *me {
                std::fs::copy(me, &dest).map_err(|e| {
                    Error::io(format!("copying the installer to {}", dest.display()), e)
                })?;
            }
        }
        Ok(())
    }

    fn horizon_tag(
        &self,
        manifest: &fish_launch::edition::EditionManifest,
        release_tag: &str,
    ) -> Result<String> {
        let short = &manifest.horizon.commit[..manifest.horizon.commit.len().min(7)];
        let find = |tags: Vec<String>| tags.into_iter().find(|t| t.contains(short));
        if let Some(t) = find(self.launcher.versions_installed()?) {
            return Ok(t);
        }
        let release = if !manifest.horizon.release_tag.is_empty() {
            manifest.horizon.release_tag.clone()
        } else {
            release_tag.to_string()
        };
        if release.is_empty() {
            return Err(Error::NoEdition(format!(
                "Horizon {short} is not installed in the launcher and the edition does not say which release to install; install that nightly in the launcher first"
            )));
        }
        self.say(&format!("Installing Horizon {release} in the launcher"));
        self.launcher.install(&release)?;
        find(self.launcher.versions_installed()?).ok_or_else(|| {
            Error::NoEdition(format!(
                "after installing {release}, the launcher still has no version for commit {short}"
            ))
        })
    }

    fn instance(&self) -> Result<InstanceInfo> {
        let name = &self.options.instance;
        let find = |all: Vec<InstanceInfo>| all.into_iter().find(|i| same_name(&i.name, name));
        if let Some(existing) = find(self.launcher.instance_list()?) {
            let wrapper = self.launcher.instance_wrapper(name)?;
            if !wrapper.trim().is_empty() && !is_fish_wrapper(&wrapper) && !self.options.force {
                return Err(Error::ForeignInstance {
                    instance: name.clone(),
                    wrapper,
                });
            }
            return Ok(existing);
        }
        self.say(&format!("Creating instance {name}"));
        self.launcher.instance_create(name)?;
        find(self.launcher.instance_list()?).ok_or_else(|| Error::LauncherOutput {
            command: "instance list --json".into(),
            message: format!("instance {name} is missing right after creating it"),
        })
    }

    /// Runs install, repair or update.
    pub fn run(&self, kind: Kind) -> Result<State> {
        let (zip, release_tag) = self.edition_bytes(kind)?;
        self.say("Unpacking the edition");
        let (_, manifest) = source::unpack_edition(&zip, &self.options.home)?;
        self.say("Installing fish-launch");
        self.install_binaries()?;
        let tag = self.horizon_tag(&manifest, &release_tag)?;
        let instance = self.instance()?;
        self.say(&format!("Pinning {} to Horizon {tag}", instance.name));
        self.launcher.instance_version(&instance.name, &tag)?;
        let wrapper = quote_wrapper(&self.options.home.join("bin").join(launch_exe_name()));
        self.launcher
            .instance_set_wrapper(&instance.name, &wrapper)?;
        if self.options.select {
            self.launcher.instance_select(&instance.name)?;
        }
        let mods = instance.home.join("fish-mods");
        std::fs::create_dir_all(&mods)
            .map_err(|e| Error::io(format!("creating {}", mods.display()), e))?;
        let state = State {
            schema: 1,
            launcher: self.options.launcher_exe.clone(),
            instance: instance.name.clone(),
            edition: manifest.id,
            horizon_tag: tag,
            installed_at: now(),
        };
        write_state(&self.options.home, &state)?;
        self.say(&format!(
            "Done: put .fish mods into {} and press Play on \"{}\"",
            mods.display(),
            instance.name
        ));
        Ok(state)
    }

    /// Clears the wrapper and removes Fish's editions and binaries; keeps the instance, worlds and mods.
    pub fn uninstall(&self) -> Result<()> {
        let state = read_state(&self.options.home).ok_or_else(|| {
            Error::NotInstalled(format!(
                "Fish is not installed ({} has no installer.json)",
                self.options.home.display()
            ))
        })?;
        match self.launcher.instance_set_wrapper(&state.instance, "") {
            Ok(()) => self.say(&format!("Cleared the launch wrapper of \"{}\"", state.instance)),
            Err(e) => self.say(&format!(
                "The launcher refused to clear the wrapper ({e}). In the launcher open the instance's Launch… settings and clear the wrapper by hand."
            )),
        }
        let editions = self.options.home.join("editions");
        if editions.exists() {
            std::fs::remove_dir_all(&editions)
                .map_err(|e| Error::io(format!("removing {}", editions.display()), e))?;
        }
        let bin = self.options.home.join("bin");
        let me = std::env::current_exe().ok();
        if let Ok(read) = std::fs::read_dir(&bin) {
            for entry in read.filter_map(|e| e.ok()) {
                if me.as_ref().is_some_and(|m| m == &entry.path()) {
                    continue;
                }
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let _ = std::fs::remove_dir(&bin);
        let _ = std::fs::remove_file(self.options.home.join("installer.json"));
        self.say(&format!(
            "Fish is uninstalled. The instance \"{}\", its worlds and its fish-mods folder were kept.",
            state.instance
        ));
        Ok(())
    }
}

/// A snapshot for `fish-installer status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The recorded installation, if any.
    pub state: Option<State>,
    /// Ids of editions on disk.
    pub editions: Vec<String>,
    /// Whether `bin/fish-launch` exists.
    pub launch_present: bool,
    /// The instance's current wrapper, if it could be read.
    pub wrapper: Option<String>,
}

/// Collects the status.
pub fn status(home: &Path, launcher: Option<&dyn LauncherCli>) -> Status {
    let state = read_state(home);
    let editions = fish_launch::edition::discover(home)
        .into_iter()
        .map(|e| e.manifest.id)
        .collect();
    let wrapper = state
        .as_ref()
        .and_then(|s| launcher.and_then(|l| l.instance_wrapper(&s.instance).ok()));
    Status {
        state,
        editions,
        launch_present: home.join("bin").join(launch_exe_name()).is_file(),
        wrapper,
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.state {
            None => writeln!(f, "Fish is not installed.")?,
            Some(s) => {
                writeln!(
                    f,
                    "Fish edition {} for Horizon {}",
                    s.edition, s.horizon_tag
                )?;
                writeln!(f, "Instance: {}", s.instance)?;
                writeln!(f, "Launcher: {}", s.launcher)?;
            }
        }
        writeln!(
            f,
            "Editions on disk: {}",
            if self.editions.is_empty() {
                "none".into()
            } else {
                self.editions.join(", ")
            }
        )?;
        writeln!(
            f,
            "fish-launch: {}",
            if self.launch_present {
                "installed"
            } else {
                "missing"
            }
        )?;
        match &self.wrapper {
            Some(w) if is_fish_wrapper(w) => writeln!(f, "Instance wrapper: {w} (ok)"),
            Some(w) if w.trim().is_empty() => {
                writeln!(f, "Instance wrapper: (none) - run fish-installer repair")
            }
            Some(w) => writeln!(f, "Instance wrapper: {w} (not Fish)"),
            None => writeln!(f, "Instance wrapper: unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_recognition_and_quoting() {
        assert!(is_fish_wrapper(
            r#""C:\Users\A B\AppData\Roaming\fish-loader\bin\fish-launch.exe""#
        ));
        assert!(is_fish_wrapper(
            "/home/a/.local/share/fish-loader/bin/fish-launch"
        ));
        assert!(is_fish_wrapper("  \"/x y/fish-launch\"  "));
        assert!(!is_fish_wrapper(""));
        assert!(!is_fish_wrapper("nice -n 5"));
        assert!(!is_fish_wrapper("\"C:\\tools\\other.exe\""));
        assert!(
            !is_fish_wrapper("mangohud fish-launch.exe"),
            "only the first word is the wrapper program"
        );
        let q = quote_wrapper(Path::new(r"C:\Users\A B\fish-loader\bin\fish-launch.exe"));
        assert_eq!(q, r#""C:\Users\A B\fish-loader\bin\fish-launch.exe""#);
        assert!(is_fish_wrapper(&q));
    }

    #[test]
    fn status_text() {
        let s = Status {
            state: None,
            editions: vec![],
            launch_present: false,
            wrapper: None,
        };
        let text = s.to_string();
        assert!(
            text.contains("not installed")
                && text.contains("none")
                && text.contains("missing")
                && text.contains("unknown")
        );
        let st = State {
            schema: 1,
            launcher: "l".into(),
            instance: "Fish".into(),
            edition: "0.1.0+horizon.x".into(),
            horizon_tag: "nightly-1".into(),
            installed_at: 0,
        };
        let with = |w: Option<&str>| {
            Status {
                state: Some(st.clone()),
                editions: vec!["a".into(), "b".into()],
                launch_present: true,
                wrapper: w.map(str::to_string),
            }
            .to_string()
        };
        assert!(with(Some("\"/x/fish-launch\"")).contains("(ok)"));
        assert!(with(Some("")).contains("run fish-installer repair"));
        assert!(with(Some("nice")).contains("(not Fish)"));
        assert!(with(None).contains("Instance: Fish") && with(None).contains("a, b"));
    }
}
