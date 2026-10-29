use std::fs;
use std::io::Write;
use std::path::Path;

use fish_pack::{
    FishArchive, Limits, PackError, PackSpec, extract_library, library_file_name, platform, read,
    validate_entry_name, write, write_with_limits,
};

const CODE_MANIFEST: &str = "schema = 1\nid = \"demo\"\nversion = \"1.0.0\"\n[library]\nname = \"demo\"\n[entrypoints]\nmain = [\"init\"]\n";
const DATA_MANIFEST: &str = "schema = 1\nid = \"data\"\nversion = \"1.0.0\"\n";

fn raw_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    for (name, data) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn code_spec() -> PackSpec {
    let mut spec = PackSpec::new(CODE_MANIFEST);
    let lib = library_file_name("demo", &platform());
    spec.add_bytes(&format!("bin/{}/{lib}", platform()), b"LIB".to_vec())
        .unwrap();
    spec.add_bytes(
        &format!("bin/{}/helper.dll", platform()),
        b"HELPER".to_vec(),
    )
    .unwrap();
    spec.add_bytes("bin/linux-x86_64/libdemo.so", b"LINUX".to_vec())
        .unwrap();
    spec.add_bytes("assets/demo/lang/en_us.json", b"{}".to_vec())
        .unwrap();
    spec
}

#[test]
fn platform_key_and_library_names() {
    assert!(platform().contains('-'));
    assert_eq!(library_file_name("x", "windows-x86_64"), "x.dll");
    assert_eq!(library_file_name("x", "linux-x86_64"), "libx.so");
    assert_eq!(library_file_name("x", "macos-aarch64"), "libx.dylib");
}

#[test]
fn entry_name_rules() {
    for ok in ["a", "a/b.txt", "dir/", "bin/windows-x86_64/x.dll"] {
        assert_eq!(validate_entry_name(ok), Ok(()), "{ok}");
    }
    let cases = [
        ("", "empty name"),
        ("/", "empty name"),
        ("../x", "contains a `..` segment"),
        ("a/../x", "contains a `..` segment"),
        ("C:/x", "contains a colon"),
        ("/x", "is absolute"),
        ("a\\b", "contains a backslash"),
        ("a\0b", "contains NUL"),
        ("a//b", "contains an empty segment"),
    ];
    for (bad, reason) in cases {
        assert_eq!(validate_entry_name(bad), Err(reason), "{bad:?}");
    }
}

#[test]
fn round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("demo.fish");
    write(code_spec(), &out).unwrap();
    let mut a = read(&out).unwrap();
    assert_eq!(a.manifest().id.as_str(), "demo");
    assert_eq!(a.library_bytes(&platform()).unwrap(), b"LIB");
    assert_eq!(a.read_entry("assets/demo/lang/en_us.json").unwrap(), b"{}");
    assert!(a.contains("fish.mod.toml"));
    assert!(!a.contains("nope"));
    assert!(a.platforms().contains(&platform()));
    assert!(a.platforms().contains(&"linux-x86_64".to_string()));
    assert!(
        a.entries()
            .iter()
            .any(|e| e.name == "bin/linux-x86_64/libdemo.so" && e.size == 5)
    );
    assert!(a.nested().is_empty());
    assert_eq!(a.fingerprint().len(), 64);
    assert!(a.location().ends_with("demo.fish"));
    assert!(format!("{a:?}").contains("demo"));
}

#[test]
fn writing_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.fish"), dir.path().join("b.fish"));
    write(code_spec(), &a).unwrap();
    write(code_spec(), &b).unwrap();
    assert_eq!(fs::read(a).unwrap(), fs::read(b).unwrap());
}

#[test]
fn missing_platform_lists_available() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("demo.fish");
    write(code_spec(), &out).unwrap();
    let mut a = read(&out).unwrap();
    let err = a.library_bytes("macos-aarch64").unwrap_err();
    assert!(matches!(err, PackError::NoLibraryForPlatform { .. }));
    assert!(
        err.to_string()
            .contains("no native library for macos-aarch64; it provides: ")
    );
    // data-only mods have no library at all
    let data = dir.path().join("data.fish");
    write(PackSpec::new(DATA_MANIFEST), &data).unwrap();
    let err = read(&data)
        .unwrap()
        .library_bytes("windows-x86_64")
        .unwrap_err();
    assert!(matches!(err, PackError::NoLibrary));
}

#[test]
fn unsafe_names_are_refused_with_the_entry_name() {
    let dir = tempfile::tempdir().unwrap();
    for bad in ["../x", "C:/x", "/x", "a\\b"] {
        let p = dir.path().join("bad.fish");
        raw_zip(
            &p,
            &[("fish.mod.toml", DATA_MANIFEST.as_bytes()), (bad, b"x")],
        );
        let err = read(&p).unwrap_err();
        assert!(
            matches!(&err, PackError::UnsafeEntry { name, .. } if name == bad),
            "{bad}: {err}"
        );
        assert!(err.to_string().contains(&format!("{bad:?}")));
    }
}

#[test]
fn symlinks_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("s.fish");
    let mut zip = zip::ZipWriter::new(fs::File::create(&p).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    zip.start_file("fish.mod.toml", opts).unwrap();
    zip.write_all(DATA_MANIFEST.as_bytes()).unwrap();
    zip.add_symlink("link", "target", opts).unwrap();
    zip.finish().unwrap();
    let err = read(&p).unwrap_err();
    assert!(err.to_string().contains("symbolic link"), "{err}");
}

#[test]
fn limits_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("l.fish");
    let mut spec = PackSpec::new(DATA_MANIFEST);
    spec.add_bytes("a", vec![0; 100]).unwrap();
    spec.add_bytes("b", vec![0; 100]).unwrap();
    write(spec, &p).unwrap();

    let tiny = |f: fn(&mut Limits)| {
        let mut l = Limits::default();
        f(&mut l);
        FishArchive::open(&p, l).unwrap_err()
    };
    assert!(matches!(
        tiny(|l| l.max_entries = 2),
        PackError::TooManyEntries { count: 3, limit: 2 }
    ));
    assert!(matches!(
        tiny(|l| l.max_entry_bytes = 50),
        PackError::EntryTooLarge { .. }
    ));
    assert!(matches!(
        tiny(|l| l.max_total_bytes = 150),
        PackError::TotalTooLarge { .. }
    ));

    // writer applies the same limits
    let w = |f: fn(&mut Limits)| {
        let mut l = Limits::default();
        f(&mut l);
        let mut spec = PackSpec::new(DATA_MANIFEST);
        spec.add_bytes("a", vec![0; 100]).unwrap();
        spec.add_bytes("b", vec![0; 100]).unwrap();
        write_with_limits(spec, &dir.path().join("w.fish"), l).unwrap_err()
    };
    assert!(matches!(
        w(|l| l.max_entries = 2),
        PackError::TooManyEntries { .. }
    ));
    assert!(matches!(
        w(|l| l.max_entry_bytes = 50),
        PackError::EntryTooLarge { .. }
    ));
    assert!(matches!(
        w(|l| l.max_total_bytes = 150),
        PackError::TotalTooLarge { .. }
    ));
}

#[test]
fn lying_headers_cannot_exceed_the_entry_limit() {
    // read_entry bounds the real data even if sizes are within limits at open time
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("big.fish");
    let mut spec = PackSpec::new(DATA_MANIFEST);
    spec.add_bytes("a", vec![7; 64]).unwrap();
    write(spec, &p).unwrap();
    let mut a = FishArchive::open(
        &p,
        Limits {
            max_entry_bytes: 1024,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(a.read_entry("a").unwrap().len(), 64);
    assert!(matches!(
        a.read_entry("missing"),
        Err(PackError::MissingEntry(_))
    ));
}

#[test]
fn manifest_problems() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("m.fish");
    raw_zip(&p, &[("x", b"1")]);
    assert!(matches!(
        read(&p).unwrap_err(),
        PackError::MissingManifest(_)
    ));
    raw_zip(&p, &[("fish.mod.toml", b"schema = 1")]);
    let err = read(&p).unwrap_err();
    assert!(
        matches!(err, PackError::Manifest(_))
            && err.to_string().starts_with("invalid fish.mod.toml")
    );
    raw_zip(&p, &[("fish.mod.toml", &[0xff, 0xfe])]);
    assert!(matches!(read(&p).unwrap_err(), PackError::Zip(_)));
    fs::write(&p, b"not a zip").unwrap();
    assert!(matches!(read(&p).unwrap_err(), PackError::Zip(_)));
    assert!(matches!(
        read(dir.path().join("absent.fish")).unwrap_err(),
        PackError::Io { .. }
    ));
}

#[test]
fn library_and_bin_must_agree() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.fish");
    raw_zip(
        &p,
        &[
            ("fish.mod.toml", DATA_MANIFEST.as_bytes()),
            ("bin/linux-x86_64/libx.so", b"x"),
        ],
    );
    assert!(matches!(
        read(&p).unwrap_err(),
        PackError::LibraryMismatch(_)
    ));
    raw_zip(&p, &[("fish.mod.toml", CODE_MANIFEST.as_bytes())]);
    assert!(matches!(
        read(&p).unwrap_err(),
        PackError::LibraryMismatch(_)
    ));

    let out = dir.path().join("w.fish");
    let mut spec = PackSpec::new(DATA_MANIFEST);
    spec.add_bytes("bin/linux-x86_64/libx.so", vec![1]).unwrap();
    assert!(matches!(
        write(spec, &out).unwrap_err(),
        PackError::LibraryMismatch(_)
    ));
    assert!(matches!(
        write(PackSpec::new(CODE_MANIFEST), &out).unwrap_err(),
        PackError::LibraryMismatch(_)
    ));
}

#[test]
fn spec_validation() {
    let mut spec = PackSpec::new(DATA_MANIFEST);
    assert!(matches!(
        spec.add_bytes("../x", vec![]),
        Err(PackError::UnsafeEntry { .. })
    ));
    assert!(matches!(
        spec.add_bytes("dir/", vec![]),
        Err(PackError::UnsafeEntry { .. })
    ));
    assert!(matches!(
        spec.add_bytes("fish.mod.toml", vec![]),
        Err(PackError::DuplicateEntry(_))
    ));
    spec.add_bytes("a", vec![]).unwrap();
    assert!(matches!(
        spec.add_bytes("a", vec![]),
        Err(PackError::DuplicateEntry(_))
    ));
    // invalid manifest text
    let out = tempfile::tempdir().unwrap().path().join("o.fish");
    assert!(matches!(
        write(PackSpec::new("nope"), &out).unwrap_err(),
        PackError::Manifest(_)
    ));
}

#[test]
fn referenced_files_must_exist() {
    let out = tempfile::tempdir().unwrap().path().join("o.fish");
    let icon = format!("{DATA_MANIFEST}icon = \"icon.png\"\n");
    let err = write(PackSpec::new(icon), &out).unwrap_err();
    assert!(err.to_string().contains("icon") && err.to_string().contains("icon.png"));
    let nested = format!("{DATA_MANIFEST}nested = [\"nested/x.fish\"]\n");
    let err = write(PackSpec::new(nested), &out).unwrap_err();
    assert!(matches!(
        err,
        PackError::MissingReferenced { key: "nested", .. }
    ));
}

#[test]
fn nested_archives_open_from_memory() {
    let dir = tempfile::tempdir().unwrap();
    let inner = dir.path().join("inner.fish");
    write(
        PackSpec::new("schema = 1\nid = \"inner\"\nversion = \"2.0.0\"\n"),
        &inner,
    )
    .unwrap();
    let mut spec = PackSpec::new(format!("{DATA_MANIFEST}nested = [\"nested/inner.fish\"]\n"));
    spec.add_file("nested/inner.fish", &inner).unwrap();
    let outer = dir.path().join("outer.fish");
    write(spec, &outer).unwrap();
    let mut a = read(&outer).unwrap();
    assert_eq!(a.nested(), ["nested/inner.fish"]);
    let bytes = a.read_entry("nested/inner.fish").unwrap();
    let n = FishArchive::from_bytes(bytes, Limits::default(), "outer!/inner".into()).unwrap();
    assert_eq!(n.manifest().id.as_str(), "inner");
}

#[test]
fn directory_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("mod");
    let plat = platform();
    let lib = library_file_name("demo", &plat);
    fs::create_dir_all(root.join(format!("bin/{plat}"))).unwrap();
    fs::write(root.join("fish.mod.toml"), CODE_MANIFEST).unwrap();
    fs::write(root.join(format!("bin/{plat}/{lib}")), b"DEVLIB").unwrap();
    let mut a = read(&root).unwrap();
    assert_eq!(a.library_bytes(&plat).unwrap(), b"DEVLIB");
    assert!(a.entries().iter().any(|e| e.is_dir && e.name == "bin/"));
    let before = a.fingerprint().to_string();
    fs::write(root.join(format!("bin/{plat}/{lib}")), b"CHANGED").unwrap();
    assert_ne!(read(&root).unwrap().fingerprint(), before);
    // directory without manifest
    fs::remove_file(root.join("fish.mod.toml")).unwrap();
    assert!(matches!(
        read(&root).unwrap_err(),
        PackError::MissingManifest(_)
    ));
}

#[test]
fn spec_add_dir_and_dir_entry_limits() {
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("assets");
    fs::create_dir_all(assets.join("sub")).unwrap();
    fs::write(assets.join("a.txt"), b"A").unwrap();
    fs::write(assets.join("sub/b.txt"), b"B").unwrap();
    let mut spec = PackSpec::new(DATA_MANIFEST);
    spec.add_dir("assets/demo", &assets).unwrap();
    let out = dir.path().join("o.fish");
    write(spec, &out).unwrap();
    let mut a = read(&out).unwrap();
    assert_eq!(a.read_entry("assets/demo/sub/b.txt").unwrap(), b"B");

    let root = dir.path().join("many");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("fish.mod.toml"), DATA_MANIFEST).unwrap();
    fs::write(root.join("a"), b"1").unwrap();
    fs::write(root.join("b"), b"1").unwrap();
    let err = FishArchive::open(
        &root,
        Limits {
            max_entries: 2,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(err, PackError::TooManyEntries { .. }));
}

#[test]
fn extraction_and_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("demo.fish");
    write(code_spec(), &out).unwrap();
    let mut a = read(&out).unwrap();
    let dest = dir.path().join("cache/demo");
    let first = extract_library(&mut a, &platform(), &dest).unwrap();
    assert!(!first.reused);
    assert_eq!(fs::read(&first.library).unwrap(), b"LIB");
    assert_eq!(fs::read(dest.join("helper.dll")).unwrap(), b"HELPER");
    assert!(
        !dest.join("libdemo.so").exists(),
        "other platforms are not extracted"
    );

    // second call does not touch the files
    fs::write(&first.library, b"TAMPERED").unwrap();
    let second = extract_library(&mut a, &platform(), &dest).unwrap();
    assert!(second.reused);
    assert_eq!(fs::read(&second.library).unwrap(), b"TAMPERED");

    // an interrupted extraction (no marker) is redone
    fs::remove_file(dest.join(".fish-extracted")).unwrap();
    let third = extract_library(&mut a, &platform(), &dest).unwrap();
    assert!(!third.reused);
    assert_eq!(fs::read(&third.library).unwrap(), b"LIB");

    // a different archive with the same destination is not "reused"
    let mut other = PackSpec::new(CODE_MANIFEST);
    other
        .add_bytes(
            &format!(
                "bin/{}/{}",
                platform(),
                library_file_name("demo", &platform())
            ),
            b"NEW".to_vec(),
        )
        .unwrap();
    let out2 = dir.path().join("demo2.fish");
    write(other, &out2).unwrap();
    let mut b = read(&out2).unwrap();
    let fourth = extract_library(&mut b, &platform(), &dest).unwrap();
    assert!(!fourth.reused);
    assert_eq!(fs::read(&fourth.library).unwrap(), b"NEW");

    assert!(matches!(
        extract_library(&mut a, "macos-aarch64", &dest).unwrap_err(),
        PackError::NoLibraryForPlatform { .. }
    ));
}

#[test]
fn error_display_and_source() {
    use std::error::Error;
    let io = PackError::Io {
        context: "doing x".into(),
        source: std::io::Error::other("boom"),
    };
    assert_eq!(io.to_string(), "doing x: boom");
    assert!(io.source().is_some());
    assert!(PackError::DuplicateEntry("a".into()).source().is_none());
    assert!(
        PackError::Zip("z".into())
            .to_string()
            .contains("not a valid .fish archive")
    );
    let m: PackError = fish_metadata::parse("x").unwrap_err().into();
    assert!(m.source().is_some());
    assert!(
        PackError::NoLibraryForPlatform {
            platform: "p".into(),
            available: vec![]
        }
        .to_string()
        .ends_with("for p")
    );
}
