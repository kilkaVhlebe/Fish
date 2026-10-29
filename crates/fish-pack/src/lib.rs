//! Reading and writing `.fish` mod archives.
//!
//! A `.fish` file is a deflate zip with `fish.mod.toml` at the root, native libraries under
//! `bin/<platform>/`, and optional `assets/`, `data/`, `nested/*.fish` and an icon. A plain directory
//! with the same layout is accepted for development. All entry names are checked against a strict
//! rule set and every archive is bounded by [`Limits`].

#![deny(missing_docs)]

mod error;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use fish_metadata::Manifest;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

pub use error::PackError;

/// Name of the manifest entry.
pub const MANIFEST_NAME: &str = "fish.mod.toml";

/// Size and count limits (mirroring Horizon content mods).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum number of entries.
    pub max_entries: usize,
    /// Maximum uncompressed size of one entry.
    pub max_entry_bytes: u64,
    /// Maximum uncompressed size of all entries together.
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_entries: 65_536,
            max_entry_bytes: 16 << 20,
            max_total_bytes: 512 << 20,
        }
    }
}

/// The platform key of this build, e.g. `windows-x86_64`.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// File name of a library called `name` on `platform` (`name.dll`, `libname.so`, `libname.dylib`).
pub fn library_file_name(name: &str, platform: &str) -> String {
    match platform.split('-').next() {
        Some("windows") => format!("{name}.dll"),
        Some("macos") => format!("lib{name}.dylib"),
        _ => format!("lib{name}.so"),
    }
}

/// Checks an archive entry name; `Err` carries the broken rule.
pub fn validate_entry_name(name: &str) -> Result<(), &'static str> {
    let trimmed = name.strip_suffix('/').unwrap_or(name);
    if trimmed.is_empty() {
        return Err("empty name");
    }
    if trimmed.contains('\0') {
        return Err("contains NUL");
    }
    if trimmed.contains('\\') {
        return Err("contains a backslash");
    }
    if trimmed.contains(':') {
        return Err("contains a colon");
    }
    if trimmed.starts_with('/') {
        return Err("is absolute");
    }
    if trimmed.split('/').any(|s| s == "..") {
        return Err("contains a `..` segment");
    }
    if trimmed.split('/').any(str::is_empty) {
        return Err("contains an empty segment");
    }
    Ok(())
}

fn unsafe_entry(name: &str, reason: &str) -> PackError {
    PackError::UnsafeEntry {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}

/// One archive entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// `/`-separated name; directories end with `/` in zip archives.
    pub name: String,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// True for directory entries.
    pub is_dir: bool,
}

enum Source {
    Zip(zip::ZipArchive<Cursor<Vec<u8>>>),
    Dir(PathBuf),
}

/// An opened `.fish` archive (or development directory) with a validated manifest.
pub struct FishArchive {
    source: Source,
    manifest: Manifest,
    entries: Vec<EntryInfo>,
    limits: Limits,
    fingerprint: String,
    location: String,
}

impl std::fmt::Debug for FishArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FishArchive")
            .field("location", &self.location)
            .field("id", &self.manifest.id.as_str())
            .field("entries", &self.entries.len())
            .finish()
    }
}

/// Opens a `.fish` file or a directory with default limits.
pub fn read(path: impl AsRef<Path>) -> Result<FishArchive, PackError> {
    FishArchive::open(path.as_ref(), Limits::default())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

impl FishArchive {
    /// Opens `path` (file or directory) with explicit limits.
    pub fn open(path: &Path, limits: Limits) -> Result<FishArchive, PackError> {
        let location = path.display().to_string();
        if path.is_dir() {
            Self::open_dir(path, limits, location)
        } else {
            let bytes =
                fs::read(path).map_err(|e| PackError::io(format!("reading {location}"), e))?;
            Self::from_bytes(bytes, limits, location)
        }
    }

    /// Opens an archive held in memory (used for nested archives). `location` is for messages.
    pub fn from_bytes(
        bytes: Vec<u8>,
        limits: Limits,
        location: String,
    ) -> Result<FishArchive, PackError> {
        let fingerprint = hex(&Sha256::digest(&bytes));
        let mut zip =
            zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| PackError::Zip(e.to_string()))?;
        if zip.len() > limits.max_entries {
            return Err(PackError::TooManyEntries {
                count: zip.len(),
                limit: limits.max_entries,
            });
        }
        let mut entries = Vec::with_capacity(zip.len());
        for i in 0..zip.len() {
            let file = zip
                .by_index_raw(i)
                .map_err(|e| PackError::Zip(e.to_string()))?;
            let name = std::str::from_utf8(file.name_raw())
                .map_err(|_| {
                    unsafe_entry(
                        &String::from_utf8_lossy(file.name_raw()),
                        "name is not UTF-8",
                    )
                })?
                .to_string();
            if file.unix_mode().is_some_and(|m| m & 0o170_000 == 0o120_000) {
                return Err(unsafe_entry(&name, "is a symbolic link"));
            }
            entries.push(EntryInfo {
                name,
                size: file.size(),
                is_dir: file.is_dir(),
            });
        }
        Self::finish(Source::Zip(zip), entries, limits, fingerprint, location)
    }

    fn open_dir(root: &Path, limits: Limits, location: String) -> Result<FishArchive, PackError> {
        let mut entries = Vec::new();
        collect_dir(root, "", &mut entries, limits.max_entries)?;
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let mut hasher = Sha256::new();
        for e in entries.iter().filter(|e| !e.is_dir) {
            hasher.update(e.name.as_bytes());
            hasher.update([0]);
            let data = fs::read(root.join(&e.name))
                .map_err(|err| PackError::io(format!("reading {}/{}", location, e.name), err))?;
            hasher.update((data.len() as u64).to_le_bytes());
            hasher.update(&data);
        }
        let fingerprint = hex(&hasher.finalize());
        Self::finish(
            Source::Dir(root.to_path_buf()),
            entries,
            limits,
            fingerprint,
            location,
        )
    }

    fn finish(
        source: Source,
        entries: Vec<EntryInfo>,
        limits: Limits,
        fingerprint: String,
        location: String,
    ) -> Result<FishArchive, PackError> {
        let mut seen = BTreeSet::new();
        let mut total: u64 = 0;
        for e in &entries {
            validate_entry_name(&e.name).map_err(|r| unsafe_entry(&e.name, r))?;
            if !seen.insert(e.name.trim_end_matches('/')) {
                return Err(PackError::DuplicateEntry(e.name.clone()));
            }
            if e.size > limits.max_entry_bytes {
                return Err(PackError::EntryTooLarge {
                    name: e.name.clone(),
                    size: e.size,
                    limit: limits.max_entry_bytes,
                });
            }
            total = total.saturating_add(e.size);
        }
        if total > limits.max_total_bytes {
            return Err(PackError::TotalTooLarge {
                size: total,
                limit: limits.max_total_bytes,
            });
        }
        let placeholder =
            fish_metadata::parse("schema = 1\nid = \"placeholder\"\nversion = \"0.0.0\"")
                .expect("static manifest");
        let mut archive = FishArchive {
            source,
            manifest: placeholder,
            entries,
            limits,
            fingerprint,
            location,
        };
        let text = match archive.read_entry(MANIFEST_NAME) {
            Ok(t) => t,
            Err(PackError::MissingEntry(_)) => {
                return Err(PackError::MissingManifest(PathBuf::from(&archive.location)));
            }
            Err(e) => return Err(e),
        };
        let text = String::from_utf8(text)
            .map_err(|_| PackError::Zip("fish.mod.toml is not UTF-8".into()))?;
        archive.manifest = fish_metadata::parse(&text)?;
        let has_bin = archive
            .entries
            .iter()
            .any(|e| !e.is_dir && e.name.starts_with("bin/"));
        match (has_bin, archive.manifest.library.is_some()) {
            (true, false) => {
                return Err(PackError::LibraryMismatch(
                    "the archive has bin/ entries but fish.mod.toml has no [library]".into(),
                ));
            }
            (false, true) => {
                return Err(PackError::LibraryMismatch(
                    "fish.mod.toml declares [library] but the archive has no bin/ entries".into(),
                ));
            }
            _ => {}
        }
        Ok(archive)
    }

    /// The validated manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// All entries, in archive order (directories: sorted order).
    pub fn entries(&self) -> &[EntryInfo] {
        &self.entries
    }

    /// Paths of nested archives declared in the manifest.
    pub fn nested(&self) -> &[String] {
        &self.manifest.nested
    }

    /// Hex SHA-256 of the archive (file bytes; for directories, a hash of names and contents).
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Where the archive was read from, for messages.
    pub fn location(&self) -> &str {
        &self.location
    }

    /// Platform keys that have a `bin/<platform>/` folder.
    pub fn platforms(&self) -> Vec<String> {
        let set: BTreeSet<String> = self
            .entries
            .iter()
            .filter(|e| !e.is_dir)
            .filter_map(|e| e.name.strip_prefix("bin/"))
            .filter_map(|rest| rest.split_once('/').map(|(p, _)| p.to_string()))
            .collect();
        set.into_iter().collect()
    }

    /// True if an entry (file) with this name exists.
    pub fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|e| !e.is_dir && e.name == name)
    }

    /// Reads one file entry fully, enforcing the per-entry limit on the real data.
    pub fn read_entry(&mut self, name: &str) -> Result<Vec<u8>, PackError> {
        let limit = self.limits.max_entry_bytes;
        let idx = self
            .entries
            .iter()
            .position(|e| !e.is_dir && e.name == name)
            .ok_or_else(|| PackError::MissingEntry(name.to_string()))?;
        let mut out = Vec::new();
        match &mut self.source {
            Source::Zip(zip) => {
                let file = zip
                    .by_index(idx)
                    .map_err(|e| PackError::Zip(e.to_string()))?;
                file.take(limit + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| PackError::io(format!("reading {name}"), e))?;
            }
            Source::Dir(root) => {
                let path = root.join(name);
                fs::File::open(&path)
                    .and_then(|f| f.take(limit + 1).read_to_end(&mut out))
                    .map_err(|e| PackError::io(format!("reading {}", path.display()), e))?;
            }
        }
        if out.len() as u64 > limit {
            return Err(PackError::EntryTooLarge {
                name: name.to_string(),
                size: out.len() as u64,
                limit,
            });
        }
        Ok(out)
    }

    /// The path of the library for `platform` inside the archive.
    pub fn library_path(&self, platform: &str) -> Result<String, PackError> {
        let lib = self.manifest.library.as_ref().ok_or(PackError::NoLibrary)?;
        let path = format!("bin/{platform}/{}", library_file_name(&lib.name, platform));
        if self.contains(&path) {
            Ok(path)
        } else {
            Err(PackError::NoLibraryForPlatform {
                platform: platform.to_string(),
                available: self.platforms(),
            })
        }
    }

    /// Reads the library for `platform`.
    pub fn library_bytes(&mut self, platform: &str) -> Result<Vec<u8>, PackError> {
        let path = self.library_path(platform)?;
        self.read_entry(&path)
    }
}

fn collect_dir(
    root: &Path,
    prefix: &str,
    out: &mut Vec<EntryInfo>,
    max_entries: usize,
) -> Result<(), PackError> {
    let dir = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix)
    };
    let read =
        fs::read_dir(&dir).map_err(|e| PackError::io(format!("listing {}", dir.display()), e))?;
    for item in read {
        let item = item.map_err(|e| PackError::io(format!("listing {}", dir.display()), e))?;
        let file_name = item.file_name();
        let Some(file_name) = file_name.to_str() else {
            return Err(unsafe_entry(
                &file_name.to_string_lossy(),
                "name is not UTF-8",
            ));
        };
        let name = if prefix.is_empty() {
            file_name.to_string()
        } else {
            format!("{prefix}/{file_name}")
        };
        let meta = fs::symlink_metadata(item.path())
            .map_err(|e| PackError::io(format!("inspecting {}", item.path().display()), e))?;
        if meta.file_type().is_symlink() {
            return Err(unsafe_entry(&name, "is a symbolic link"));
        }
        if out.len() >= max_entries {
            return Err(PackError::TooManyEntries {
                count: out.len() + 1,
                limit: max_entries,
            });
        }
        if meta.is_dir() {
            out.push(EntryInfo {
                name: format!("{name}/"),
                size: 0,
                is_dir: true,
            });
            collect_dir(root, &name, out, max_entries)?;
        } else {
            out.push(EntryInfo {
                name,
                size: meta.len(),
                is_dir: false,
            });
        }
    }
    Ok(())
}

/// Result of [`extract_library`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extraction {
    /// Absolute-or-relative path of the library file inside `dest`.
    pub library: PathBuf,
    /// True if `dest` already held this archive's files, so nothing was written.
    pub reused: bool,
}

const EXTRACT_MARKER: &str = ".fish-extracted";

/// Extracts `bin/<platform>/**` (the library and sibling DLLs) into `dest`.
///
/// A marker file records the archive fingerprint; if it matches and the library exists, the call
/// is a no-op (`reused: true`). The marker is written last, so an interrupted extraction is redone.
pub fn extract_library(
    archive: &mut FishArchive,
    platform: &str,
    dest: &Path,
) -> Result<Extraction, PackError> {
    let lib_entry = archive.library_path(platform)?;
    let lib_name = lib_entry
        .rsplit('/')
        .next()
        .unwrap_or(&lib_entry)
        .to_string();
    let library = dest.join(&lib_name);
    let marker = dest.join(EXTRACT_MARKER);
    let stamp = format!("{}\n{platform}\n", archive.fingerprint);
    if fs::read_to_string(&marker).is_ok_and(|m| m == stamp) && library.is_file() {
        return Ok(Extraction {
            library,
            reused: true,
        });
    }
    let _ = fs::remove_file(&marker);
    let prefix = format!("bin/{platform}/");
    let names: Vec<String> = archive
        .entries
        .iter()
        .filter(|e| !e.is_dir && e.name.starts_with(&prefix))
        .map(|e| e.name.clone())
        .collect();
    for name in names {
        let data = archive.read_entry(&name)?;
        let target = dest.join(&name[prefix.len()..]);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| PackError::io(format!("creating {}", parent.display()), e))?;
        }
        fs::write(&target, data)
            .map_err(|e| PackError::io(format!("writing {}", target.display()), e))?;
    }
    fs::write(&marker, stamp)
        .map_err(|e| PackError::io(format!("writing {}", marker.display()), e))?;
    Ok(Extraction {
        library,
        reused: false,
    })
}

enum FileSource {
    Bytes(Vec<u8>),
    Path(PathBuf),
}

/// Describes the contents of a `.fish` archive to write.
pub struct PackSpec {
    manifest: String,
    files: BTreeMap<String, FileSource>,
}

impl PackSpec {
    /// Starts a spec with the text of `fish.mod.toml`.
    pub fn new(manifest_text: impl Into<String>) -> Self {
        PackSpec {
            manifest: manifest_text.into(),
            files: BTreeMap::new(),
        }
    }

    fn insert(&mut self, name: &str, src: FileSource) -> Result<&mut Self, PackError> {
        validate_entry_name(name).map_err(|r| unsafe_entry(name, r))?;
        if name.ends_with('/') {
            return Err(unsafe_entry(name, "files must not end with `/`"));
        }
        if name == MANIFEST_NAME || self.files.contains_key(name) {
            return Err(PackError::DuplicateEntry(name.to_string()));
        }
        self.files.insert(name.to_string(), src);
        Ok(self)
    }

    /// Adds a file from memory.
    pub fn add_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<&mut Self, PackError> {
        self.insert(name, FileSource::Bytes(bytes))
    }

    /// Adds a file from disk (read when the archive is written).
    pub fn add_file(
        &mut self,
        name: &str,
        path: impl Into<PathBuf>,
    ) -> Result<&mut Self, PackError> {
        self.insert(name, FileSource::Path(path.into()))
    }

    /// Adds every file under `dir` below the archive folder `prefix` (use `""` for the root).
    pub fn add_dir(&mut self, prefix: &str, dir: &Path) -> Result<&mut Self, PackError> {
        let mut entries = Vec::new();
        collect_dir(dir, "", &mut entries, usize::MAX)?;
        for e in entries.into_iter().filter(|e| !e.is_dir) {
            let name = if prefix.is_empty() {
                e.name.clone()
            } else {
                format!("{prefix}/{}", e.name)
            };
            self.insert(&name, FileSource::Path(dir.join(&e.name)))?;
        }
        Ok(self)
    }
}

/// Writes `spec` to `out` as a deterministic deflate zip, with default limits.
pub fn write(spec: PackSpec, out: &Path) -> Result<(), PackError> {
    write_with_limits(spec, out, Limits::default())
}

/// Like [`write`], with explicit limits.
pub fn write_with_limits(spec: PackSpec, out: &Path, limits: Limits) -> Result<(), PackError> {
    let manifest = fish_metadata::parse(&spec.manifest)?;
    let has_bin = spec.files.keys().any(|n| n.starts_with("bin/"));
    match (has_bin, manifest.library.is_some()) {
        (true, false) => {
            return Err(PackError::LibraryMismatch(
                "bin/ entries need a [library] table in fish.mod.toml".into(),
            ));
        }
        (false, true) => {
            return Err(PackError::LibraryMismatch(
                "fish.mod.toml declares [library] but no bin/ entries were added".into(),
            ));
        }
        _ => {}
    }
    if let Some(icon) = &manifest.icon
        && !spec.files.contains_key(icon)
    {
        return Err(PackError::MissingReferenced {
            key: "icon",
            path: icon.clone(),
        });
    }
    for n in &manifest.nested {
        if !spec.files.contains_key(n) {
            return Err(PackError::MissingReferenced {
                key: "nested",
                path: n.clone(),
            });
        }
    }
    if spec.files.len() + 1 > limits.max_entries {
        return Err(PackError::TooManyEntries {
            count: spec.files.len() + 1,
            limit: limits.max_entries,
        });
    }
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|e| PackError::io(format!("creating {}", parent.display()), e))?;
    }
    let file = fs::File::create(out)
        .map_err(|e| PackError::io(format!("creating {}", out.display()), e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    let mut total = 0u64;
    let mut put = |name: &str, data: &[u8]| -> Result<(), PackError> {
        if data.len() as u64 > limits.max_entry_bytes {
            return Err(PackError::EntryTooLarge {
                name: name.to_string(),
                size: data.len() as u64,
                limit: limits.max_entry_bytes,
            });
        }
        total += data.len() as u64;
        if total > limits.max_total_bytes {
            return Err(PackError::TotalTooLarge {
                size: total,
                limit: limits.max_total_bytes,
            });
        }
        zip.start_file(name, options)
            .map_err(|e| PackError::Zip(e.to_string()))?;
        zip.write_all(data)
            .map_err(|e| PackError::io(format!("writing {name}"), e))
    };
    put(MANIFEST_NAME, spec.manifest.as_bytes())?;
    for (name, src) in &spec.files {
        match src {
            FileSource::Bytes(b) => put(name, b)?,
            FileSource::Path(p) => {
                let data = fs::read(p)
                    .map_err(|e| PackError::io(format!("reading {}", p.display()), e))?;
                put(name, &data)?;
            }
        }
    }
    zip.finish().map_err(|e| PackError::Zip(e.to_string()))?;
    Ok(())
}
