//! lcov aggregation per crate and gate checking.

use std::collections::BTreeMap;

/// Line counts for one crate.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Lines {
    pub found: u64,
    pub hit: u64,
}

impl Lines {
    /// Coverage percentage; an empty crate counts as fully covered.
    pub fn percent(&self) -> f64 {
        if self.found == 0 {
            100.0
        } else {
            self.hit as f64 * 100.0 / self.found as f64
        }
    }
}

/// Returns the crate name for a source path (`.../crates/<name>/src/...`).
pub fn crate_of(path: &str) -> Option<String> {
    let norm = path.replace('\\', "/");
    let rest = norm.split("crates/").nth(1)?;
    let (name, tail) = rest.split_once('/')?;
    tail.starts_with("src/").then(|| name.to_string())
}

/// True if `path` matches one of the `exclude` globs (`*` matches within and across segments).
pub fn excluded(path: &str, patterns: &[String]) -> bool {
    let norm = path.replace('\\', "/");
    patterns.iter().any(|p| {
        let mut parts = p.split('*');
        let first = parts.next().unwrap_or("");
        // Pattern must appear as a substring, with segments in order.
        let Some(mut pos) = norm.find(first) else {
            return false;
        };
        pos += first.len();
        for part in parts {
            if part.is_empty() {
                continue;
            }
            match norm[pos..].find(part) {
                Some(i) => pos += i + part.len(),
                None => return false,
            }
        }
        true
    })
}

/// Aggregates lcov text into per-crate line counts.
pub fn aggregate(lcov: &str, exclude: &[String]) -> BTreeMap<String, Lines> {
    let mut out: BTreeMap<String, Lines> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in lcov.lines() {
        if let Some(path) = line.strip_prefix("SF:") {
            current = if excluded(path, exclude) {
                None
            } else {
                crate_of(path)
            };
        } else if let Some(da) = line.strip_prefix("DA:") {
            let Some(name) = &current else { continue };
            let hits = da
                .split(',')
                .nth(1)
                .and_then(|h| h.parse::<u64>().ok())
                .unwrap_or(0);
            let e = out.entry(name.clone()).or_default();
            e.found += 1;
            if hits > 0 {
                e.hit += 1;
            }
        } else if line == "end_of_record" {
            current = None;
        }
    }
    out
}

/// Checks gates; returns failure messages. Crates without data are reported unless
/// `missing_ok` says the crate does not exist yet.
pub fn check(
    data: &BTreeMap<String, Lines>,
    gates: &BTreeMap<String, f64>,
    missing_ok: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, gate) in gates {
        match data.get(name) {
            Some(l) if l.percent() + 1e-9 >= *gate => {}
            Some(l) => errors.push(format!(
                "{name}: line coverage {:.1}% ({}/{}) is below the {gate}% gate",
                l.percent(),
                l.hit,
                l.found
            )),
            None if missing_ok(name) => {}
            None => errors.push(format!("{name}: no coverage data")),
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCOV: &str = "SF:C:\\p\\Fish\\crates\\a\\src\\lib.rs\nDA:1,1\nDA:2,0\nDA:3,5\nend_of_record\nSF:/p/Fish/crates/a/tests/t.rs\nDA:1,0\nend_of_record\nSF:/p/Fish/crates/b/src/x.rs\nDA:1,1\nend_of_record\nSF:/p/Fish/crates/b/src/gui/v.rs\nDA:1,0\nend_of_record\nSF:/p/Fish/xtask/src/main.rs\nDA:1,0\nend_of_record\n";

    #[test]
    fn crate_of_requires_src_dir() {
        assert_eq!(crate_of("/x/crates/a/src/lib.rs").as_deref(), Some("a"));
        assert_eq!(
            crate_of("C:\\x\\crates\\a\\src\\lib.rs").as_deref(),
            Some("a")
        );
        assert_eq!(crate_of("/x/crates/a/tests/t.rs"), None);
        assert_eq!(crate_of("/x/xtask/src/main.rs"), None);
    }

    #[test]
    fn excluded_globs() {
        let pats = vec!["crates/b/src/gui/*".to_string(), "xtask".to_string()];
        assert!(excluded("/p/crates/b/src/gui/v.rs", &pats));
        assert!(excluded("/p/xtask/src/main.rs", &pats));
        assert!(!excluded("/p/crates/b/src/x.rs", &pats));
    }

    #[test]
    fn aggregates_per_crate_with_exclusions() {
        let ex = vec!["crates/b/src/gui/*".to_string()];
        let m = aggregate(LCOV, &ex);
        assert_eq!(m["a"], Lines { found: 3, hit: 2 });
        assert_eq!(m["b"], Lines { found: 1, hit: 1 });
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn gates() {
        let m = aggregate(LCOV, &[]);
        let gates = BTreeMap::from([
            ("a".to_string(), 90.0),
            ("b".to_string(), 50.0),
            ("c".to_string(), 50.0),
            ("d".to_string(), 50.0),
        ]);
        let errs = check(&m, &gates, &|n| n == "c" || n == "d");
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].starts_with("a:"));
        let errs = check(&m, &gates, &|n| n == "d");
        assert!(errs.iter().any(|e| e.contains("no coverage data")));
    }
}
