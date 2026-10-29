//! Errors of `fish-pack`.

use std::fmt;
use std::path::PathBuf;

use fish_metadata::ManifestError;

/// Everything that can go wrong reading or writing a `.fish` archive.
#[derive(Debug)]
pub enum PackError {
    /// An I/O operation failed.
    Io {
        /// What was being done (a path or action).
        context: String,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The file is not a valid zip archive.
    Zip(String),
    /// An entry name breaks the safety rules.
    UnsafeEntry {
        /// The offending entry.
        name: String,
        /// Which rule it breaks.
        reason: String,
    },
    /// Two entries share a name.
    DuplicateEntry(String),
    /// More entries than the limit allows.
    TooManyEntries {
        /// Entries in the archive.
        count: usize,
        /// The limit.
        limit: usize,
    },
    /// One entry is larger than the limit allows.
    EntryTooLarge {
        /// The entry.
        name: String,
        /// Its size in bytes.
        size: u64,
        /// The limit.
        limit: u64,
    },
    /// The uncompressed total is larger than the limit allows.
    TotalTooLarge {
        /// Total in bytes.
        size: u64,
        /// The limit.
        limit: u64,
    },
    /// `fish.mod.toml` is missing.
    MissingManifest(PathBuf),
    /// `fish.mod.toml` is invalid.
    Manifest(ManifestError),
    /// A named entry does not exist.
    MissingEntry(String),
    /// The manifest has no `[library]`.
    NoLibrary,
    /// The archive has no library for this platform.
    NoLibraryForPlatform {
        /// The requested platform key.
        platform: String,
        /// Platform keys the archive does provide.
        available: Vec<String>,
    },
    /// `bin/` exists but the manifest has no `[library]`, or the reverse.
    LibraryMismatch(String),
    /// A file referenced by the manifest (icon, nested) is not in the archive.
    MissingReferenced {
        /// The manifest key.
        key: &'static str,
        /// The missing path.
        path: String,
    },
}

impl PackError {
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        PackError::Io {
            context: context.into(),
            source,
        }
    }
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::Io { context, source } => write!(f, "{context}: {source}"),
            PackError::Zip(m) => write!(f, "not a valid .fish archive: {m}"),
            PackError::UnsafeEntry { name, reason } => {
                write!(f, "unsafe archive entry {name:?}: {reason}")
            }
            PackError::DuplicateEntry(n) => write!(f, "duplicate archive entry {n:?}"),
            PackError::TooManyEntries { count, limit } => {
                write!(f, "archive has {count} entries, the limit is {limit}")
            }
            PackError::EntryTooLarge { name, size, limit } => {
                write!(
                    f,
                    "archive entry {name:?} is {size} bytes, the limit is {limit}"
                )
            }
            PackError::TotalTooLarge { size, limit } => {
                write!(f, "archive expands to {size} bytes, the limit is {limit}")
            }
            PackError::MissingManifest(p) => write!(f, "{} has no fish.mod.toml", p.display()),
            PackError::Manifest(e) => write!(f, "invalid fish.mod.toml: {e}"),
            PackError::MissingEntry(n) => write!(f, "archive has no entry {n:?}"),
            PackError::NoLibrary => f.write_str("the mod has no [library]"),
            PackError::NoLibraryForPlatform {
                platform,
                available,
            } => {
                if available.is_empty() {
                    write!(f, "the mod has no native library for {platform}")
                } else {
                    write!(
                        f,
                        "the mod has no native library for {platform}; it provides: {}",
                        available.join(", ")
                    )
                }
            }
            PackError::LibraryMismatch(m) => f.write_str(m),
            PackError::MissingReferenced { key, path } => {
                write!(
                    f,
                    "manifest {key} refers to {path:?}, which is not in the archive"
                )
            }
        }
    }
}

impl std::error::Error for PackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PackError::Io { source, .. } => Some(source),
            PackError::Manifest(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ManifestError> for PackError {
    fn from(e: ManifestError) -> Self {
        PackError::Manifest(e)
    }
}
