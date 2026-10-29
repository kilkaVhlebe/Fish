//! Process-wide install and the edition self test (global state, so its own test binary).

use fish_loader::selftest::{self, TICKS};
use fish_loader::{LoaderDispatch, install_global};
use fish_test_host::{TestHost, build_example};

#[test]
fn install_global_registers_the_hooks_and_the_selftest_passes() {
    assert!(fish_hooks::get().is_none());
    let host = TestHost::new()
        .add_mod(build_example("hello"))
        .boot()
        .unwrap();
    let loader = install_global(host.loader().clone());
    assert!(fish_hooks::get().is_some());

    // a second install keeps the first loader
    let again = TestHost::new().boot().unwrap();
    let kept = install_global(again.loader().clone());
    assert!(std::ptr::eq(loader, kept));

    let dispatch = LoaderDispatch::new(loader.clone());
    let report = selftest::run(loader, &dispatch);
    assert!(report.ok, "{:#?}", report.checks);
    let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "boot_clean",
            "mods_ok",
            "hooks_installed",
            "hello_ticks",
            "hello_hud",
            "hello_command"
        ]
    );
    assert_eq!(report.boot.mods[0].id, "hello");
    assert_eq!(report.boot.mods[0].status, "ok");
    assert_eq!(TICKS, 20);

    // the report round-trips through JSON with the boot fields flattened at the top
    let dir = tempfile::tempdir().unwrap();
    let plain = TestHost::new().boot().unwrap();
    let plain_dispatch = LoaderDispatch::new(plain.loader().clone());
    // an unwritable report path fails even though every check passes
    assert_eq!(
        selftest::run_and_write(dir.path(), plain.loader(), &plain_dispatch),
        1
    );
    let path = dir.path().join("report.json");
    assert_eq!(
        selftest::run_and_write(&path, plain.loader(), &plain_dispatch),
        0
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["side"], "client");
    assert_eq!(json["mods"].as_array().unwrap().len(), 0);
    assert!(
        json["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["ok"] == true)
    );
}
