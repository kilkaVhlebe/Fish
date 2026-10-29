//! `cargo fish test`: run `tests/*.toml` scripts against the built mod.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fish_test_host::{TestHost, script};

/// Script files under `tests/` whose file name contains `filter`, sorted.
pub fn find_scripts(project: &Path, filter: Option<&str>) -> Result<Vec<PathBuf>> {
    let dir = project.join("tests");
    let mut found = Vec::new();
    if let Ok(read) = std::fs::read_dir(&dir) {
        for entry in read {
            let path = entry
                .with_context(|| format!("listing {}", dir.display()))?
                .path();
            let is_script = path.extension().is_some_and(|e| e == "toml");
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if is_script && filter.is_none_or(|f| stem.contains(f)) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Runs one script against `built` in a fresh host; `Err` carries the failure text.
pub fn run_script(built: &Path, script_path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(script_path).map_err(|e| e.to_string())?;
    let script = script::parse(&text)?;
    let host = TestHost::new()
        .add_mod(built)
        .boot()
        .map_err(|e| e.to_string())?;
    let mut host = host;
    script::run(&mut host, &script).map_err(|f| f.to_string())
}

/// Runs every matching script; returns `(passed, failed)` after printing results to `out`.
pub fn run_all(
    built: &Path,
    scripts: &[PathBuf],
    out: &mut dyn std::io::Write,
) -> Result<(usize, usize)> {
    let (mut passed, mut failed) = (0, 0);
    for path in scripts {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match run_script(built, path) {
            Ok(()) => {
                passed += 1;
                writeln!(out, "ok      {name}")?;
            }
            Err(e) => {
                failed += 1;
                writeln!(
                    out,
                    "FAILED  {name}\n{}",
                    e.lines()
                        .map(|l| format!("        {l}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )?;
            }
        }
    }
    writeln!(out, "\ntest result: {} passed; {failed} failed", passed)?;
    Ok((passed, failed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_found_sorted_and_filtered() {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("tests");
        std::fs::create_dir_all(&t).unwrap();
        for n in ["b_two.toml", "a_one.toml", "notes.txt", "a_other.toml"] {
            std::fs::write(t.join(n), "").unwrap();
        }
        let names = |f: Option<&str>| -> Vec<String> {
            find_scripts(dir.path(), f)
                .unwrap()
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names(None), ["a_one.toml", "a_other.toml", "b_two.toml"]);
        assert_eq!(names(Some("a_")), ["a_one.toml", "a_other.toml"]);
        assert!(names(Some("zzz")).is_empty());
        assert!(
            find_scripts(&dir.path().join("none"), None)
                .unwrap()
                .is_empty()
        );
    }
}
