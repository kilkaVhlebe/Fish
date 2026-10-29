//! End-to-end tests of the `cargo-fish` binary on generated projects.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fish_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn target_dir() -> PathBuf {
    fish_root().join("target/cargo-fish-tests")
}

const EXE: &str = env!("CARGO_BIN_EXE_cargo-fish");

/// Result of one in-process invocation.
struct Run {
    status: Status,
    out: String,
}

struct Status(i32);

impl Status {
    fn success(&self) -> bool {
        self.0 == 0
    }

    fn code(&self) -> Option<i32> {
        Some(self.0)
    }
}

fn text(r: &Run) -> String {
    r.out.clone()
}

/// Runs `cargo fish <args>` in-process.
fn fish(dir: &Path, args: &[&str]) -> Run {
    let (code, out) = fish_raw(dir, args);
    Run {
        status: Status(code),
        out,
    }
}

fn fish_raw(dir: &Path, args: &[&str]) -> (i32, String) {
    let mut argv: Vec<std::ffi::OsString> = vec!["cargo-fish".into()];
    argv.extend(args.iter().map(Into::into));
    if matches!(args.get(1), Some(&"build") | Some(&"test")) {
        argv.push("--target-dir".into());
        argv.push(target_dir().into());
    }
    let mut out = Vec::new();
    match cargo_fish::run_with_exe(argv, dir, Path::new(EXE), &mut out) {
        Ok(code) => (code, String::from_utf8_lossy(&out).into_owned()),
        Err(e) => (1, format!("{}error: {e:#}", String::from_utf8_lossy(&out))),
    }
}

fn new_project(dir: &Path, name: &str, template: &str) -> PathBuf {
    let root = fish_root();
    let o = fish(
        dir,
        &[
            "fish",
            "new",
            name,
            "--template",
            template,
            "--fish-path",
            root.to_str().unwrap(),
        ],
    );
    assert!(o.status.success(), "{}", text(&o));
    dir.join(name)
}

#[test]
fn new_build_test_check_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let project = new_project(tmp.path(), "cli_roundtrip", "basic");
    for f in [
        "Cargo.toml",
        "fish.mod.toml",
        "src/lib.rs",
        "tests/hello.toml",
        "assets/cli_roundtrip/lang/en_us.json",
    ] {
        assert!(project.join(f).is_file(), "{f}");
    }

    let o = fish(&project, &["fish", "test"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("ok      hello.toml") && text(&o).contains("1 passed; 0 failed"),
        "{}",
        text(&o)
    );

    let o = fish(&project, &["fish", "build"]);
    assert!(o.status.success(), "{}", text(&o));
    let packed = target_dir().join("fish/cli_roundtrip-0.1.0.fish");
    assert!(packed.is_file(), "{}", text(&o));
    let archive = fish_pack::read(&packed).unwrap();
    assert!(archive.contains(&format!(
        "bin/{}/{}",
        fish_pack::platform(),
        fish_pack::library_file_name("cli_roundtrip", &fish_pack::platform())
    )));
    assert!(archive.contains("assets/cli_roundtrip/lang/en_us.json"));

    let o = fish(&project, &["fish", "check", packed.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("mod cli_roundtrip 0.1.0 loaded"),
        "{}",
        text(&o)
    );
}

#[test]
fn failing_expectations_show_expected_and_actual() {
    let tmp = tempfile::tempdir().unwrap();
    let project = new_project(tmp.path(), "cli_failing", "client");
    std::fs::write(
        project.join("tests/hello.toml"),
        "[[step]]\n[step.expect]\nlogs = [\"this is never logged\"]\n",
    )
    .unwrap();
    std::fs::write(project.join("tests/aaa_fine.toml"), "[[step]]\nticks = 3\n").unwrap();
    let o = fish(&project, &["fish", "test"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    let out = text(&o);
    assert!(out.contains("ok      aaa_fine.toml"), "{out}");
    assert!(out.contains("FAILED  hello.toml"), "{out}");
    assert!(out.contains("step 1: expect.logs failed"), "{out}");
    assert!(
        out.contains("expected: a message containing \"this is never logged\""),
        "{out}"
    );
    assert!(
        out.contains("Hello from cli_failing"),
        "actual output lists what was logged: {out}"
    );

    // the filter selects scripts by name
    let o = fish(&project, &["fish", "test", "aaa"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("1 passed; 0 failed"));
}

#[test]
fn invalid_scripts_are_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let project = new_project(tmp.path(), "cli_badscript", "basic");
    std::fs::write(project.join("tests/hello.toml"), "[[step]]\nbogus = 1\n").unwrap();
    let o = fish(&project, &["fish", "test"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("unknown field"), "{}", text(&o));
}

#[test]
fn content_projects_have_no_library() {
    let tmp = tempfile::tempdir().unwrap();
    let project = new_project(tmp.path(), "cli_content", "content");
    assert!(!project.join("src").exists() && !project.join("Cargo.toml").exists());
    let o = fish(&project, &["fish", "test"]);
    assert!(o.status.success(), "{}", text(&o));
    let packed = project.join("target/fish/cli_content-0.1.0.fish");
    assert!(packed.is_file());
    let a = fish_pack::read(&packed).unwrap();
    assert!(a.manifest().library.is_none());
    let o = fish(&project, &["fish", "check", packed.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
}

#[test]
fn panic_abort_is_refused_by_build() {
    let tmp = tempfile::tempdir().unwrap();
    let project = new_project(tmp.path(), "cli_abort", "basic");
    let toml = std::fs::read_to_string(project.join("Cargo.toml")).unwrap();
    std::fs::write(
        project.join("Cargo.toml"),
        format!("{toml}\n[profile.dev]\npanic = \"abort\"\n"),
    )
    .unwrap();
    let o = fish(&project, &["fish", "build"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o).contains("panic = \"abort\" is set for the dev profile"),
        "{}",
        text(&o)
    );
}

#[test]
fn library_name_mismatch_is_explained() {
    let tmp = tempfile::tempdir().unwrap();
    let project = new_project(tmp.path(), "cli_mismatch", "basic");
    let m = std::fs::read_to_string(project.join("fish.mod.toml")).unwrap();
    std::fs::write(
        project.join("fish.mod.toml"),
        m.replace(
            "name = \"cli_mismatch\"\n\n[entry",
            "name = \"other\"\n\n[entry",
        ),
    )
    .unwrap();
    let o = fish(&project, &["fish", "build"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("expects"), "{}", text(&o));
}

#[test]
fn a_crashing_mod_does_not_take_check_down() {
    let tmp = tempfile::tempdir().unwrap();
    let project = new_project(tmp.path(), "cli_crash", "basic");
    std::fs::write(
        project.join("src/lib.rs"),
        "fn init() {\n    std::process::abort();\n}\n\nfish_api::export_mod! {\n    main: [init],\n}\n",
    )
    .unwrap();
    let o = fish(&project, &["fish", "build"]);
    assert!(o.status.success(), "{}", text(&o));
    let packed = target_dir().join("fish/cli_crash-0.1.0.fish");
    let o = fish(&project, &["fish", "check", packed.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(
        text(&o).contains("the check process failed"),
        "{}",
        text(&o)
    );
}

#[test]
fn check_reports_unloadable_files() {
    let tmp = tempfile::tempdir().unwrap();
    let junk = tmp.path().join("junk.fish");
    std::fs::write(&junk, b"not a zip").unwrap();
    let o = fish(tmp.path(), &["fish", "check", junk.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("no mod was loaded"), "{}", text(&o));
    assert!(
        text(&o).contains("not a valid .fish archive"),
        "{}",
        text(&o)
    );
}

#[test]
fn argument_and_project_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let o = fish(tmp.path(), &["fish", "build"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("fish.mod.toml"), "{}", text(&o));

    let o = fish(tmp.path(), &["fish", "new", "Bad-Name"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("lowercase"), "{}", text(&o));

    let o = fish(tmp.path(), &["fish", "nonsense"]);
    assert_eq!(o.status.code(), Some(2));

    let o = fish(tmp.path(), &["fish", "--help"]);
    assert!(o.status.success());
    assert!(text(&o).contains("Build, check and test Fish mods"));

    // `cargo fish` and a direct `cargo-fish` invocation both work
    let o = fish(tmp.path(), &["--version"]);
    assert!(o.status.success() && text(&o).contains("cargo-fish"));

    let no_tests = tempfile::tempdir().unwrap();
    let project = new_project(no_tests.path(), "cli_notests", "content");
    std::fs::remove_dir_all(project.join("tests")).unwrap();
    let o = fish(&project, &["fish", "test"]);
    assert!(o.status.success());
    assert!(text(&o).contains("no test scripts found"), "{}", text(&o));
}

#[test]
fn unknown_target_platform_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let project = new_project(tmp.path(), "cli_platform", "basic");
    let o = fish(
        &project,
        &["fish", "build", "--target-platform", "beos-x86"],
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("unknown platform"), "{}", text(&o));
}

#[test]
fn the_binary_reports_errors_and_exit_codes() {
    let tmp = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        Command::new(EXE)
            .args(args)
            .current_dir(tmp.path())
            .output()
            .unwrap()
    };
    let o = run(&["fish", "build"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&o.stderr).starts_with("error: "),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let o = run(&["--version"]);
    assert!(o.status.success() && String::from_utf8_lossy(&o.stdout).contains("cargo-fish"));
    // the hidden child command prints a JSON report
    let junk = tmp.path().join("x.fish");
    std::fs::write(&junk, b"nope").unwrap();
    let o = run(&["__check-child", junk.to_str().unwrap()]);
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("\"mods\":[]"));
}
