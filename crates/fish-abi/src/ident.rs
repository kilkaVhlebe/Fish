//! Validation of `namespace:path` identifiers.

const fn all(bytes: &[u8], allow_slash: bool) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let ok = matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-')
            || (allow_slash && b == b'/');
        if !ok {
            return false;
        }
        i += 1;
    }
    true
}

/// True for a non-empty namespace made of `[a-z0-9_.-]`.
pub const fn is_valid_namespace(s: &str) -> bool {
    !s.is_empty() && all(s.as_bytes(), false)
}

/// True for a non-empty path made of `[a-z0-9_./-]`.
pub const fn is_valid_path(s: &str) -> bool {
    !s.is_empty() && all(s.as_bytes(), true)
}

/// Splits `namespace:path` at the first `:`; `None` unless both halves are valid.
pub fn split(s: &str) -> Option<(&str, &str)> {
    let (ns, path) = s.split_once(':')?;
    (is_valid_namespace(ns) && is_valid_path(path)).then_some((ns, path))
}

/// True if `s` is a valid `namespace:path` identifier.
pub fn is_valid_identifier(s: &str) -> bool {
    split(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces() {
        for ok in ["fish", "my_mod", "a.b-c", "x1"] {
            assert!(is_valid_namespace(ok), "{ok}");
        }
        for bad in ["", "Fish", "a:b", "a/b", "a b", "é"] {
            assert!(!is_valid_namespace(bad), "{bad}");
        }
    }

    #[test]
    fn paths() {
        for ok in ["a", "a/b", "tick_end", "x.y-z/1"] {
            assert!(is_valid_path(ok), "{ok}");
        }
        for bad in ["", "A", "a:b", "a b", "a\\b"] {
            assert!(!is_valid_path(bad), "{bad}");
        }
    }

    #[test]
    fn identifiers() {
        assert_eq!(
            split("fish:lifecycle/server_end_tick"),
            Some(("fish", "lifecycle/server_end_tick"))
        );
        assert!(is_valid_identifier("hello:ping"));
        // only the first ':' separates, so a second one makes the path invalid
        assert!(!is_valid_identifier("a:b:c"));
        assert!(!is_valid_identifier("nocolon"));
        assert!(!is_valid_identifier(":x"));
        assert!(!is_valid_identifier("x:"));
        assert!(!is_valid_identifier("A:b"));
    }

    #[test]
    fn usable_in_const_context() {
        const { assert!(is_valid_namespace("fish") && is_valid_path("a/b")) };
    }
}
