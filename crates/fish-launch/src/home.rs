//! Where Fish keeps its files.

use std::ffi::OsString;
use std::path::PathBuf;

/// Fish's home folder from a variable lookup (`FISH_LOADER_HOME`, else the platform's data dir).
pub fn fish_home_from(var: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    if let Some(h) = var("FISH_LOADER_HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(h);
    }
    let home = || {
        var("HOME")
            .or_else(|| var("USERPROFILE"))
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        var("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support"))
    } else {
        var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local/share")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("fish-loader")
}

/// Fish's home folder for this process.
pub fn fish_home() -> PathBuf {
    fish_home_from(|name| std::env::var_os(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |n| {
            pairs
                .iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn explicit_home_wins() {
        assert_eq!(
            fish_home_from(lookup(&[("FISH_LOADER_HOME", "/x/y")])),
            PathBuf::from("/x/y")
        );
        // an empty override is ignored
        let p = fish_home_from(lookup(&[
            ("FISH_LOADER_HOME", ""),
            ("APPDATA", "/a"),
            ("HOME", "/h"),
        ]));
        assert!(p.ends_with("fish-loader"), "{p:?}");
    }

    #[test]
    fn platform_default_is_named_fish_loader() {
        let p = fish_home_from(lookup(&[
            ("APPDATA", "/appdata"),
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "/xdg"),
        ]));
        assert_eq!(p.file_name().unwrap(), "fish-loader");
        let none = fish_home_from(|_| None);
        assert_eq!(none, std::env::temp_dir().join("fish-loader"));
    }
}
