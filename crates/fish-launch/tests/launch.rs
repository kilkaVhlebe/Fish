//! The wrapper started as a real process, with a fake game as the edition.

use std::path::{Path, PathBuf};
use std::process::Command;

use fish_test_host::build_tool;
use sha2::{Digest, Sha256};

const WRAPPER: &str = env!("CARGO_BIN_EXE_fish-launch");

/// Installs the fake game as an edition for Horizon commit `6f8c7a0…`; returns Fish home.
fn install_edition(dir: &Path, sha_override: Option<&str>) -> PathBuf {
    let home = dir.join("fish-home");
    let edition = home.join("editions/0.1.0+horizon.6f8c7a0");
    std::fs::create_dir_all(&edition).unwrap();
    let game = build_tool("fish-test-tools", "fake-game");
    let name = game.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::copy(&game, edition.join(&name)).unwrap();
    let sha: String = Sha256::digest(std::fs::read(&game).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let sha = sha_override.map_or(sha, str::to_string);
    std::fs::write(
        edition.join("fish-edition.json"),
        format!(
            r#"{{"schema":1,"id":"0.1.0+horizon.6f8c7a0","fish_version":"0.1.0","horizon":{{"commit":"6f8c7a0e5d531d3834c4d0be4659a1db8e7e5c2a","release_tag":""}},"platform":"{}-{}","server":false,"binary":{{"name":"{name}","sha256":"{sha}"}}}}"#,
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    )
    .unwrap();
    home
}

fn vanilla(dir: &Path, tag: &str) -> PathBuf {
    let p = dir
        .join("launcher/versions")
        .join(tag)
        .join("horizon-client.exe");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    p
}

fn wrapper(home: &Path) -> Command {
    let mut c = Command::new(WRAPPER);
    c.env("FISH_LOADER_HOME", home);
    c
}

#[test]
fn starts_the_matching_edition_with_the_same_arguments_and_propagates_the_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let home = install_edition(dir.path(), None);
    let game = vanilla(dir.path(), "nightly-2026-10-28-6f8c7a0");
    let record = dir.path().join("record.json");
    let cwd = dir.path().join("instance");
    std::fs::create_dir_all(&cwd).unwrap();
    let out = wrapper(&home)
        .current_dir(&cwd)
        .env("FAKE_GAME_RECORD", &record)
        .env("FAKE_GAME_EXIT", "7")
        .arg(&game)
        .args(["--resources", "C:/a b/res", "--name", "Dev"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(7),
        "the game's exit code comes through: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rec: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&record).unwrap()).unwrap();
    assert_eq!(
        rec["args"],
        serde_json::json!(["--resources", "C:/a b/res", "--name", "Dev"])
    );
    assert_eq!(rec["fish_home"], home.to_str().unwrap());
    let started = Path::new(rec["cwd"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert_eq!(
        started,
        cwd.canonicalize().unwrap(),
        "the game keeps the instance folder as cwd"
    );
    let log = std::fs::read_to_string(home.join("logs/fish-launch.log")).unwrap();
    assert!(
        log.contains("starting edition 0.1.0+horizon.6f8c7a0 for nightly-2026-10-28-6f8c7a0"),
        "{log}"
    );
    assert!(log.contains("the game exited with 7"), "{log}");
}

#[test]
fn a_version_without_an_edition_is_refused_with_exit_code_2() {
    let dir = tempfile::tempdir().unwrap();
    let home = install_edition(dir.path(), None);
    let game = vanilla(dir.path(), "nightly-2026-11-02-abcdef0");
    let out = wrapper(&home).arg(&game).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("Fish has no build for Horizon nightly-2026-11-02-abcdef0"),
        "{err}"
    );
    assert!(err.contains("fish-installer repair"), "{err}");
    assert!(
        std::fs::read_to_string(home.join("logs/fish-launch.log"))
            .unwrap()
            .contains("Fish has no build")
    );
}

#[test]
fn a_damaged_edition_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let home = install_edition(dir.path(), Some("00"));
    let game = vanilla(dir.path(), "nightly-2026-10-28-6f8c7a0");
    let out = wrapper(&home).arg(&game).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("Fish edition 0.1.0+horizon.6f8c7a0 is damaged (checksum mismatch). Run \"fish-installer repair\"."),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn without_arguments_it_reports_what_is_installed() {
    let dir = tempfile::tempdir().unwrap();
    let home = install_edition(dir.path(), None);
    let out = wrapper(&home).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("0.1.0+horizon.6f8c7a0 (Fish 0.1.0, Horizon 6f8c7a0"),
        "{text}"
    );
    assert!(text.contains(home.to_str().unwrap()), "{text}");
}

#[cfg(windows)]
#[test]
fn killing_the_wrapper_kills_the_game() {
    let dir = tempfile::tempdir().unwrap();
    let home = install_edition(dir.path(), None);
    let game = vanilla(dir.path(), "nightly-2026-10-28-6f8c7a0");
    let record = dir.path().join("record.json");
    let mut wrapper_proc = wrapper(&home)
        .env("FAKE_GAME_RECORD", &record)
        .env("FAKE_GAME_SLEEP_SECS", "60")
        .arg(&game)
        .spawn()
        .unwrap();
    // wait for the game to say it started
    let started = std::time::Instant::now();
    while !record.is_file() {
        assert!(started.elapsed().as_secs() < 30, "the game never started");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let pid: u32 =
        serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&record).unwrap())
            .unwrap()["pid"]
            .as_u64()
            .unwrap() as u32;
    wrapper_proc.kill().unwrap();
    wrapper_proc.wait().unwrap();
    // the job object takes the game down with the wrapper
    let gone = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}")])
            .output()
            .unwrap();
        !String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    });
    assert!(gone, "game process {pid} survived its wrapper");
}

#[test]
fn main_with_reports_errors_and_info_in_process() {
    let dir = tempfile::tempdir().unwrap();
    let home = install_edition(dir.path(), None);
    let args = |v: &[&std::path::Path]| -> Vec<std::ffi::OsString> {
        std::iter::once("fish-launch".into())
            .chain(v.iter().map(|p| p.as_os_str().to_owned()))
            .collect()
    };
    assert_eq!(
        fish_launch::main_with(&args(&[]), &home),
        0,
        "no game: prints what is installed"
    );
    let unknown = vanilla(dir.path(), "nightly-2026-11-02-abcdef0");
    assert_eq!(fish_launch::main_with(&args(&[&unknown]), &home), 2);
    let elsewhere = dir.path().join("horizon-client.exe");
    assert_eq!(fish_launch::main_with(&args(&[&elsewhere]), &home), 2);
    let log = std::fs::read_to_string(home.join("logs/fish-launch.log")).unwrap();
    assert!(
        log.contains("Fish has no build for Horizon")
            && log.contains("cannot tell which Horizon version"),
        "{log}"
    );
}

/// On Unix a successful start replaces the process (exec), which a test cannot survive.
#[cfg(windows)]
#[test]
fn main_with_runs_the_game_in_process() {
    let dir = tempfile::tempdir().unwrap();
    let home = install_edition(dir.path(), None);
    let game = vanilla(dir.path(), "nightly-2026-10-28-6f8c7a0");
    let args = vec![
        std::ffi::OsString::from("fish-launch"),
        game.into_os_string(),
        "--name".into(),
        "Dev".into(),
    ];
    assert_eq!(fish_launch::main_with(&args, &home), 0);
    let log = std::fs::read_to_string(home.join("logs/fish-launch.log")).unwrap();
    assert!(log.contains("the game exited with 0"), "{log}");
}
