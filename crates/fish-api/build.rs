//! Generates `tag::c` constants from `src/tag/c_tags.txt`.

use std::fmt::Write as _;

fn main() {
    println!("cargo:rerun-if-changed=src/tag/c_tags.txt");
    let text = std::fs::read_to_string("src/tag/c_tags.txt").expect("c_tags.txt");
    let mut out = String::new();
    let mut all = String::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (registry, path) = line.split_once(' ').expect("registry and path");
        let name = format!(
            "{}_{}",
            registry.to_uppercase(),
            path.replace('/', "_").to_uppercase()
        );
        let _ = writeln!(
            out,
            "/// `c:{path}` in the `{registry}` registry.\npub const {name}: TagKey = TagKey::new(\"{registry}\", \"c:{path}\");"
        );
        let _ = writeln!(all, "    {name},");
    }
    let _ = writeln!(
        out,
        "/// Every convention tag, in file order.\npub const ALL: &[TagKey] = &[\n{all}];"
    );
    let dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::write(std::path::Path::new(&dir).join("c_tags.rs"), out).expect("write c_tags.rs");
}
