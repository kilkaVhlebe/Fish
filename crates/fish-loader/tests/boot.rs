//! Integration tests: real example mods are built, packed and loaded by a real loader.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;

use fish_api::ActionResult;
use fish_api::event::Payload;
use fish_loader::{ModState, Side};
use fish_test_host::{TestHost, build_example, pack_example};

#[repr(C)]
struct Ping {
    size: u32,
    flag: u32,
}

// SAFETY: repr(C) with a leading `size: u32`.
unsafe impl Payload for Ping {}

fn ping(flag: u32) -> Ping {
    Ping {
        size: size_of::<Ping>() as u32,
        flag,
    }
}

#[repr(C)]
struct Tick {
    size: u32,
}

// SAFETY: repr(C) with a leading `size: u32`.
unsafe impl Payload for Tick {}

fn tick() -> Tick {
    Tick {
        size: size_of::<Tick>() as u32,
    }
}

fn example_manifest(package: &str) -> String {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/mods")
        .join(package)
        .join("fish.mod.toml");
    std::fs::read_to_string(p).unwrap()
}

fn failed_reason(host: &TestHost, id: &str) -> String {
    match host.state(id) {
        Some(ModState::Failed(r)) => r,
        other => panic!("expected {id} to be failed, got {other:?}"),
    }
}

#[test]
fn entrypoints_run_in_order_and_client_follows_main() {
    let host = TestHost::new()
        .add_mod(build_example("hello"))
        .boot()
        .unwrap();
    assert_eq!(
        host.mod_log("hello"),
        ["Hello from main", "Hello from client"]
    );
    assert_eq!(host.state("hello"), Some(ModState::Ok));
    let report = host.loader().report();
    assert_eq!(report.mods.len(), 1);
    assert_eq!((report.abi.major, report.abi.minor), (0, 1));
    assert_eq!(report.side, "client");
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("\"id\":\"hello\"") && json.contains("\"status\":\"ok\""));
}

#[test]
fn server_side_skips_client_entrypoints() {
    let host = TestHost::new()
        .side(Side::Server)
        .add_mod(build_example("hello"))
        .boot()
        .unwrap();
    assert_eq!(host.mod_log("hello"), ["Hello from main"]);
    assert_eq!(host.loader().report().side, "server");
}

#[test]
fn pre_launch_of_every_mod_runs_before_any_main() {
    let host = TestHost::new()
        .add_mod(build_example("hello"))
        .add_mod(build_example("ordering"))
        .boot()
        .unwrap();
    let logs: Vec<String> = host.logs().into_iter().map(|r| r.message).collect();
    let pos = |needle: &str| logs.iter().position(|m| m.contains(needle)).unwrap();
    // "ordering" sorts after "hello", yet its pre_launch runs before hello's main.
    assert!(pos("ordering early") < pos("Hello from main"), "{logs:?}");
}

#[test]
fn environment_filtering() {
    let manifest =
        example_manifest("hello").replace("license = \"MIT\"", "environment = \"client\"");
    let client_only = pack_example("hello", &manifest, "hello_client_only");
    let server = TestHost::new()
        .side(Side::Server)
        .add_mod(&client_only)
        .boot()
        .unwrap();
    assert!(server.loader().mods().is_empty());
    assert!(server.mod_log("hello").is_empty());
    let client = TestHost::new().add_mod(&client_only).boot().unwrap();
    assert_eq!(client.loader().mods().len(), 1);
}

#[test]
fn abi_mismatch_is_refused() {
    let host = TestHost::new()
        .add_mod(build_example("abi_mismatch"))
        .boot()
        .unwrap();
    assert_eq!(
        failed_reason(&host, "abi_mismatch"),
        "abi_mismatch was built for Fish ABI 0.999; this Fish serves 0.1"
    );
    assert_eq!(host.loader().report().mods[0].status, "failed");
}

#[test]
fn missing_symbol_is_refused() {
    let host = TestHost::new()
        .add_mod(build_example("nosymbol"))
        .boot()
        .unwrap();
    let reason = failed_reason(&host, "nosymbol");
    assert!(reason.contains("does not export fish_mod_v1"), "{reason}");
}

#[test]
fn missing_declared_entrypoint_is_refused() {
    let manifest = example_manifest("hello").replace("main = [\"init\"]", "main = [\"nope\"]");
    let host = TestHost::new()
        .add_mod(pack_example("hello", &manifest, "hello_nope"))
        .boot()
        .unwrap();
    assert_eq!(
        failed_reason(&host, "hello"),
        "hello declares entrypoint main=nope but its library does not export it"
    );
    assert!(host.mod_log("hello").is_empty());
}

#[test]
fn entrypoint_kind_mismatch_is_refused() {
    let custom = example_manifest("kinds").replace("main = []", "\"kinds:custom\" = [\"setup\"]");
    let host = TestHost::new()
        .add_mod(pack_example("kinds", &custom, "kinds_custom"))
        .boot()
        .unwrap();
    assert_eq!(
        failed_reason(&host, "kinds"),
        "kinds declares kinds:custom=setup, which its library exports as an init entrypoint"
    );
    let standard = example_manifest("kinds").replace("main = []", "main = [\"api\"]");
    let host = TestHost::new()
        .add_mod(pack_example("kinds", &standard, "kinds_main"))
        .boot()
        .unwrap();
    assert_eq!(
        failed_reason(&host, "kinds"),
        "kinds declares main=api, which its library exports as an object entrypoint"
    );
}

#[test]
fn panicking_listener_fails_its_mod_and_others_continue() {
    let host = TestHost::new()
        .add_mod(build_example("hello"))
        .add_mod(build_example("panicky"))
        .boot()
        .unwrap();
    let pings = host
        .share::<AtomicU64>("hello:pings")
        .expect("hello publishes its counter");
    let count = || pings.load(std::sync::atomic::Ordering::SeqCst);

    host.fire("hello:ping", &mut ping(0));
    assert_eq!(count(), 1);
    assert_eq!(host.state("panicky"), Some(ModState::Ok));

    host.fire("hello:ping", &mut ping(1));
    assert_eq!(
        count(),
        2,
        "hello's listener runs before and independently of the panic"
    );
    let reason = failed_reason(&host, "panicky");
    assert!(reason.contains("hello:ping"), "{reason}");
    assert!(host.logged("panicky was asked to panic"));
    assert!(host.logged("Fish mod Panicky stopped"));

    host.fire("hello:ping", &mut ping(1));
    assert_eq!(
        count(),
        3,
        "panicky no longer receives events, hello keeps counting"
    );
    assert_eq!(host.state("hello"), Some(ModState::Ok));
    let report = host.loader().report();
    assert_eq!(
        report
            .mods
            .iter()
            .find(|m| m.id == "panicky")
            .unwrap()
            .status,
        "failed"
    );
}

#[test]
fn strict_mode_aborts_in_a_child_process() {
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(exe)
        .env("FISH_TEST_HELLO", build_example("hello"))
        .env("FISH_TEST_PANICKY", build_example("panicky"))
        .args(["--ignored", "--exact", "strict_child_panics", "--nocapture"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "child must abort");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("FISH_LOADER_STRICT"), "{stderr}");
}

#[test]
#[ignore = "run by strict_mode_aborts_in_a_child_process"]
fn strict_child_panics() {
    let host = TestHost::new()
        .strict(true)
        .add_mod(std::env::var_os("FISH_TEST_HELLO").expect("set by the parent test"))
        .add_mod(std::env::var_os("FISH_TEST_PANICKY").expect("set by the parent test"))
        .boot()
        .unwrap();
    host.fire("hello:ping", &mut ping(1));
    unreachable!("strict mode must abort the process");
}

#[test]
fn phases_are_ordered_and_cycles_warn() {
    let host = TestHost::new()
        .add_mod(build_example("ordering"))
        .boot()
        .unwrap();
    let trace = host.share::<Mutex<String>>("ordering:trace").unwrap();
    let take = || std::mem::take(&mut *trace.lock().unwrap());

    host.fire("ordering:phases", &mut tick());
    assert_eq!(take(), "a1 b1 b2 d1 ");

    host.fire("ordering:cycle", &mut tick());
    assert_eq!(
        take(),
        "d1 x1 y1 ",
        "a cycle falls back to first-seen phase order"
    );
    let warnings = host.loader().report().warnings;
    let w = warnings
        .iter()
        .find(|w| w.contains("ordering:cycle"))
        .expect("cycle warning");
    assert!(
        w.contains("ordering:x") && w.contains("ordering:y") && w.contains("cycle"),
        "{w}"
    );
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.contains("ordering:cycle"))
            .count(),
        1,
        "reported once"
    );
}

#[test]
fn first_non_pass_stops_at_the_first_decision() {
    let host = TestHost::new()
        .add_mod(build_example("ordering"))
        .boot()
        .unwrap();
    let trace = host.share::<Mutex<String>>("ordering:trace").unwrap();
    let mut result = ActionResult::Pass;
    host.fire_with_result("ordering:first", &mut tick(), &mut result);
    assert_eq!(result, ActionResult::Consume);
    assert_eq!(
        *trace.lock().unwrap(),
        "p c ",
        "the Fail listener never ran"
    );
}

#[test]
fn object_share_exchange_and_type_mismatch() {
    let host = TestHost::new()
        .add_mod(build_example("share_a"))
        .add_mod(build_example("share_b"))
        .boot()
        .unwrap();
    assert_eq!(
        host.share::<AtomicU64>("share:value")
            .unwrap()
            .load(std::sync::atomic::Ordering::SeqCst),
        42
    );
    assert!(host.logged("share_b: type mismatch detected"));
    assert!(
        host.share::<String>("share:value").is_none(),
        "wrong type tag is refused"
    );
    assert!(host.share::<AtomicU64>("share:absent").is_none());
}

#[test]
fn unsafe_archives_are_refused_and_named() {
    use std::io::Write;
    let host = TestHost::new();
    let dir = host.game_dir().join("fish-mods");
    std::fs::create_dir_all(&dir).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(dir.join("evil.fish")).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    zip.start_file("fish.mod.toml", opts).unwrap();
    zip.write_all(b"schema = 1\nid = \"evil\"\nversion = \"1.0.0\"\n")
        .unwrap();
    zip.start_file("../escape.txt", opts).unwrap();
    zip.write_all(b"x").unwrap();
    zip.finish().unwrap();
    let host = host.boot().unwrap();
    assert!(host.loader().mods().is_empty());
    let errors = host.loader().report().errors;
    assert!(
        errors
            .iter()
            .any(|e| e.contains("../escape.txt") && e.contains("evil.fish")),
        "{errors:?}"
    );
}

#[test]
fn library_extraction_is_reused_for_identical_archives() {
    let host = TestHost::new()
        .add_mod(build_example("hello"))
        .boot()
        .unwrap();
    assert!(host.logged("extracted library of hello"));
    let cache = host.game_dir().join(".fish/cache");
    let dirs: Vec<_> = std::fs::read_dir(&cache)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(dirs.len(), 1);
    let name = dirs[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("hello-0.1.0-") && name.len() == "hello-0.1.0-".len() + 16,
        "{name}"
    );
    let lib = dirs[0].join(fish_pack::library_file_name(
        "hello",
        &fish_pack::platform(),
    ));
    let modified = std::fs::metadata(&lib).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    let host = host.reboot().unwrap();
    assert!(host.logged("reused cached library of hello"));
    assert_eq!(
        std::fs::metadata(&lib).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(host.state("hello"), Some(ModState::Ok));
}

#[test]
fn duplicate_ids_keep_the_newest_version() {
    let newer = example_manifest("hello").replace("version = \"0.1.0\"", "version = \"0.2.0\"");
    let host = TestHost::new()
        .add_mod_as(build_example("hello"), "hello-old.fish")
        .add_mod_as(pack_example("hello", &newer, "hello_new"), "hello-new.fish")
        .boot()
        .unwrap();
    let mods = host.loader().mods();
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].version, "0.2.0");
    let warnings = host.loader().report().warnings;
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("duplicate mod \"hello\"")
                && w.contains("hello-old.fish")
                && w.contains("hello-new.fish")),
        "{:#?}",
        host.loader().report()
    );
}

#[test]
fn dependencies_load_first_and_objects_are_shared() {
    let host = TestHost::new()
        .add_mod(build_example("dep_a"))
        .add_mod(build_example("dep_b"))
        .boot()
        .unwrap();
    let order: Vec<String> = host
        .logs()
        .into_iter()
        .map(|r| r.message)
        .filter(|m| m.starts_with("dep_"))
        .collect();
    assert_eq!(
        order,
        [
            "dep_b early",
            "dep_b init",
            "dep_a init",
            "dep_a sees dep_b api = 7"
        ]
    );
    let ids: Vec<String> = host.loader().mods().into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["dep_b", "dep_a"]);
}

#[test]
fn missing_dependency_fails_the_dependent_with_the_resolver_message() {
    let host = TestHost::new()
        .add_mod(build_example("dep_a"))
        .add_mod(build_example("hello"))
        .boot()
        .unwrap();
    let reason = failed_reason(&host, "dep_a");
    assert!(
        reason.contains("mod \"dep_a\" 0.1.0 needs \"dep_b\" >=1, but \"dep_b\" is not installed"),
        "{reason}"
    );
    assert_eq!(
        host.state("hello"),
        Some(ModState::Ok),
        "unrelated mods still load"
    );
    assert_eq!(host.mod_log("hello").len(), 2);
    assert!(
        host.loader()
            .report()
            .errors
            .iter()
            .any(|e| e.contains("needs \"dep_b\""))
    );
}

#[test]
fn dependents_of_a_mod_that_fails_to_load_fail_too() {
    let broken = example_manifest("dep_b").replace("main = [\"init\"]", "main = [\"nope\"]");
    let host = TestHost::new()
        .add_mod(build_example("dep_a"))
        .add_mod(pack_example("dep_b", &broken, "dep_b_broken"))
        .boot()
        .unwrap();
    assert!(failed_reason(&host, "dep_b").contains("does not export it"));
    assert_eq!(
        failed_reason(&host, "dep_a"),
        "dependency dep_b failed to load"
    );
    assert!(
        host.mod_log("dep_a").is_empty(),
        "a failed dependent never runs"
    );
}

#[test]
fn disabled_hidden_and_foreign_files_are_ignored() {
    let host = TestHost::new()
        .add_mod_as(build_example("hello"), "hello.fish.disabled")
        .add_mod_as(build_example("hello"), ".hidden.fish")
        .add_mod_as(build_example("hello"), "readme.txt");
    let host = host.boot().unwrap();
    assert!(host.loader().mods().is_empty());
    assert!(host.loader().report().errors.is_empty());
}

#[test]
fn dev_mods_and_directories_load() {
    let dir = tempfile::tempdir().unwrap();
    let lib_src = {
        let packed = build_example("hello");
        let mut a = fish_pack::read(&packed).unwrap();
        a.library_bytes(&fish_pack::platform()).unwrap()
    };
    let plat = fish_pack::platform();
    let root = dir.path().join("hello_dev");
    std::fs::create_dir_all(root.join("bin").join(&plat)).unwrap();
    std::fs::write(root.join("fish.mod.toml"), example_manifest("hello")).unwrap();
    std::fs::write(
        root.join("bin")
            .join(&plat)
            .join(fish_pack::library_file_name("hello", &plat)),
        lib_src,
    )
    .unwrap();
    let host = TestHost::new().add_dev_mod(&root).dev(true).boot().unwrap();
    assert_eq!(host.state("hello"), Some(ModState::Ok));
    let missing = TestHost::new()
        .add_dev_mod(dir.path().join("absent"))
        .boot()
        .unwrap();
    assert!(
        missing
            .loader()
            .report()
            .errors
            .iter()
            .any(|e| e.contains("does not exist"))
    );
}

#[test]
fn nested_archives_load_when_needed() {
    let hello = build_example("hello");
    let mut spec = fish_pack::PackSpec::new(
        "schema = 1\nid = \"outer\"\nversion = \"1.0.0\"\nnested = [\"nested/hello.fish\"]\n[depends]\nhello = \">=0.1\"\n",
    );
    spec.add_file("nested/hello.fish", &hello).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let outer = dir.path().join("outer.fish");
    fish_pack::write(spec, &outer).unwrap();
    let host = TestHost::new().add_mod(&outer).boot().unwrap();
    let ids: Vec<String> = host.loader().mods().into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["hello", "outer"]);
    assert_eq!(host.state("hello"), Some(ModState::Ok));
}

#[test]
fn loader_service_reflects_mod_states() {
    // The fake in fish-api covers the wrappers; here the real service is queried through hello's
    // report to make sure statuses agree.
    let host = TestHost::new()
        .add_mod(build_example("hello"))
        .add_mod(build_example("abi_mismatch"))
        .boot()
        .unwrap();
    let report = host.loader().report();
    let status = |id: &str| {
        report
            .mods
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .status
            .clone()
    };
    assert_eq!(status("hello"), "ok");
    assert_eq!(status("abi_mismatch"), "failed");
}

#[test]
fn registering_services_and_events_from_the_host_side() {
    use std::ffi::c_void;
    let host = TestHost::new().boot().unwrap();
    let l = host.loader();
    assert!(l.define_event("host:thing", 0).is_ok());
    assert!(l.define_event("host:thing", 0).is_ok(), "idempotent");
    assert_eq!(
        l.define_event("host:thing", 1),
        fish_abi_status::ALREADY_EXISTS
    );
    assert_eq!(
        l.define_event("bad id", 0),
        fish_abi_status::INVALID_ARGUMENT
    );
    assert_eq!(
        l.define_event("host:other", 7),
        fish_abi_status::INVALID_ARGUMENT
    );
    assert_eq!(l.bus().listener_count("host:thing"), 0);
    // SAFETY: no listeners; payload unused.
    assert!(unsafe { l.fire("host:thing", std::ptr::null_mut(), std::ptr::null_mut()) }.is_ok());
    // SAFETY: as above.
    assert_eq!(
        unsafe { l.fire("host:none", std::ptr::null_mut(), std::ptr::null_mut()) },
        fish_abi_status::NOT_FOUND
    );
    static TABLE: u32 = 1;
    let p = std::ptr::from_ref(&TABLE).cast::<c_void>();
    // SAFETY: `TABLE` is static.
    unsafe {
        assert!(l.register_service("host:svc", 1, p).is_ok());
        assert_eq!(
            l.register_service("host:svc", 1, p),
            fish_abi_status::ALREADY_EXISTS
        );
        assert_eq!(
            l.register_service("bad", 1, p),
            fish_abi_status::INVALID_ARGUMENT
        );
    }
    assert_eq!(
        l.object_share_get("nothing", "t"),
        Err(fish_abi_status::NOT_FOUND)
    );
    assert!(l.game_dir().contains("fish-") || !l.game_dir().is_empty());
    assert_eq!(l.side(), Side::Client);
    assert_eq!(l.horizon_version().to_string(), "0.0.0-dev");
}

mod fish_abi_status {
    pub const ALREADY_EXISTS: fish_api::abi::FishStatus = fish_api::abi::FishStatus::ALREADY_EXISTS;
    pub const INVALID_ARGUMENT: fish_api::abi::FishStatus =
        fish_api::abi::FishStatus::INVALID_ARGUMENT;
    pub const NOT_FOUND: fish_api::abi::FishStatus = fish_api::abi::FishStatus::NOT_FOUND;
}

#[test]
fn probe_exercises_every_host_callback() {
    let host = TestHost::new()
        .dev(true)
        .add_mod(build_example("probe"))
        .boot()
        .unwrap();
    let log = host.mod_log("probe");
    assert!(log.iter().any(|m| m == "probe: ok"), "{log:?}");
    assert!(
        host.loader()
            .report()
            .warnings
            .iter()
            .any(|w| w.contains("returned INVALID_ARGUMENT"))
    );
    assert_eq!(host.state("probe"), Some(ModState::Ok));
    assert!(
        host.logs().iter().any(|r| r.target == "fish/unknown"),
        "unknown token logs as fish/unknown"
    );
}

#[test]
fn malformed_export_tables_are_refused_with_a_reason() {
    let reason = |pkg: &str| {
        let host = TestHost::new().add_mod(build_example(pkg)).boot().unwrap();
        failed_reason(&host, pkg)
    };
    assert_eq!(reason("bad_null"), "bad_null's fish_mod_v1 returned null");
    assert_eq!(
        reason("bad_size"),
        "bad_size's export table is too small (4 bytes)"
    );
    assert_eq!(
        reason("bad_entry"),
        "bad_entry's entrypoint main=init is malformed"
    );
    assert_eq!(
        reason("bad_list"),
        "bad_list's export table has a null entrypoint list"
    );
}

#[test]
fn nested_archive_problems_are_reported() {
    let mut spec = fish_pack::PackSpec::new(
        "schema = 1\nid = \"outer\"\nversion = \"1.0.0\"\nnested = [\"nested/broken.fish\"]\n",
    );
    spec.add_bytes("nested/broken.fish", b"not a zip".to_vec())
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let outer = dir.path().join("outer.fish");
    fish_pack::write(spec, &outer).unwrap();
    let host = TestHost::new().add_mod(&outer).boot().unwrap();
    assert_eq!(host.state("outer"), Some(ModState::Ok));
    let errors = host.loader().report().errors;
    assert!(
        errors
            .iter()
            .any(|e| e.contains("nested archive nested/broken.fish of mod outer")),
        "{errors:?}"
    );
}

#[test]
fn boot_config_reads_the_documented_variables() {
    use fish_loader::BootConfig;
    use std::ffi::OsString;
    let sep = if cfg!(windows) { ";" } else { ":" };
    let vars = |name: &str| -> Option<OsString> {
        match name {
            "FISH_LOADER_ADD_MODS" => Some(format!("a{sep}{sep}b").into()),
            "FISH_LOADER_STRICT" => Some("1".into()),
            "FISH_LOADER_DEV" => Some("0".into()),
            _ => None,
        }
    };
    let cfg = BootConfig::from_vars(Side::Server, "game", vars);
    assert_eq!(cfg.extra_mods, [PathBuf::from("a"), PathBuf::from("b")]);
    assert!(cfg.strict && !cfg.dev);
    assert_eq!(cfg.side, Side::Server);
    let none = BootConfig::from_vars(Side::Client, "game", |_| None);
    assert!(none.extra_mods.is_empty() && !none.strict && !none.dev);
    assert_eq!(none.horizon_version.to_string(), "0.0.0-dev");
}

#[test]
fn boot_fails_only_when_the_game_dir_is_unusable() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a-file");
    std::fs::write(&file, b"x").unwrap();
    let err = fish_loader::boot(fish_loader::BootConfig::new(Side::Client, file.join("sub")))
        .err()
        .unwrap();
    assert!(err.to_string().contains("cannot create"), "{err}");
}
