use std::path::PathBuf;

use fish_metadata::{
    Builtins, Candidate, LoadCondition, ModId, Origin, Reason, Resolution, ResolutionError, Side,
    parse, resolve,
};
use proptest::prelude::*;
use semver::Version;

fn builtins() -> Builtins {
    Builtins::new(
        Version::parse("0.1.0").unwrap(),
        Version::parse("0.2.0-nightly.20261028+6f8c7a0").unwrap(),
    )
}

fn manifest(id: &str, version: &str, extra: &str) -> fish_metadata::Manifest {
    parse(&format!(
        "schema = 1\nid = \"{id}\"\nversion = \"{version}\"\n{extra}"
    ))
    .unwrap()
}

fn folder(id: &str, version: &str, extra: &str) -> Candidate {
    Candidate {
        manifest: manifest(id, version, extra),
        origin: Origin::Folder(PathBuf::from(format!("fish-mods/{id}.fish"))),
        condition: LoadCondition::Always,
    }
}

fn nested(parent: &str, id: &str, version: &str, extra: &str) -> Candidate {
    Candidate {
        manifest: manifest(id, version, extra),
        origin: Origin::Nested {
            parent: ModId::parse(parent).unwrap(),
            path: format!("nested/{id}.fish"),
        },
        condition: LoadCondition::IfPossible,
    }
}

fn ids(r: &Resolution) -> Vec<String> {
    r.active
        .iter()
        .map(|m| format!("{}@{}", m.manifest.id, m.manifest.version))
        .collect()
}

fn go(c: Vec<Candidate>) -> Result<Resolution, ResolutionError> {
    resolve(c, Side::Client, &builtins())
}

#[test]
fn missing_dependency_wrong_version_message() {
    let err = go(vec![
        folder("aa", "1.0.0", "[depends]\nbb = \">=2\""),
        folder("bb", "1.4.0", ""),
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "mod \"aa\" 1.0.0 needs \"bb\" >=2, but only 1.4.0 is installed (fish-mods/bb.fish)"
    );
    assert_eq!(err.problems[0].subject.as_str(), "aa");
}

#[test]
fn missing_dependency_not_installed_and_many_versions() {
    let err = go(vec![folder("aa", "1.0.0", "[depends]\nzz = \"*\"")]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "mod \"aa\" 1.0.0 needs \"zz\" *, but \"zz\" is not installed"
    );

    let err = go(vec![
        folder("aa", "1.0.0", "[depends]\nbb = \">=5\""),
        nested("aa", "bb", "2.0.0", ""),
        folder("bb", "1.0.0", ""),
    ])
    .unwrap_err();
    assert!(
        err.to_string().contains("only 2.0.0, 1.0.0 are installed"),
        "{err}"
    );
}

#[test]
fn every_problem_is_listed() {
    let err = go(vec![
        folder("aa", "1.0.0", "[depends]\nxx = \"*\""),
        folder("bb", "1.0.0", "[depends]\nyy = \"*\"\nminecraft = \">=99\""),
    ])
    .unwrap_err();
    assert_eq!(err.problems.len(), 3, "{err}");
    assert!(
        err.to_string()
            .contains("needs \"minecraft\" >=99, but only 26.3.0 is installed (built in)")
    );
}

#[test]
fn builtin_dependencies_and_lenient_horizon() {
    let r = go(vec![folder(
        "aa",
        "1.0.0",
        "[depends]\nfish = \">=0.1.0\"\nfish_api = \"*\"\nminecraft = \"26.3\"\nhorizon = \">=0.2.0\"",
    )])
    .unwrap();
    assert_eq!(ids(&r), ["aa@1.0.0"]);
    assert!(
        go(vec![folder(
            "aa",
            "1.0.0",
            "[depends]\nhorizon = \">=0.3.0\""
        )])
        .is_err()
    );
}

#[test]
fn picks_highest_satisfying_nested_version() {
    let r = go(vec![
        folder("app", "1.0.0", "[depends]\nlib = \"<3\""),
        nested("app", "lib", "1.0.0", ""),
        nested("app", "lib", "2.5.0", ""),
        nested("app", "lib", "3.0.0", ""),
    ])
    .unwrap();
    assert_eq!(ids(&r), ["lib@2.5.0", "app@1.0.0"]);
    assert_eq!(
        r.inactive
            .iter()
            .filter(|(_, why)| *why == Reason::NotSelected)
            .count(),
        2
    );
}

#[test]
fn nested_if_possible_is_dropped_when_unsatisfiable() {
    let r = go(vec![
        folder("app", "1.0.0", ""),
        nested("app", "extra", "1.0.0", "[depends]\nmissing = \"*\""),
    ])
    .unwrap();
    assert_eq!(ids(&r), ["app@1.0.0"]);
    let (id, why) = &r.inactive[0];
    assert_eq!(id.as_str(), "extra");
    assert!(
        matches!(why, Reason::Unsatisfied(m) if m.contains("needs \"missing\"")),
        "{why:?}"
    );
}

#[test]
fn nested_if_possible_is_dropped_when_it_conflicts() {
    let r = go(vec![
        folder("app", "1.0.0", "[breaks]\nextra = \"*\""),
        nested("app", "extra", "1.0.0", ""),
    ])
    .unwrap();
    assert_eq!(ids(&r), ["app@1.0.0"]);
    assert!(matches!(&r.inactive[0].1, Reason::Unsatisfied(m) if m.contains("conflicts")));
}

#[test]
fn nested_without_requirements_is_loaded() {
    let r = go(vec![
        folder("app", "1.0.0", ""),
        nested("app", "extra", "1.0.0", ""),
    ])
    .unwrap();
    assert_eq!(ids(&r), ["app@1.0.0", "extra@1.0.0"]);
}

#[test]
fn provides_satisfies_dependencies() {
    let r = go(vec![
        folder("aa", "1.0.0", "[depends]\nold_name = \">=2\""),
        folder("bb", "2.1.0", "provides = [\"old_name\"]"),
    ])
    .unwrap();
    assert_eq!(ids(&r), ["bb@2.1.0", "aa@1.0.0"]);
    let err = go(vec![
        folder("aa", "1.0.0", "[depends]\nold_name = \">=3\""),
        folder("bb", "2.1.0", "provides = [\"old_name\"]"),
    ])
    .unwrap_err();
    assert!(err.to_string().contains("only 2.1.0 is installed"));
}

#[test]
fn alias_collisions_fail() {
    let err = go(vec![
        folder("aa", "1.0.0", "provides = [\"shared\"]"),
        folder("bb", "1.0.0", "provides = [\"shared\"]"),
    ])
    .unwrap_err();
    assert!(err.to_string().contains("no consistent set"), "{err}");
    let err = go(vec![
        folder("aa", "1.0.0", "provides = [\"bb\"]"),
        folder("bb", "1.0.0", ""),
    ])
    .unwrap_err();
    assert!(!err.problems.is_empty());
}

#[test]
fn breaks_is_a_hard_error() {
    let err = go(vec![
        folder("aa", "1.0.0", "[breaks]\nbb = \"<2\""),
        folder("bb", "1.5.0", ""),
    ])
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("mod \"aa\" 1.0.0 breaks \"bb\" <2, but 1.5.0 is installed"),
        "{err}"
    );
    // A non-matching version is fine.
    assert!(
        go(vec![
            folder("aa", "1.0.0", "[breaks]\nbb = \"<2\""),
            folder("bb", "2.0.0", ""),
        ])
        .is_ok()
    );
    // breaks is symmetric in effect: b's declaration hits a as well.
    assert!(
        go(vec![
            folder("aa", "1.0.0", ""),
            folder("bb", "1.0.0", "[breaks]\naa = \"*\""),
        ])
        .is_err()
    );
}

#[test]
fn conflicts_and_recommends_warn() {
    let r = go(vec![
        folder("aa", "1.0.0", "[conflicts]\nbb = \"<2\"\n[recommends]\ncc = \">=1\"\nbb = \">=5\"\n[suggests]\ndd = \"*\""),
        folder("bb", "1.5.0", ""),
    ])
    .unwrap();
    assert_eq!(ids(&r), ["aa@1.0.0", "bb@1.5.0"]);
    assert_eq!(
        r.warnings,
        [
            "mod \"aa\" 1.0.0 recommends \"bb\" >=5, but 1.5.0 is installed",
            "mod \"aa\" 1.0.0 recommends \"cc\" >=1, but it is not installed",
            "mod \"aa\" 1.0.0 conflicts with \"bb\" 1.5.0",
        ]
    );
}

#[test]
fn duplicate_ids_keep_the_higher_version() {
    let r = go(vec![folder("aa", "1.0.0", ""), {
        let mut c = folder("aa", "2.0.0", "");
        c.origin = Origin::Folder(PathBuf::from("fish-mods/a-new.fish"));
        c
    }])
    .unwrap();
    assert_eq!(ids(&r), ["aa@2.0.0"]);
    assert_eq!(r.warnings.len(), 1);
    assert!(r.warnings[0].contains("keeping 2.0.0") && r.warnings[0].contains("discarding 1.0.0"));
    assert!(matches!(&r.inactive[0].1, Reason::Duplicate { .. }));
}

#[test]
fn dev_mod_wins_equal_version() {
    let mut dev = folder("aa", "1.0.0", "");
    dev.origin = Origin::Dev(PathBuf::from("dev/a"));
    let r = go(vec![folder("aa", "1.0.0", ""), dev]).unwrap();
    assert!(matches!(r.active[0].origin, Origin::Dev(_)));
    assert!(matches!(
        &r.inactive[0].1,
        Reason::Duplicate {
            kept: Origin::Dev(_)
        }
    ));
}

#[test]
fn environment_filter() {
    let c = vec![
        folder("cc", "1.0.0", "environment = \"client\""),
        folder("ss", "1.0.0", "environment = \"server\""),
        folder("xx", "1.0.0", ""),
    ];
    let r = resolve(c.clone(), Side::Client, &builtins()).unwrap();
    assert_eq!(ids(&r), ["cc@1.0.0", "xx@1.0.0"]);
    assert!(matches!(&r.inactive[0], (id, Reason::Environment(_)) if id.as_str() == "ss"));
    let r = resolve(c, Side::Server, &builtins()).unwrap();
    assert_eq!(ids(&r), ["ss@1.0.0", "xx@1.0.0"]);
    // a client-only mod needed by a server mod is reported as missing
    let err = resolve(
        vec![
            folder("cc", "1.0.0", "environment = \"client\""),
            folder("ss", "1.0.0", "[depends]\ncc = \"*\""),
        ],
        Side::Server,
        &builtins(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("\"cc\" is not installed"));
}

#[test]
fn load_order_follows_dependencies_then_id() {
    let r = go(vec![
        folder("zz", "1.0.0", ""),
        folder("aa", "1.0.0", "[depends]\nmm = \"*\""),
        folder("mm", "1.0.0", "[depends]\nzz = \"*\""),
        folder("bb", "1.0.0", ""),
    ])
    .unwrap();
    assert_eq!(ids(&r), ["bb@1.0.0", "zz@1.0.0", "mm@1.0.0", "aa@1.0.0"]);
}

#[test]
fn cycles_are_ordered_by_id_with_a_warning() {
    let r = go(vec![
        folder("bb", "1.0.0", "[depends]\naa = \"*\""),
        folder("aa", "1.0.0", "[depends]\nbb = \"*\""),
        folder("cc", "1.0.0", "[depends]\nbb = \"*\""),
    ])
    .unwrap();
    assert_eq!(ids(&r), ["aa@1.0.0", "bb@1.0.0", "cc@1.0.0"]);
    assert_eq!(r.warnings, ["dependency cycle between: aa, bb, cc"]);
}

#[test]
fn self_dependency_is_ignored() {
    let r = go(vec![folder(
        "aa",
        "1.0.0",
        "provides = [\"al\"]\n[depends]\naa = \">=9\"\nal = \"*\"\n[conflicts]\nal = \"*\"",
    )])
    .unwrap();
    assert_eq!(ids(&r), ["aa@1.0.0"]);
    assert!(r.warnings.is_empty());
}

#[test]
fn backtracks_over_versions() {
    // x 2.0 needs y>=2, but y 2.0 breaks x>=2 => x 1.0 must be used.
    let r = go(vec![
        folder("ww", "1.0.0", "[depends]\nyy = \">=1\""),
        nested("ww", "xx", "2.0.0", "[depends]\nyy = \">=2\""),
        nested("ww", "xx", "1.0.0", ""),
        nested("ww", "yy", "2.0.0", "[breaks]\nxx = \">=2\""),
    ])
    .unwrap();
    assert_eq!(ids(&r), ["xx@1.0.0", "yy@2.0.0", "ww@1.0.0"]);
}

#[test]
fn step_limit() {
    // 12 optional mods each needing the next-but-one => exponential without a limit.
    let mut c = vec![folder("root", "1.0.0", "[depends]\nzzz = \"*\"")];
    for i in 0..12 {
        c.push(nested(
            "root",
            &format!("m{i:02}"),
            "1.0.0",
            "[depends]\nzzz = \"*\"",
        ));
    }
    let err = fish_metadata::resolve_with_limit(c, Side::Client, &builtins(), 5).unwrap_err();
    assert_eq!(err.to_string(), "dependency resolution exceeded 5 steps");
}

#[test]
fn empty_input_resolves_to_nothing() {
    let r = go(vec![]).unwrap();
    assert!(r.active.is_empty() && r.inactive.is_empty() && r.warnings.is_empty());
}

#[test]
fn origin_display() {
    assert_eq!(Origin::Builtin.to_string(), "built in");
    let n = Origin::Nested {
        parent: ModId::parse("p1").unwrap(),
        path: "n/x.fish".into(),
    };
    assert_eq!(n.to_string(), "p1!/n/x.fish");
}

fn arb_candidates() -> impl Strategy<Value = Vec<Candidate>> {
    let names = ["aa", "bb", "cc", "dd", "ee"];
    let one = (
        0..5usize,
        1..4u64,
        prop::collection::vec((0..5usize, 0..4u64), 0..3),
        any::<bool>(),
    )
        .prop_map(move |(n, ver, deps, is_nested)| {
            let mut extra = String::from("[depends]\n");
            let mut seen = std::collections::BTreeSet::new();
            for (d, v) in deps {
                if d == n || !seen.insert(d) {
                    continue;
                }
                extra.push_str(&format!("{} = \">={v}\"\n", names[d]));
            }
            let id = names[n];
            let version = format!("{ver}.0.0");
            if is_nested {
                nested("aa", id, &version, &extra)
            } else {
                let mut c = folder(id, &version, &extra);
                c.origin = Origin::Folder(PathBuf::from(format!("m/{id}-{version}.fish")));
                c
            }
        });
    prop::collection::vec(one, 0..8).prop_map(|mut v| {
        // make origins unique so identical (id, version) pairs stay distinguishable
        for (i, c) in v.iter_mut().enumerate() {
            if let Origin::Folder(p) = &mut c.origin {
                *p = PathBuf::from(format!("{}#{i}", p.display()));
            } else if let Origin::Nested { path, .. } = &mut c.origin {
                *path = format!("{path}#{i}");
            }
        }
        v
    })
}

proptest! {
    #[test]
    fn resolution_is_independent_of_input_order(cands in arb_candidates(), seed in any::<u64>()) {
        let mut shuffled = cands.clone();
        // deterministic Fisher-Yates driven by `seed`
        let mut s = seed | 1;
        for i in (1..shuffled.len()).rev() {
            s ^= s << 13; s ^= s >> 7; s ^= s << 17;
            shuffled.swap(i, (s % (i as u64 + 1)) as usize);
        }
        let a = go(cands);
        let b = go(shuffled);
        prop_assert_eq!(a, b);
    }
}
