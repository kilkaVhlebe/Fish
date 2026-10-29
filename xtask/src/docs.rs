//! `docs-check`: en/ru parity, SUMMARY links, roadmap parity, TODO ban.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use regex::Regex;
use walkdir::WalkDir;

use crate::roadmap;

fn md_files(src: &Path) -> BTreeSet<String> {
    WalkDir::new(src)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "md"))
        .filter_map(|e| {
            e.path()
                .strip_prefix(src)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        })
        .collect()
}

/// Runs all docs checks under `root/docs`; returns error messages.
pub fn check(root: &Path) -> Result<Vec<String>> {
    let mut errors = Vec::new();
    let en = root.join("docs/en/src");
    let ru = root.join("docs/ru/src");
    let (fen, fru) = (md_files(&en), md_files(&ru));
    for f in fen.difference(&fru) {
        errors.push(format!("docs/ru/src is missing {f}"));
    }
    for f in fru.difference(&fen) {
        errors.push(format!("docs/en/src is missing {f}"));
    }
    let link = Regex::new(r"\]\(([^)#\s]+\.md)(?:#[^)]*)?\)")?;
    for (lang, dir, files) in [("en", &en, &fen), ("ru", &ru, &fru)] {
        let summary = dir.join("SUMMARY.md");
        if let Ok(text) = std::fs::read_to_string(&summary) {
            for caps in link.captures_iter(&text) {
                let target = &caps[1];
                if !dir.join(target).is_file() {
                    errors.push(format!(
                        "docs/{lang}/src/SUMMARY.md links to missing {target}"
                    ));
                }
            }
        } else {
            errors.push(format!("docs/{lang}/src/SUMMARY.md is missing"));
        }
        for f in files {
            let text = std::fs::read_to_string(dir.join(f))?;
            for (i, line) in text.lines().enumerate() {
                if line.contains("TODO") {
                    errors.push(format!("docs/{lang}/src/{f}:{} contains TODO", i + 1));
                }
            }
        }
    }
    let road =
        |dir: &Path| std::fs::read_to_string(dir.join("roadmap.md")).map(|t| roadmap::parse(&t));
    match (road(&en), road(&ru)) {
        (Ok(a), Ok(b)) => errors.extend(roadmap::compare(&a, &b)),
        _ => errors.push("roadmap.md missing in en or ru".to_string()),
    }
    Ok(errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fish-xtask-docs-{name}-{}", std::process::id()));
        fs::remove_dir_all(&d).ok();
        for l in ["en", "ru"] {
            fs::create_dir_all(d.join(format!("docs/{l}/src"))).unwrap();
            fs::write(
                d.join(format!("docs/{l}/src/SUMMARY.md")),
                "- [R](roadmap.md)\n",
            )
            .unwrap();
            fs::write(
                d.join(format!("docs/{l}/src/roadmap.md")),
                "- [ ] **P0.1** x\n",
            )
            .unwrap();
        }
        d
    }

    #[test]
    fn clean_tree_passes() {
        let d = scratch("ok");
        assert!(check(&d).unwrap().is_empty());
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn detects_every_failure_class() {
        let d = scratch("bad");
        fs::write(d.join("docs/en/src/extra.md"), "TODO later\n").unwrap();
        fs::write(
            d.join("docs/en/src/SUMMARY.md"),
            "- [R](roadmap.md)\n- [N](nope.md)\n",
        )
        .unwrap();
        fs::write(d.join("docs/ru/src/roadmap.md"), "- [x] **P0.1** x\n").unwrap();
        let errs = check(&d).unwrap().join("\n");
        assert!(errs.contains("docs/ru/src is missing extra.md"), "{errs}");
        assert!(errs.contains("links to missing nope.md"), "{errs}");
        assert!(errs.contains("contains TODO"), "{errs}");
        assert!(errs.contains("check state differs"), "{errs}");
        fs::remove_dir_all(d).ok();
    }
}
