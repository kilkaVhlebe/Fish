//! The wrapper's logic: pick an edition for the game the launcher meant to start, and start it.

use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::edition::{self, Edition, VerifyError};

/// Why the game was not started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchError(pub String);

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LaunchError {}

/// What to do for a command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// No game was named: print this and exit 0.
    Info(String),
    /// Start this program.
    Launch(Launch),
}

/// A game start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The edition's binary.
    pub program: PathBuf,
    /// The game's arguments, exactly as the launcher passed them.
    pub args: Vec<OsString>,
    /// Id of the chosen edition.
    pub edition_id: String,
    /// The launcher version tag the launcher meant to play.
    pub tag: String,
}

/// The platform key of this build.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// What `fish-launch` with no arguments prints.
pub fn info(home: &Path, platform: &str) -> String {
    let mut out = format!(
        "fish-launch {}\nFish home: {}\nPlatform: {platform}\nInstalled editions:\n",
        env!("CARGO_PKG_VERSION"),
        home.display()
    );
    let editions = edition::discover(home);
    if editions.is_empty() {
        out.push_str("  (none: run fish-installer)\n");
    }
    for e in editions {
        let short = &e.manifest.horizon.commit[..e.manifest.horizon.commit.len().min(7)];
        out.push_str(&format!(
            "  {} (Fish {}, Horizon {short}, {}{})\n",
            e.manifest.id,
            e.manifest.fish_version,
            e.manifest.platform,
            if e.manifest.server { ", server" } else { "" }
        ));
    }
    out
}

/// The Horizon tag Fish was installed for (`installer.json`), if known.
fn pinned_tag(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(home.join("installer.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["horizon_tag"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Chooses what to do for `args` (`[fish-launch, <game program>, <game args…>]`).
pub fn plan(args: &[OsString], home: &Path, platform: &str) -> Result<Plan, LaunchError> {
    let Some(program) = args.get(1) else {
        return Ok(Plan::Info(info(home, platform)));
    };
    let program_path = PathBuf::from(program);
    let tag = edition::version_tag(&program_path).ok_or_else(|| {
        LaunchError(format!(
            "Fish cannot tell which Horizon version {} belongs to (it is not below a launcher \"versions\" folder).",
            program_path.display()
        ))
    })?;
    let editions = edition::discover(home);
    let Some(chosen) = edition::select(&editions, &tag, platform) else {
        let hint = match pinned_tag(home) {
            Some(t) => format!("set the instance's version back to {t}"),
            None => "set the instance's version back to the one Fish was installed for".to_string(),
        };
        return Err(LaunchError(format!(
            "Fish has no build for Horizon {tag}. Run \"fish-installer repair\" or {hint}."
        )));
    };
    check(chosen)?;
    Ok(Plan::Launch(Launch {
        program: chosen.binary_path(),
        args: args[2..].to_vec(),
        edition_id: chosen.manifest.id.clone(),
        tag,
    }))
}

fn check(e: &Edition) -> Result<(), LaunchError> {
    match edition::verify(e) {
        Ok(()) => Ok(()),
        Err(VerifyError::ChecksumMismatch) => Err(LaunchError(format!(
            "Fish edition {} is damaged (checksum mismatch). Run \"fish-installer repair\".",
            e.manifest.id
        ))),
        Err(VerifyError::Missing) => Err(LaunchError(format!(
            "Fish edition {} is damaged (its binary is missing). Run \"fish-installer repair\".",
            e.manifest.id
        ))),
    }
}

/// Appends a line to `<home>/logs/fish-launch.log` (best effort).
pub fn log_line(home: &Path, message: &str) {
    let dir = home.join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("fish-launch.log"))
    {
        let _ = writeln!(f, "[{secs}] {message}");
    }
}

fn command(launch: &Launch, home: &Path) -> Command {
    let mut cmd = Command::new(&launch.program);
    cmd.args(&launch.args).env("FISH_LOADER_HOME", home);
    cmd
}

#[cfg(unix)]
fn execute(launch: &Launch, home: &Path) -> Result<i32, LaunchError> {
    use std::os::unix::process::CommandExt;
    let err = command(launch, home).exec();
    Err(LaunchError(format!(
        "cannot start {}: {err}",
        launch.program.display()
    )))
}

#[cfg(windows)]
fn execute(launch: &Launch, home: &Path) -> Result<i32, LaunchError> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    let mut child = command(launch, home)
        .spawn()
        .map_err(|e| LaunchError(format!("cannot start {}: {e}", launch.program.display())))?;
    // The game lives and dies with this wrapper, so Stop in the launcher reaches it.
    // SAFETY: plain Win32 calls with valid arguments; the job handle is closed below.
    let job: HANDLE = unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if !job.is_null() {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            let assigned = AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE);
            if set == 0 || assigned == 0 {
                log_line(
                    home,
                    "could not tie the game to a job object; Stop may leave it running",
                );
            }
        }
        job
    };
    let status = child
        .wait()
        .map_err(|e| LaunchError(format!("waiting for the game failed: {e}")))?;
    if !job.is_null() {
        // SAFETY: `job` came from CreateJobObjectW and is closed once.
        unsafe { CloseHandle(job) };
    }
    Ok(status.code().unwrap_or(1))
}

/// Runs the wrapper; returns the process exit code (on Unix a successful start never returns).
pub fn main_with(args: &[OsString], home: &Path) -> i32 {
    match plan(args, home, &platform()) {
        Ok(Plan::Info(text)) => {
            print!("{text}");
            0
        }
        Ok(Plan::Launch(launch)) => {
            log_line(
                home,
                &format!("starting edition {} for {}", launch.edition_id, launch.tag),
            );
            match execute(&launch, home) {
                Ok(code) => {
                    log_line(home, &format!("the game exited with {code}"));
                    code
                }
                Err(e) => fail(home, &e),
            }
        }
        Err(e) => fail(home, &e),
    }
}

fn fail(home: &Path, e: &LaunchError) -> i32 {
    eprintln!("{e}");
    log_line(home, &e.0);
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_arguments_prints_info() {
        let home = std::env::temp_dir().join(format!("fish-launch-info-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let Plan::Info(text) = plan(&args(&["fish-launch"]), &home, "windows-x86_64").unwrap()
        else {
            panic!("expected info")
        };
        assert!(
            text.contains("Fish home:") && text.contains("(none: run fish-installer)"),
            "{text}"
        );
    }

    #[test]
    fn unknown_locations_and_missing_editions_give_actionable_errors() {
        let home = std::env::temp_dir().join(format!("fish-launch-err-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let e = plan(
            &args(&["fish-launch", "/opt/horizon-client"]),
            &home,
            "windows-x86_64",
        )
        .unwrap_err();
        assert!(e.0.contains("cannot tell which Horizon version"), "{e}");

        let game = "/x/versions/nightly-2026-10-28-6f8c7a0/horizon-client";
        let e = plan(&args(&["fish-launch", game]), &home, "windows-x86_64").unwrap_err();
        assert_eq!(
            e.0,
            "Fish has no build for Horizon nightly-2026-10-28-6f8c7a0. Run \"fish-installer repair\" or set the instance's version back to the one Fish was installed for."
        );
        std::fs::write(
            home.join("installer.json"),
            r#"{"horizon_tag":"nightly-2026-10-27-aaaaaaa"}"#,
        )
        .unwrap();
        let e = plan(&args(&["fish-launch", game]), &home, "windows-x86_64").unwrap_err();
        assert!(
            e.0.ends_with("set the instance's version back to nightly-2026-10-27-aaaaaaa."),
            "{e}"
        );
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn log_lines_are_appended() {
        let home = std::env::temp_dir().join(format!("fish-launch-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        log_line(&home, "first");
        log_line(&home, "second");
        let text = std::fs::read_to_string(home.join("logs/fish-launch.log")).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("first") && text.contains("second"));
        std::fs::remove_dir_all(home).ok();
    }
}
