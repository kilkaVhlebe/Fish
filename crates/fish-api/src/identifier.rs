//! `namespace:path` identifiers and action results.

use std::fmt;
use std::str::FromStr;

use fish_abi::ident;

pub use fish_abi::ActionResult;

/// A validated `namespace:path` identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identifier {
    namespace: String,
    path: String,
}

/// An invalid identifier string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierError(String);

impl fmt::Display for IdentifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid identifier {:?}: expected `namespace:path` with namespace [a-z0-9_.-] and path [a-z0-9_./-]",
            self.0
        )
    }
}

impl std::error::Error for IdentifierError {}

impl Identifier {
    /// Builds an identifier from parts.
    pub fn new(namespace: &str, path: &str) -> Result<Identifier, IdentifierError> {
        if ident::is_valid_namespace(namespace) && ident::is_valid_path(path) {
            Ok(Identifier {
                namespace: namespace.to_string(),
                path: path.to_string(),
            })
        } else {
            Err(IdentifierError(format!("{namespace}:{path}")))
        }
    }

    /// The namespace (usually a mod id).
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// The path within the namespace.
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl FromStr for Identifier {
    type Err = IdentifierError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match ident::split(s) {
            Some((ns, path)) => Identifier::new(ns, path),
            None => Err(IdentifierError(s.to_string())),
        }
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

/// An [`ActionResult`] that also carries a value (Fabric's `TypedActionResult`).
///
/// `#[repr(C)]` with the action first, so it can be the `out` of a first-non-pass event.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypedActionResult<T> {
    action: u8,
    value: T,
}

impl<T> TypedActionResult<T> {
    /// Wraps `value` with `result`.
    pub fn new(result: ActionResult, value: T) -> Self {
        TypedActionResult {
            action: result.as_u8(),
            value,
        }
    }

    /// The action; an invalid wire value reads as [`ActionResult::Pass`].
    pub fn result(&self) -> ActionResult {
        ActionResult::from_u8(self.action).unwrap_or(ActionResult::Pass)
    }

    /// The carried value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Consumes the result, returning the value.
    pub fn into_value(self) -> T {
        self.value
    }
}

impl<T: Default> Default for TypedActionResult<T> {
    fn default() -> Self {
        TypedActionResult::new(ActionResult::Pass, T::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        let id: Identifier = "fish:lifecycle/server_end_tick".parse().unwrap();
        assert_eq!(id.namespace(), "fish");
        assert_eq!(id.path(), "lifecycle/server_end_tick");
        assert_eq!(id.to_string(), "fish:lifecycle/server_end_tick");
    }

    #[test]
    fn rejects_bad_identifiers() {
        for bad in ["", "x", "A:b", "a:B", "a:b:c", ":b", "a:"] {
            let e = bad.parse::<Identifier>().unwrap_err();
            assert!(e.to_string().contains("invalid identifier"), "{bad}");
        }
        assert!(Identifier::new("a b", "c").is_err());
        assert_eq!(Identifier::new("a", "b").unwrap().to_string(), "a:b");
    }

    #[test]
    fn identifiers_order_and_hash_by_value() {
        let a: Identifier = "a:x".parse().unwrap();
        let b: Identifier = "a:y".parse().unwrap();
        assert!(a < b);
        assert_eq!(a, "a:x".parse().unwrap());
    }

    #[test]
    fn typed_results() {
        let r = TypedActionResult::new(ActionResult::Success, 7u32);
        assert_eq!(r.result(), ActionResult::Success);
        assert_eq!(*r.value(), 7);
        assert_eq!(r.into_value(), 7);
        let d: TypedActionResult<u8> = TypedActionResult::default();
        assert_eq!(d.result(), ActionResult::Pass);
        // first byte is the action, as the wire format requires
        let raw: u8 = unsafe { *(&r as *const TypedActionResult<u32> as *const u8) };
        assert_eq!(raw, 1);
        let bad = TypedActionResult {
            action: 200,
            value: 0u8,
        };
        assert_eq!(bad.result(), ActionResult::Pass);
    }
}
