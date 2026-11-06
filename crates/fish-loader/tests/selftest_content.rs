//! The P8 checks of the edition self test, with the example mod `content_demo`.

use fish_loader::LoaderDispatch;
use fish_loader::selftest;
use fish_test_host::{TestHost, build_example};

#[test]
fn the_selftest_checks_the_content_of_content_demo() {
    let host = TestHost::new()
        .add_mod(build_example("content-demo"))
        .boot()
        .unwrap();
    let dispatch = LoaderDispatch::new(host.loader().clone());
    let report = selftest::run(host.loader(), &dispatch);
    for name in ["content_blocks", "content_folder", "content_tables"] {
        let check = report
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no {name} check: {:#?}", report.checks));
        assert!(check.ok, "{name}: {}", check.detail);
    }
    let tables = report
        .checks
        .iter()
        .find(|c| c.name == "content_tables")
        .unwrap();
    assert!(tables.detail.starts_with("1 fuel(s)"), "{}", tables.detail);
}

#[test]
fn without_content_demo_there_are_no_content_checks() {
    let host = TestHost::new()
        .add_mod(build_example("hello"))
        .boot()
        .unwrap();
    let dispatch = LoaderDispatch::new(host.loader().clone());
    let report = selftest::run(host.loader(), &dispatch);
    assert!(!report.checks.iter().any(|c| c.name.starts_with("content_")));
}
