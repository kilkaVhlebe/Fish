//! Finding mods on disk and inside other mods.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fish_metadata::{Candidate, LoadCondition, Origin};
use fish_pack::{FishArchive, Limits};

/// Maximum nesting depth of archives inside archives.
const MAX_NESTING: usize = 4;

/// Everything discovery found.
#[derive(Default)]
pub(crate) struct Discovery {
    pub candidates: Vec<Candidate>,
    pub archives: HashMap<Origin, FishArchive>,
    pub errors: Vec<String>,
}

fn is_hidden_or_disabled(name: &str) -> bool {
    name.starts_with('.') || name.ends_with(".disabled")
}

fn add(
    d: &mut Discovery,
    archive: FishArchive,
    origin: Origin,
    condition: LoadCondition,
    depth: usize,
) {
    let manifest = archive.manifest().clone();
    let mut archive = archive;
    let nested: Vec<String> = archive.nested().to_vec();
    let mut children = Vec::new();
    if depth < MAX_NESTING {
        for path in nested {
            let loaded = archive.read_entry(&path).and_then(|bytes| {
                FishArchive::from_bytes(
                    bytes,
                    Limits::default(),
                    format!("{}!/{path}", archive.location()),
                )
            });
            match loaded {
                Ok(child) => children.push((child, path)),
                Err(e) => d
                    .errors
                    .push(format!("nested archive {path} of mod {}: {e}", manifest.id)),
            }
        }
    } else if !nested.is_empty() {
        d.errors.push(format!(
            "mod {} nests archives more than {MAX_NESTING} levels deep",
            manifest.id
        ));
    }
    d.candidates.push(Candidate {
        manifest: manifest.clone(),
        origin: origin.clone(),
        condition,
    });
    d.archives.insert(origin, archive);
    for (child, path) in children {
        let origin = Origin::Nested {
            parent: manifest.id.clone(),
            path,
        };
        add(d, child, origin, LoadCondition::IfPossible, depth + 1);
    }
}

fn open(d: &mut Discovery, path: &Path, origin: Origin) {
    match fish_pack::read(path) {
        Ok(archive) => add(d, archive, origin, LoadCondition::Always, 0),
        Err(e) => d.errors.push(format!("{}: {e}", path.display())),
    }
}

/// Scans `<game dir>/fish-mods` and the extra development mods.
pub(crate) fn discover(game_dir: &Path, extra_mods: &[PathBuf]) -> Discovery {
    let mut d = Discovery::default();
    let mods_dir = game_dir.join("fish-mods");
    if let Ok(read) = std::fs::read_dir(&mods_dir) {
        let mut paths: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if is_hidden_or_disabled(name) {
                continue;
            }
            let is_mod = if path.is_dir() {
                path.join(fish_pack::MANIFEST_NAME).is_file()
            } else {
                name.ends_with(".fish")
            };
            if is_mod {
                open(&mut d, &path, Origin::Folder(path.clone()));
            }
        }
    }
    for path in extra_mods {
        if path.exists() {
            open(&mut d, path, Origin::Dev(path.clone()));
        } else {
            d.errors.push(format!(
                "{}: development mod path does not exist",
                path.display()
            ));
        }
    }
    d
}
