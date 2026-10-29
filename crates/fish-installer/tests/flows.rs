//! The installer's flows against a fake `horizon-launcher` process (the installer itself runs in
//! this process so its coverage counts; `process.rs` runs the real binary).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use fish_installer::cli::{Cli, Ctx, run};
use fish_installer::flow::{LaunchBinary, Reporter};
use fish_installer::launcher::Probes;
use fish_installer::source::HttpFetcher;
use fish_test_host::build_tool;
use serde_json::Value;
use sha2::{Digest, Sha256};

const INSTALLER: &str = env!("CARGO_BIN_EXE_fish-installer");

/// Result of one in-process run.
struct Out {
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

struct Lines(std::sync::Mutex<Vec<String>>);

impl Reporter for Lines {
    fn step(&self, message: &str) {
        self.0.lock().unwrap().push(message.to_string());
    }
}

struct Env {
    dir: tempfile::TempDir,
    launcher: PathBuf,
    launch: PathBuf,
    home: PathBuf,
    releases_url: std::sync::Mutex<Option<String>>,
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

impl Env {
    /// A sandbox: fake launcher with `installed` versions, installer + fish-launch side by side,
    /// and a Fish home with a space in its path.
    fn new(installed: &[&str]) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let tools = dir.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let launch = tools.join(exe("fish-launch"));
        std::fs::copy(build_tool("fish-launch", "fish-launch"), &launch).unwrap();
        // one copy of the fake launcher per sandbox: its state file lives next to it
        let launcher_dir = dir.path().join("launcher");
        std::fs::create_dir_all(&launcher_dir).unwrap();
        let launcher = launcher_dir.join(exe("horizon-launcher"));
        std::fs::copy(
            build_tool("fish-test-tools", "fake-horizon-launcher"),
            &launcher,
        )
        .unwrap();
        let env = Env {
            launcher,
            home: dir.path().join("Fish Home"),
            launch,
            releases_url: Default::default(),
            dir,
        };
        let state = serde_json::json!({
            "installed": installed,
            "instances": [{"id":"horizon","name":"Horizon","home": env.dir.path().join("game-home"),"version":null,"wrapper":"","selected":true}],
            "calls": []
        });
        std::fs::write(env.state_path(), state.to_string()).unwrap();
        env
    }

    fn state_path(&self) -> PathBuf {
        self.launcher.with_file_name("state.json")
    }

    fn state(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.state_path()).unwrap()).unwrap()
    }

    fn instance(&self, name: &str) -> Option<Value> {
        self.state()["instances"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == name)
            .cloned()
    }

    /// Argument lists of the launcher calls so far, joined with spaces.
    fn calls(&self) -> Vec<String> {
        self.state()["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                c.as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    /// An edition zip whose "horizon-client" is the fake game.
    fn edition_zip(&self, fish_version: &str, commit: &str, release_tag: &str) -> PathBuf {
        let game = std::fs::read(build_tool("fish-test-tools", "fake-game")).unwrap();
        let sha: String = Sha256::digest(&game)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let id = format!("{fish_version}+horizon.{}", &commit[..7]);
        let manifest = format!(
            r#"{{"schema":1,"id":"{id}","fish_version":"{fish_version}","horizon":{{"commit":"{commit}","release_tag":"{release_tag}"}},"platform":"{}-{}","server":false,"binary":{{"name":"{}","sha256":"{sha}"}}}}"#,
            std::env::consts::OS,
            std::env::consts::ARCH,
            exe("horizon-client")
        );
        let path = self
            .dir
            .path()
            .join(format!("edition-{fish_version}-{}.zip", &commit[..7]));
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        z.start_file(exe("horizon-client"), o).unwrap();
        z.write_all(&game).unwrap();
        z.start_file("fish-edition.json", o).unwrap();
        z.write_all(manifest.as_bytes()).unwrap();
        z.finish().unwrap();
        path
    }

    /// Runs the installer in this process against the sandbox.
    fn run(&self, args: &[&str], stdin: &str) -> Out {
        let mut argv = vec!["fish-installer"];
        argv.extend_from_slice(args);
        let cli = Cli::try_parse_from(argv).unwrap();
        let probes = Probes {
            env: None,
            path: vec![],
            running: vec![],
            default_location: None,
        };
        let ctx = Ctx {
            home: self.home.clone(),
            releases_url: self.releases_url.lock().unwrap().clone(),
            allow_unsigned_env: false,
            launch: LaunchBinary::File(self.launch.clone()),
            self_exe: Some(PathBuf::from(INSTALLER)),
            fetcher: &HttpFetcher,
            key: "",
            github: "",
        };
        let reporter = Lines(Default::default());
        let mut out = Vec::new();
        let result = run(
            &cli,
            &probes,
            &mut stdin.as_bytes(),
            &mut out,
            &reporter,
            ctx,
        );
        let mut text = String::from_utf8_lossy(&out).into_owned();
        for line in reporter.0.lock().unwrap().iter() {
            text.push_str(line);
            text.push('\n');
        }
        match result {
            Ok(code) => Out {
                status: Status(code),
                out: text,
            },
            Err(e) => Out {
                status: Status(1),
                out: format!("{text}error: {e}\n"),
            },
        }
    }

    fn install(&self, zip: &Path, extra: &[&str]) -> Out {
        let mut args = vec![
            "install",
            "--launcher",
            self.launcher.to_str().unwrap(),
            "--edition",
            zip.to_str().unwrap(),
            "--yes",
        ];
        args.extend(extra);
        self.run(&args, "")
    }
}

fn text(o: &Out) -> String {
    o.out.clone()
}

const COMMIT: &str = "6f8c7a0e5d531d3834c4d0be4659a1db8e7e5c2a";
const TAG: &str = "nightly-2026-10-28-6f8c7a0";

#[test]
fn install_sets_up_files_instance_pin_and_a_quoted_wrapper() {
    let env = Env::new(&[TAG]);
    let zip = env.edition_zip("0.1.0", COMMIT, "");
    let out = env.install(&zip, &["--select"]);
    assert!(out.status.success(), "{}", text(&out));

    assert!(env.home.join("bin").join(exe("fish-launch")).is_file());
    assert!(
        env.home.join("bin").join(exe("fish-installer")).is_file(),
        "the installer copies itself"
    );
    assert!(
        env.home
            .join("editions/0.1.0+horizon.6f8c7a0/fish-edition.json")
            .is_file()
    );
    let state: Value =
        serde_json::from_str(&std::fs::read_to_string(env.home.join("installer.json")).unwrap())
            .unwrap();
    assert_eq!(state["instance"], "Fish");
    assert_eq!(state["edition"], "0.1.0+horizon.6f8c7a0");
    assert_eq!(state["horizon_tag"], TAG);

    let fish = env.instance("Fish").expect("instance created");
    assert_eq!(fish["version"], TAG);
    assert_eq!(fish["selected"], true);
    assert!(
        Path::new(fish["home"].as_str().unwrap())
            .join("fish-mods")
            .is_dir()
    );
    let wrapper = fish["wrapper"].as_str().unwrap();
    let expected = format!(
        "\"{}\"",
        env.home.join("bin").join(exe("fish-launch")).display()
    );
    assert_eq!(wrapper, expected, "the path with a space is double-quoted");
    assert!(wrapper.contains("Fish Home"));

    let calls = env.calls();
    let pos = |needle: &str| {
        calls
            .iter()
            .position(|c| c.starts_with(needle))
            .unwrap_or_else(|| panic!("{needle} not in {calls:?}"))
    };
    assert!(pos("versions --installed") < pos("instance create Fish"));
    assert!(pos("instance create Fish") < pos(&format!("instance version Fish {TAG}")));
    assert!(pos(&format!("instance version Fish {TAG}")) < pos("instance launch Fish --wrapper"));
    assert!(pos("instance launch Fish --wrapper") < pos("instance select Fish"));
    assert!(
        !calls.iter().any(|c| c.starts_with("install ")),
        "the version was already installed: {calls:?}"
    );
}

#[test]
fn a_missing_horizon_version_is_installed_through_the_launcher() {
    let env = Env::new(&[]);
    let zip = env.edition_zip("0.1.0", COMMIT, "nightly-2026-10-28");
    let out = env.install(&zip, &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        env.calls()
            .contains(&"install nightly-2026-10-28".to_string()),
        "{:?}",
        env.calls()
    );
    assert_eq!(
        env.instance("Fish").unwrap()["version"],
        "nightly-2026-10-28-6f8c7a0"
    );

    // without a release tag in the edition there is nothing to install
    let env = Env::new(&[]);
    let out = env.install(&env.edition_zip("0.1.0", COMMIT, ""), &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out).contains("is not installed in the launcher"),
        "{}",
        text(&out)
    );
}

#[test]
fn installing_twice_is_idempotent_and_never_duplicates_the_instance() {
    let env = Env::new(&[TAG]);
    let zip = env.edition_zip("0.1.0", COMMIT, "");
    assert!(env.install(&zip, &[]).status.success());
    assert!(env.install(&zip, &[]).status.success());
    let n = env.state()["instances"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["name"] == "Fish")
        .count();
    assert_eq!(n, 1);
    let creates = env
        .calls()
        .iter()
        .filter(|c| c.starts_with("instance create"))
        .count();
    assert_eq!(creates, 1);
}

#[test]
fn a_foreign_wrapper_is_refused_unless_forced() {
    let env = Env::new(&[TAG]);
    let zip = env.edition_zip("0.1.0", COMMIT, "");
    assert!(env.install(&zip, &["--instance", "Mine"]).status.success());
    // somebody else's wrapper on that instance
    let mut s = env.state();
    for i in s["instances"].as_array_mut().unwrap() {
        if i["name"] == "Mine" {
            i["wrapper"] = "nice -n 5".into();
        }
    }
    std::fs::write(env.state_path(), s.to_string()).unwrap();

    let out = env.install(&zip, &["--instance", "Mine"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out).contains("already runs through another wrapper (nice -n 5)")
            && text(&out).contains("--force"),
        "{}",
        text(&out)
    );
    assert_eq!(
        env.instance("Mine").unwrap()["wrapper"],
        "nice -n 5",
        "nothing was changed"
    );

    let out = env.install(&zip, &["--instance", "Mine", "--force"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        env.instance("Mine").unwrap()["wrapper"]
            .as_str()
            .unwrap()
            .contains("fish-launch")
    );
}

#[test]
fn uninstall_clears_the_wrapper_keeps_the_instance_and_repair_restores_it() {
    let env = Env::new(&[TAG]);
    let zip = env.edition_zip("0.1.0", COMMIT, "");
    assert!(env.install(&zip, &[]).status.success());
    let mods = Path::new(env.instance("Fish").unwrap()["home"].as_str().unwrap()).join("fish-mods");
    std::fs::write(mods.join("keep.fish"), b"x").unwrap();

    // declining changes nothing
    let out = env.run(
        &["uninstall", "--launcher", env.launcher.to_str().unwrap()],
        "n\n",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(env.home.join("editions").is_dir());

    let out = env.run(
        &[
            "uninstall",
            "--launcher",
            env.launcher.to_str().unwrap(),
            "--yes",
        ],
        "",
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.instance("Fish").unwrap()["wrapper"], "");
    assert!(mods.join("keep.fish").is_file(), "mods are kept");
    assert!(!env.home.join("editions").exists() && !env.home.join("bin").exists());
    assert!(!env.home.join("installer.json").exists());
    assert!(text(&out).contains("were kept"));

    // a second uninstall says so
    let again = env.run(
        &[
            "uninstall",
            "--launcher",
            env.launcher.to_str().unwrap(),
            "--yes",
        ],
        "",
    );
    assert_eq!(again.status.code(), Some(1));
    assert!(text(&again).contains("Fish is not installed"));

    // installing again reuses the instance (its wrapper is empty)
    assert!(env.install(&zip, &[]).status.success());
    assert!(
        env.instance("Fish").unwrap()["wrapper"]
            .as_str()
            .unwrap()
            .contains("fish-launch")
    );
}

#[test]
fn uninstall_survives_a_launcher_that_rejects_an_empty_wrapper() {
    let env = Env::new(&[TAG]);
    assert!(
        env.install(&env.edition_zip("0.1.0", COMMIT, ""), &[])
            .status
            .success()
    );
    std::fs::write(env.launcher.with_file_name("reject-empty-wrapper"), b"").unwrap();
    let out = env.run(
        &[
            "uninstall",
            "--launcher",
            env.launcher.to_str().unwrap(),
            "--yes",
        ],
        "",
    );
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("clear the wrapper by hand"),
        "{}",
        text(&out)
    );
    assert!(
        !env.home.join("editions").exists(),
        "Fish's own files are removed regardless"
    );
}

#[test]
fn status_reports_the_installation_and_the_wrapper() {
    let env = Env::new(&[TAG]);
    let before = env.run(
        &["status", "--launcher", env.launcher.to_str().unwrap()],
        "",
    );
    assert_eq!(before.status.code(), Some(1));
    assert!(text(&before).contains("Fish is not installed"));
    assert!(
        env.install(&env.edition_zip("0.1.0", COMMIT, ""), &[])
            .status
            .success()
    );
    let out = env.run(
        &["status", "--launcher", env.launcher.to_str().unwrap()],
        "",
    );
    assert!(out.status.success());
    let t = text(&out);
    assert!(
        t.contains("Fish edition 0.1.0+horizon.6f8c7a0 for Horizon nightly-2026-10-28-6f8c7a0"),
        "{t}"
    );
    assert!(
        t.contains("Instance wrapper:")
            && t.contains("(ok)")
            && t.contains("fish-launch: installed"),
        "{t}"
    );
}

#[test]
fn a_broken_launcher_or_zip_gives_a_clear_error() {
    let env = Env::new(&[TAG]);
    let out = env.run(&["install", "--launcher", "/does/not/exist", "--yes"], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out).contains("no working horizon-launcher"),
        "{}",
        text(&out)
    );
    let bad = env.dir.path().join("bad.zip");
    std::fs::write(&bad, b"nope").unwrap();
    let out = env.install(&bad, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("not a valid zip"), "{}", text(&out));
    assert!(
        env.instance("Fish").is_none(),
        "nothing happens in the launcher before the edition is verified"
    );
}

/// Serves `files` (path to bytes) on 127.0.0.1:`port` until `stop` is set.
fn serve(port: u16, files: Vec<(String, Vec<u8>)>, stop: Arc<AtomicBool>) {
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let Ok((mut s, _)) = listener.accept() else {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            };
            s.set_nonblocking(false).unwrap();
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let reply = match files.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => {
                    let mut r = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    r.extend_from_slice(body);
                    r
                }
                None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec(),
            };
            let _ = s.write_all(&reply);
        }
    });
}

#[test]
fn install_and_update_from_a_release_index() {
    let env = Env::new(&[TAG, "nightly-2026-10-29-aaaaaaa"]);
    let old = env.edition_zip("0.1.0", COMMIT, "");
    let new_commit = "aaaaaaa5d531d3834c4d0be4659a1db8e7e5c2a0";
    let new = env.edition_zip("0.2.0", new_commit, "");
    let entry = |zip: &Path, id: &str, commit: &str| {
        let bytes = std::fs::read(zip).unwrap();
        serde_json::json!({
            "id": id, "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            "horizon_commit": commit, "horizon_release_tag": "", "url": format!("@BASE@/{id}.zip"),
            "sha256": Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>(), "size": bytes.len()
        })
    };
    let stop = Arc::new(AtomicBool::new(false));
    // Reserve a port first: the index must contain the download URLs, which contain the port.
    let port = {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    let base = format!("http://127.0.0.1:{port}");
    let index = serde_json::json!({"schema":1,"fish_version":"0.2.0","editions":[
        entry(&old, "0.1.0+horizon.6f8c7a0", COMMIT), entry(&new, "0.2.0+horizon.aaaaaaa", new_commit)
    ]})
    .to_string()
    .replace("@BASE@", &base);
    serve(
        port,
        vec![
            ("/editions.json".to_string(), index.into_bytes()),
            (
                "/0.1.0+horizon.6f8c7a0.zip".to_string(),
                std::fs::read(&old).unwrap(),
            ),
            (
                "/0.2.0+horizon.aaaaaaa.zip".to_string(),
                std::fs::read(&new).unwrap(),
            ),
        ],
        stop.clone(),
    );
    *env.releases_url.lock().unwrap() = Some(base);
    let run = |args: &[&str], unsigned: bool| {
        let mut a = vec![
            args[0],
            "--launcher",
            env.launcher.to_str().unwrap(),
            "--yes",
        ];
        if unsigned {
            a.push("--allow-unsigned");
        }
        env.run(&a, "")
    };

    // unsigned indexes are refused by default: there is no release key in this build
    let refused = run(&["install"], false);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        text(&refused).contains("no release signing key"),
        "{}",
        text(&refused)
    );
    assert!(!env.home.join("editions").exists());

    let out = run(&["install"], true);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        env.home.join("editions/0.2.0+horizon.aaaaaaa").is_dir(),
        "the newest edition wins"
    );
    assert_eq!(
        env.instance("Fish").unwrap()["version"],
        "nightly-2026-10-29-aaaaaaa"
    );

    // repair reinstalls the *recorded* edition
    let out = run(&["repair"], true);
    assert!(out.status.success(), "{}", text(&out));
    assert!(env.home.join("editions/0.2.0+horizon.aaaaaaa").is_dir());
    stop.store(true, Ordering::Relaxed);
}
