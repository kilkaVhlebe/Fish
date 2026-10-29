//! The real `fish-installer` binary, as a player's shell would run it.

use std::process::Command;

const INSTALLER: &str = env!("CARGO_BIN_EXE_fish-installer");

fn run(args: &[&str], home: &std::path::Path) -> std::process::Output {
    Command::new(INSTALLER)
        .args(args)
        .env("FISH_LOADER_HOME", home)
        .output()
        .unwrap()
}

#[test]
fn help_and_status_work_without_a_launcher() {
    let home = tempfile::tempdir().unwrap();
    let help = run(&["--help"], home.path());
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(
        text.contains("Install Fish next to the Horizon launcher") && text.contains("uninstall"),
        "{text}"
    );

    let status = run(&["status"], home.path());
    assert_eq!(status.status.code(), Some(1), "not installed");
    assert!(String::from_utf8_lossy(&status.stdout).contains("Fish is not installed"));
}

#[test]
fn a_missing_launcher_is_a_clear_error_and_unknown_commands_are_usage_errors() {
    let home = tempfile::tempdir().unwrap();
    let out = run(
        &[
            "install",
            "--launcher",
            "/no/such/launcher",
            "--yes",
            "--edition",
            "x.zip",
        ],
        home.path(),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).starts_with("error: no working horizon-launcher"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(run(&["frobnicate"], home.path()).status.code(), Some(2));
    assert!(run(&["--version"], home.path()).status.success());
}
