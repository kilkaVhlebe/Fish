//! Runs every example mod's `tests/*.toml` and checks the script engine's failure reports.

use std::path::Path;

use fish_test_host::script::{self, ScriptFailure};
use fish_test_host::{TestHost, build_example};

fn examples_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mods")
}

#[test]
fn every_example_script_passes() {
    let mut ran = 0;
    let mut dirs: Vec<_> = std::fs::read_dir(examples_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let tests = dir.join("tests");
        if !tests.is_dir() {
            continue;
        }
        let package = dir.file_name().unwrap().to_string_lossy().into_owned();
        let built = build_example(&package);
        let mut scripts: Vec<_> = std::fs::read_dir(&tests)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        scripts.sort();
        for path in scripts
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        {
            let script = script::parse(&std::fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let mut host = TestHost::new().add_mod(&built).boot().unwrap();
            script::run(&mut host, &script).unwrap_or_else(|f| panic!("{}: {f}", path.display()));
            ran += 1;
        }
    }
    assert!(ran >= 1, "no example scripts found");
}

fn run(text: &str) -> Result<(), ScriptFailure> {
    let mut host = TestHost::new()
        .add_mod(build_example("hello"))
        .boot()
        .unwrap();
    script::run(&mut host, &script::parse(text).unwrap())
}

#[test]
fn failures_name_the_step_and_what_was_seen() {
    let f = run("[[step]]\n[[step]]\n[step.expect]\nlogs = [\"Hello from main\", \"nope\"]")
        .unwrap_err();
    assert_eq!(f.step, 2);
    assert_eq!(f.what, "expect.logs");
    assert_eq!(f.expected, "a message containing \"nope\"");
    assert!(f.actual.contains("Hello from main"), "{}", f.actual);
}

#[test]
fn empty_observations_are_reported_as_nothing() {
    for (key, what) in [
        ("chat", "expect.chat"),
        ("hud", "expect.hud"),
        ("command_feedback", "expect.command_feedback"),
    ] {
        let f = run(&format!("[[step]]\n[step.expect]\n{key} = [\"x\"]")).unwrap_err();
        assert_eq!((f.what.as_str(), f.actual.as_str()), (what, "nothing"));
    }
}

#[test]
fn stopped_requires_a_failed_mod() {
    let f = run("[[step]]\n[step.expect]\nstopped = [\"hello\"]").unwrap_err();
    assert_eq!(f.what, "expect.stopped");
    assert!(f.actual.contains("mod hello is Ok"), "{}", f.actual);
    let f = run("[[step]]\n[step.expect]\nstopped = [\"ghost\"]").unwrap_err();
    assert_eq!(f.actual, "mod ghost is not loaded");
    // a mod that really failed satisfies it
    let mut host = TestHost::new()
        .add_mod(build_example("abi_mismatch"))
        .boot()
        .unwrap();
    let s = script::parse("[[step]]\n[step.expect]\nstopped = [\"abi_mismatch\"]").unwrap();
    script::run(&mut host, &s).unwrap();
}

#[test]
fn actions_drive_the_simulated_client() {
    let mut host = TestHost::new()
        .add_mod(build_example("hello"))
        .boot()
        .unwrap();
    let s = script::parse(
        "[[step]]\nkey = { name = \"H\", pressed = true }\ncommand = \"fish hello\"\nchat = \"hi\"\nticks = 20\nframes = 3\n",
    )
    .unwrap();
    script::run(&mut host, &s).unwrap();
    assert_eq!((host.sim.ticks, host.sim.frames), (20, 3));
    assert_eq!(host.sim.commands, ["fish hello"]);
    assert_eq!(host.sim.sent_chat, ["hi"]);
    assert!(host.sim.keys_down.contains("H"));
}
