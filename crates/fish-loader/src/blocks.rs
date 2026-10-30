//! The blocks mods registered, the state ids Horizon gave them, and the Horizon content mods the
//! loader writes for them.

use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fish_abi::DropUserFn;
use fish_abi::blocks::BlockHandlerFn;
use fish_hooks::{BlockDef, BlockRun};
use fish_pack::FishArchive;
use serde::Serialize;

/// The user pointer of a handler with its destructor; dropping it runs the destructor once.
pub(crate) struct UserBox {
    ptr: *mut c_void,
    drop_user: Option<DropUserFn>,
}

// SAFETY: the pointer is only handed back to the mod's own handler, which is `Send + Sync` by the
// `fish-api` contract (behaviours are `Fn + Send + Sync`).
unsafe impl Send for UserBox {}
// SAFETY: see above.
unsafe impl Sync for UserBox {}

impl UserBox {
    /// Takes over `ptr` and its destructor.
    pub(crate) fn new(ptr: *mut c_void, drop_user: Option<DropUserFn>) -> UserBox {
        UserBox { ptr, drop_user }
    }

    /// The pointer the handler is called with.
    pub(crate) fn ptr(&self) -> *mut c_void {
        self.ptr
    }
}

impl Drop for UserBox {
    fn drop(&mut self) {
        if let Some(f) = self.drop_user {
            // SAFETY: `ptr` was handed over together with its destructor by `register`; this runs
            // once.
            unsafe { f(self.ptr) };
        }
    }
}

/// One registered block.
pub(crate) struct BlockEntry {
    /// What the block is.
    pub def: BlockDef,
    /// Token of the registering mod.
    pub owner: u64,
    /// The mod's handler, if it wants events.
    pub handler: Option<BlockHandlerFn>,
    /// The handler's user pointer.
    pub user: UserBox,
}

/// The states of one block.
struct Run {
    first: u32,
    count: u32,
    entry: Arc<BlockEntry>,
}

/// What [`BlockStore::set_runs`] found.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Built {
    /// Blocks of ours that got states.
    pub kept: usize,
    /// Lowest state of ours.
    pub first: u32,
    /// One past the highest state of ours.
    pub end: u32,
    /// Registered blocks Horizon did not add.
    pub missing: Vec<String>,
    /// Blocks whose states were not one run (only the first run is kept).
    pub repeated: Vec<String>,
}

/// Every block a loader knows.
#[derive(Default)]
pub(crate) struct BlockStore {
    blocks: Vec<Arc<BlockEntry>>,
    /// Sorted by first state.
    runs: Vec<Run>,
    /// The archives of the mods that registered blocks, for the content copy.
    archives: HashMap<u64, FishArchive>,
}

impl BlockStore {
    /// Adds a block; `Err` holds the token of the mod that registered the id first.
    pub(crate) fn register(&mut self, entry: BlockEntry) -> Result<(), u64> {
        if let Some(first) = self.blocks.iter().find(|b| b.def.id == entry.def.id) {
            return Err(first.owner);
        }
        self.blocks.push(Arc::new(entry));
        Ok(())
    }

    /// The blocks in registration order.
    pub(crate) fn entries(&self) -> Vec<Arc<BlockEntry>> {
        self.blocks.clone()
    }

    /// True when `token` registered a block.
    pub(crate) fn has_blocks_of(&self, token: u64) -> bool {
        self.blocks.iter().any(|b| b.owner == token)
    }

    /// Keeps `archive` as the source of the content copy of mod `token`.
    pub(crate) fn keep_archive(&mut self, token: u64, archive: FishArchive) {
        self.archives.insert(token, archive);
    }

    /// The kept archive of mod `token`.
    pub(crate) fn archive_mut(&mut self, token: u64) -> Option<&mut FishArchive> {
        self.archives.get_mut(&token)
    }

    /// Takes the runs Horizon gave to the registered blocks out of `runs`.
    pub(crate) fn set_runs(&mut self, runs: &[BlockRun]) -> Built {
        let mut kept: Vec<Run> = Vec::new();
        let mut repeated = Vec::new();
        for r in runs.iter().filter(|r| r.count > 0) {
            let Some(entry) = self.blocks.iter().find(|b| b.def.id == r.block) else {
                continue;
            };
            if kept.iter().any(|k| Arc::ptr_eq(&k.entry, entry)) {
                repeated.push(r.block.clone());
                continue;
            }
            kept.push(Run {
                first: r.first,
                count: r.count,
                entry: entry.clone(),
            });
        }
        kept.sort_by_key(|r| r.first);
        let missing = self
            .blocks
            .iter()
            .filter(|b| !kept.iter().any(|k| Arc::ptr_eq(&k.entry, b)))
            .map(|b| b.def.id.clone())
            .collect();
        let built = Built {
            kept: kept.len(),
            first: kept.first().map_or(0, |r| r.first),
            end: kept
                .iter()
                .map(|r| r.first.saturating_add(r.count))
                .max()
                .unwrap_or(0),
            missing,
            repeated,
        };
        self.runs = kept;
        built
    }

    /// The block `state` belongs to, with the first state of its run.
    pub(crate) fn find(&self, state: u32) -> Option<(Arc<BlockEntry>, u32)> {
        let at = self.runs.partition_point(|r| r.first <= state);
        let run = self.runs.get(at.checked_sub(1)?)?;
        (state - run.first < run.count).then(|| (run.entry.clone(), run.first))
    }

    /// The first state and the state count of block `id`, once the runs are known.
    pub(crate) fn range(&self, id: &str) -> Option<(u32, u32)> {
        self.runs
            .iter()
            .find(|r| r.entry.def.id == id)
            .map(|r| (r.first, r.count))
    }

    /// Drops every block (and so every user pointer), run and archive.
    pub(crate) fn clear(&mut self) {
        self.runs.clear();
        self.blocks.clear();
        self.archives.clear();
    }
}

fn path_chars_ok(path: &str) -> bool {
    !path.is_empty()
        && path
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'/' | b'-'))
}

/// Checks that `id` is `<mod_id>:<path>` with a path Horizon accepts; `Err` is the reason.
pub(crate) fn check_id(id: &str, mod_id: &str) -> Result<(), String> {
    let Some((namespace, path)) = id.split_once(':') else {
        return Err("the id must be namespace:path".to_string());
    };
    if namespace != mod_id {
        return Err(format!(
            "the namespace {namespace:?} is not the id of the mod ({mod_id})"
        ));
    }
    if !mod_id
        .bytes()
        .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
    {
        return Err(format!(
            "the mod id {mod_id:?} is not a namespace (use a-z, 0-9 and _)"
        ));
    }
    if !path_chars_ok(path) {
        return Err("the path must be made of a-z, 0-9 and _ . / -".to_string());
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part.bytes().all(|b| b == b'.'))
    {
        return Err("the path has an empty or dotted part".to_string());
    }
    Ok(())
}

fn check_minecraft(what: &str, id: &str) -> Result<(), String> {
    match id.strip_prefix("minecraft:") {
        Some(path) if path_chars_ok(path) => Ok(()),
        _ => Err(format!("the {what} {id:?} must be minecraft:<path>")),
    }
}

/// Checks the template, the numbers and the sounds of `def`; `Err` is the reason.
pub(crate) fn check_def(def: &BlockDef) -> Result<(), String> {
    check_minecraft("template", &def.template)?;
    if let Some(s) = &def.sounds {
        check_minecraft("sounds", s)?;
    }
    if let Some(h) = def.hardness
        && !(h.is_finite() && (h >= 0.0 || h.to_bits() == (-1.0f32).to_bits()))
    {
        return Err(format!("the hardness {h} is not 0 or more, or -1"));
    }
    if let Some(r) = def.resistance
        && !(r.is_finite() && r >= 0.0)
    {
        return Err(format!("the resistance {r} is not 0 or more"));
    }
    if let Some(f) = def.friction
        && !(f.is_finite() && (0.0..=2.0).contains(&f))
    {
        return Err(format!("the friction {f} is not between 0 and 2"));
    }
    if let Some(l) = def.light
        && l > 15
    {
        return Err(format!("the light {l} is more than 15"));
    }
    if let Some(c) = def.map_color
        && c > 61
    {
        return Err(format!("the map colour {c} is more than 61"));
    }
    Ok(())
}

#[derive(Serialize)]
struct Properties<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    hardness: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    explosion_resistance: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    light: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requires_tool: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    friction: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    map_color: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sounds: Option<&'a str>,
}

#[derive(Serialize)]
struct BlockJson<'a> {
    template: &'a str,
    properties: Properties<'a>,
    item: bool,
}

/// The bytes of Horizon's `horizon/block/<path>.json` for `def`.
pub(crate) fn block_json(def: &BlockDef) -> Vec<u8> {
    let json = BlockJson {
        template: &def.template,
        properties: Properties {
            hardness: def.hardness,
            explosion_resistance: def.resistance,
            light: def.light,
            requires_tool: def.requires_tool,
            friction: def.friction,
            map_color: def.map_color,
            sounds: def.sounds.as_deref(),
        },
        item: true,
    };
    let mut bytes = serde_json::to_vec_pretty(&json).expect("a block serializes");
    bytes.push(b'\n');
    bytes
}

/// What a content mod is named after.
pub(crate) struct ContentMod<'a> {
    /// The mod's id (also the namespace of its blocks).
    pub id: &'a str,
    /// Display name.
    pub name: &'a str,
    /// Version text.
    pub version: &'a str,
    /// Description (may be empty).
    pub description: &'a str,
}

fn toml_str(s: &str) -> String {
    serde_json::to_string(s)
        .expect("a string serializes")
        .replace('\u{7f}', "\\u007f")
}

/// The bytes of the `horizon-mod.toml` of a content mod.
pub(crate) fn horizon_manifest(m: &ContentMod<'_>) -> Vec<u8> {
    let description = if m.description.is_empty() {
        format!("Blocks of the Fish mod {}", m.name)
    } else {
        m.description.to_string()
    };
    format!(
        "id = {}\nname = {}\nversion = {}\ndescription = {}\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n",
        toml_str(m.id),
        toml_str(m.name),
        toml_str(m.version),
        toml_str(&description),
    )
    .into_bytes()
}

fn content_path_ok(name: &str) -> bool {
    name.split('/').all(|part| {
        !part.is_empty()
            && !part.bytes().all(|b| b == b'.')
            && part
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-'))
    })
}

/// How many skipped names a warning lists.
const LISTED: usize = 5;

fn describe(names: &[String]) -> String {
    let mut text = names
        .iter()
        .take(LISTED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > LISTED {
        text.push_str(&format!(", and {} more", names.len() - LISTED));
    }
    text
}

/// Everything the content folder of a mod holds, by path relative to it; and the warnings about
/// what could not be copied.
pub(crate) fn content_files(
    m: &ContentMod<'_>,
    defs: &[&BlockDef],
    archive: Option<&mut FishArchive>,
) -> (BTreeMap<String, Vec<u8>>, Vec<String>) {
    let mut files = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut generated = Vec::new();
    files.insert("horizon-mod.toml".to_string(), horizon_manifest(m));
    for def in defs {
        let path = def
            .id
            .split_once(':')
            .map_or(def.id.as_str(), |(_, path)| path);
        let name = format!("data/{}/horizon/block/{path}.json", m.id);
        files.insert(name.clone(), block_json(def));
        generated.push(name);
    }
    let Some(archive) = archive else {
        return (files, warnings);
    };
    let (mut foreign, mut invalid, mut unreadable) = (Vec::new(), Vec::new(), Vec::new());
    let names: Vec<String> = archive
        .entries()
        .iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.name.clone())
        .collect();
    for name in names {
        let Some((kind, rest)) = name.split_once('/') else {
            continue;
        };
        if kind != "assets" && kind != "data" {
            continue;
        }
        if generated.contains(&name) {
            continue;
        }
        let (namespace, below) = rest.split_once('/').unwrap_or((rest, ""));
        let tags =
            kind == "data" && matches!(namespace, "minecraft" | "c") && below.starts_with("tags/");
        if namespace != m.id && !tags {
            foreign.push(name);
            continue;
        }
        if !content_path_ok(&name) {
            invalid.push(name);
            continue;
        }
        match archive.read_entry(&name) {
            Ok(bytes) => {
                files.insert(name, bytes);
            }
            Err(e) => unreadable.push(format!("{name} ({e})")),
        }
    }
    let who = m.name;
    if !foreign.is_empty() {
        warnings.push(format!(
            "{} file(s) of {who} are not in its own namespace ({}) or in the tag folders, and were not copied: {}",
            foreign.len(),
            m.id,
            describe(&foreign)
        ));
    }
    if !invalid.is_empty() {
        warnings.push(format!(
            "{} file(s) of {who} have names Horizon refuses (use a-z, 0-9 and _ . - ) and were not copied: {}",
            invalid.len(),
            describe(&invalid)
        ));
    }
    if !unreadable.is_empty() {
        warnings.push(format!(
            "{} file(s) of {who} could not be read and were not copied: {}",
            unreadable.len(),
            describe(&unreadable)
        ));
    }
    (files, warnings)
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    }
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
}

/// Removes what is in `dir` and not in `keep` (paths relative to `root`); folders left empty go.
fn prune(root: &Path, dir: &Path, keep: &BTreeMap<String, Vec<u8>>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            prune(root, &path, keep)?;
            if std::fs::read_dir(&path)?.next().is_none() {
                std::fs::remove_dir(&path)?;
            }
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map(|p| {
                p.components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default();
        if !keep.contains_key(&relative) {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Writes the content mod of `m` into `<content_root>/<id>`: files whose bytes differ, and
/// removes the files that no longer belong. Returns the folder and the warnings.
pub(crate) fn write_content(
    content_root: &Path,
    m: &ContentMod<'_>,
    defs: &[&BlockDef],
    archive: Option<&mut FishArchive>,
) -> Result<(PathBuf, Vec<String>), String> {
    let folder = content_root.join(m.id);
    let (files, warnings) = content_files(m, defs, archive);
    let fail = |what: &str, e: io::Error| format!("could not {what} {}: {e}", folder.display());
    std::fs::create_dir_all(&folder).map_err(|e| fail("create", e))?;
    for (name, bytes) in &files {
        write_if_changed(&folder.join(name), bytes).map_err(|e| fail("write into", e))?;
    }
    prune(&folder, &folder, &files).map_err(|e| fail("clean", e))?;
    Ok((folder, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn def(id: &str) -> BlockDef {
        BlockDef {
            id: id.to_string(),
            mod_id: id.split(':').next().unwrap().to_string(),
            template: "minecraft:stone".to_string(),
            hardness: None,
            resistance: None,
            light: None,
            requires_tool: None,
            friction: None,
            map_color: None,
            sounds: None,
            events: 0,
        }
    }

    fn entry(id: &str, owner: u64) -> BlockEntry {
        BlockEntry {
            def: def(id),
            owner,
            handler: None,
            user: UserBox::new(std::ptr::null_mut(), None),
        }
    }

    fn run(block: &str, first: u32, count: u32) -> BlockRun {
        BlockRun {
            block: block.to_string(),
            first,
            count,
        }
    }

    #[test]
    fn ids_are_the_mods_namespace_and_a_clean_path() {
        assert_eq!(check_id("ruby:block", "ruby"), Ok(()));
        assert_eq!(check_id("ruby:a/b.c-d_1", "ruby"), Ok(()));
        for (id, why) in [
            ("ruby", "namespace:path"),
            ("other:block", "is not the id of the mod"),
            ("ruby:", "the path must be"),
            ("ruby:Block", "the path must be"),
            ("ruby:a b", "the path must be"),
            ("ruby:a//b", "empty or dotted"),
            ("ruby:/a", "empty or dotted"),
            ("ruby:a/", "empty or dotted"),
            ("ruby:a/../b", "empty or dotted"),
            ("ruby:.", "empty or dotted"),
        ] {
            let err = check_id(id, "ruby").unwrap_err();
            assert!(err.contains(why), "{id}: {err}");
        }
        let err = check_id("my-mod:block", "my-mod").unwrap_err();
        assert!(err.contains("is not a namespace"), "{err}");
    }

    #[test]
    fn definitions_are_checked_like_horizon_does() {
        assert_eq!(check_def(&def("a:b")), Ok(()));
        let with = |f: fn(&mut BlockDef)| {
            let mut d = def("a:b");
            f(&mut d);
            check_def(&d)
        };
        assert_eq!(with(|d| d.hardness = Some(-1.0)), Ok(()));
        assert_eq!(with(|d| d.hardness = Some(0.0)), Ok(()));
        assert_eq!(with(|d| d.resistance = Some(0.0)), Ok(()));
        assert_eq!(with(|d| d.friction = Some(2.0)), Ok(()));
        assert_eq!(with(|d| d.friction = Some(0.0)), Ok(()));
        assert_eq!(with(|d| d.light = Some(15)), Ok(()));
        assert_eq!(with(|d| d.map_color = Some(61)), Ok(()));
        assert_eq!(
            with(|d| d.sounds = Some("minecraft:block.stone".into())),
            Ok(())
        );
        for (result, why) in [
            (with(|d| d.template = "stone".into()), "template"),
            (with(|d| d.template = "minecraft:".into()), "template"),
            (with(|d| d.template = "minecraft:Stone".into()), "template"),
            (with(|d| d.template = "other:stone".into()), "template"),
            (with(|d| d.sounds = Some("stone".into())), "sounds"),
            (with(|d| d.hardness = Some(-2.0)), "hardness"),
            (with(|d| d.hardness = Some(f32::NAN)), "hardness"),
            (with(|d| d.hardness = Some(f32::INFINITY)), "hardness"),
            (with(|d| d.resistance = Some(-0.5)), "resistance"),
            (with(|d| d.resistance = Some(f32::NAN)), "resistance"),
            (with(|d| d.friction = Some(2.5)), "friction"),
            (with(|d| d.friction = Some(-0.1)), "friction"),
            (with(|d| d.friction = Some(f32::NAN)), "friction"),
            (with(|d| d.light = Some(16)), "light"),
            (with(|d| d.map_color = Some(62)), "map colour"),
        ] {
            let err = result.unwrap_err();
            assert!(err.contains(why), "{why}: {err}");
        }
    }

    #[test]
    fn the_store_refuses_a_second_block_of_an_id() {
        let mut store = BlockStore::default();
        assert_eq!(store.register(entry("a:x", 1)), Ok(()));
        assert_eq!(store.register(entry("a:x", 2)), Err(1));
        assert_eq!(store.register(entry("a:y", 2)), Ok(()));
        let ids: Vec<String> = store.entries().iter().map(|e| e.def.id.clone()).collect();
        assert_eq!(ids, ["a:x", "a:y"]);
        assert!(store.has_blocks_of(1) && store.has_blocks_of(2) && !store.has_blocks_of(3));
    }

    #[test]
    fn runs_are_found_by_state_including_multi_state_blocks_and_edges() {
        let mut store = BlockStore::default();
        for id in ["a:one", "a:slab", "a:three", "a:absent"] {
            store.register(entry(id, 1)).unwrap();
        }
        assert!(store.find(100).is_none());
        assert_eq!(store.range("a:one"), None);
        let built = store.set_runs(&[
            run("a:three", 110, 1),
            run("minecraft:stone", 5, 1),
            run("a:one", 100, 1),
            run("a:slab", 101, 3),
            run("other:foreign", 104, 2),
            run("a:slab", 120, 3),
            run("a:one", 130, 0),
        ]);
        assert_eq!(
            built,
            Built {
                kept: 3,
                first: 100,
                end: 111,
                missing: vec!["a:absent".to_string()],
                repeated: vec!["a:slab".to_string()],
            }
        );
        let id_at = |state| {
            store
                .find(state)
                .map(|(e, first)| (e.def.id.clone(), first))
        };
        assert_eq!(id_at(99), None);
        assert_eq!(id_at(100), Some(("a:one".to_string(), 100)));
        assert_eq!(id_at(101), Some(("a:slab".to_string(), 101)));
        assert_eq!(id_at(103), Some(("a:slab".to_string(), 101)));
        assert_eq!(id_at(104), None);
        assert_eq!(id_at(109), None);
        assert_eq!(id_at(110), Some(("a:three".to_string(), 110)));
        assert_eq!(id_at(111), None);
        assert_eq!(id_at(u32::MAX), None);
        assert_eq!(store.range("a:slab"), Some((101, 3)));
        assert_eq!(store.range("a:absent"), None);
        store.clear();
        assert!(store.find(100).is_none() && store.entries().is_empty());
    }

    #[test]
    fn no_runs_means_nothing_found() {
        let mut store = BlockStore::default();
        assert_eq!(store.set_runs(&[]), Built::default());
        store.register(entry("a:x", 1)).unwrap();
        let built = store.set_runs(&[]);
        assert_eq!((built.kept, built.end), (0, 0));
        assert_eq!(built.missing, ["a:x"]);
    }

    #[test]
    fn dropping_the_store_drops_each_user_pointer_once() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static DROPPED: AtomicU32 = AtomicU32::new(0);
        unsafe extern "C" fn count(_: *mut c_void) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
        let mut store = BlockStore::default();
        for id in ["a:x", "a:y"] {
            let mut e = entry(id, 1);
            e.user = UserBox::new(std::ptr::null_mut(), Some(count));
            store.register(e).unwrap();
        }
        let refused = BlockEntry {
            user: UserBox::new(std::ptr::null_mut(), Some(count)),
            ..entry("a:x", 1)
        };
        assert!(store.register(refused).is_err());
        assert_eq!(DROPPED.load(Ordering::SeqCst), 1, "a refused block");
        store.clear();
        assert_eq!(DROPPED.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn block_json_holds_only_what_was_set() {
        let plain = String::from_utf8(block_json(&def("a:x"))).unwrap();
        let v: serde_json::Value = serde_json::from_str(&plain).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"template": "minecraft:stone", "properties": {}, "item": true})
        );
        let mut d = def("a:x");
        d.hardness = Some(0.3);
        d.resistance = Some(6.0);
        d.light = Some(7);
        d.requires_tool = Some(true);
        d.friction = Some(0.98);
        d.map_color = Some(14);
        d.sounds = Some("minecraft:block.amethyst_block".into());
        let text = String::from_utf8(block_json(&d)).unwrap();
        assert!(text.contains("\"hardness\": 0.3"), "{text}");
        assert!(text.contains("\"friction\": 0.98"), "{text}");
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let p = &v["properties"];
        assert_eq!(p["explosion_resistance"], 6.0);
        assert_eq!(p["light"], 7);
        assert_eq!(p["requires_tool"], true);
        assert_eq!(p["map_color"], 14);
        assert_eq!(p["sounds"], "minecraft:block.amethyst_block");
        assert_eq!(v["item"], true);
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn the_horizon_manifest_is_valid_toml_text() {
        let m = ContentMod {
            id: "ruby",
            name: "Ruby \"Mod\"",
            version: "1.2.3",
            description: "",
        };
        let text = String::from_utf8(horizon_manifest(&m)).unwrap();
        assert_eq!(
            text,
            "id = \"ruby\"\nname = \"Ruby \\\"Mod\\\"\"\nversion = \"1.2.3\"\ndescription = \"Blocks of the Fish mod Ruby \\\"Mod\\\"\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n"
        );
        let m = ContentMod {
            description: "Gems\u{7f}",
            ..m
        };
        let text = String::from_utf8(horizon_manifest(&m)).unwrap();
        assert!(text.contains("description = \"Gems\\u007f\""), "{text}");
    }

    fn archive(dir: &Path, files: &[(&str, &[u8])]) -> FishArchive {
        let mut spec = fish_pack::PackSpec::new("schema = 1\nid = \"ruby\"\nversion = \"1.0.0\"\n");
        for (name, bytes) in files {
            spec.add_bytes(name, bytes.to_vec()).unwrap();
        }
        let path = dir.join("ruby.fish");
        fish_pack::write(spec, &path).unwrap();
        fish_pack::read(&path).unwrap()
    }

    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.push(
                    p.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    const MOD: ContentMod<'static> = ContentMod {
        id: "ruby",
        name: "Ruby",
        version: "1.0.0",
        description: "Gems",
    };

    #[test]
    fn the_content_folder_gets_generated_and_copied_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut arc = archive(
            tmp.path(),
            &[
                ("assets/ruby/lang/en_us.json", b"{}"),
                ("assets/ruby/textures/block/ruby.png", b"png"),
                ("data/ruby/loot_table/ruby.json", b"loot"),
                (
                    "data/ruby/horizon/block/ruby_block.json",
                    b"ignored: generated",
                ),
                ("data/minecraft/tags/block/mineable.json", b"tag"),
                ("data/c/tags/item/gems.json", b"gems"),
                ("data/minecraft/recipe/x.json", b"foreign"),
                ("assets/other/lang/en_us.json", b"foreign"),
                ("assets/ruby/Upper.png", b"bad"),
                ("docs/readme.txt", b"not content"),
            ],
        );
        let blocks = [def("ruby:ruby_block"), def("ruby:deep/gem")];
        let defs: Vec<&BlockDef> = blocks.iter().collect();
        let root = tmp.path().join("content");
        let (folder, warnings) = write_content(&root, &MOD, &defs, Some(&mut arc)).unwrap();
        assert_eq!(folder, root.join("ruby"));
        assert_eq!(
            listing(&folder),
            [
                "assets/ruby/lang/en_us.json",
                "assets/ruby/textures/block/ruby.png",
                "data/c/tags/item/gems.json",
                "data/minecraft/tags/block/mineable.json",
                "data/ruby/horizon/block/deep/gem.json",
                "data/ruby/horizon/block/ruby_block.json",
                "data/ruby/loot_table/ruby.json",
                "horizon-mod.toml",
            ]
        );
        assert_eq!(
            std::fs::read(folder.join("data/ruby/horizon/block/ruby_block.json")).unwrap(),
            block_json(&blocks[0])
        );
        assert_eq!(
            std::fs::read(folder.join("assets/ruby/lang/en_us.json")).unwrap(),
            b"{}"
        );
        assert_eq!(
            std::fs::read(folder.join("horizon-mod.toml")).unwrap(),
            horizon_manifest(&MOD)
        );
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(
            warnings[0].contains("not in its own namespace") && warnings[0].contains("2 file(s)")
        );
        assert!(warnings[0].contains("assets/other/lang/en_us.json"));
        assert!(warnings[1].contains("names Horizon refuses") && warnings[1].contains("Upper.png"));
    }

    #[test]
    fn unchanged_files_are_not_rewritten_and_stale_ones_go() {
        let tmp = tempfile::tempdir().unwrap();
        let mut arc = archive(tmp.path(), &[("assets/ruby/a.txt", b"one")]);
        let blocks = [def("ruby:x")];
        let defs: Vec<&BlockDef> = blocks.iter().collect();
        let root = tmp.path().join("content");
        let (folder, _) = write_content(&root, &MOD, &defs, Some(&mut arc)).unwrap();
        let a = folder.join("assets/ruby/a.txt");
        let stamp = std::fs::metadata(&a).unwrap().modified().unwrap();

        // leftovers: a file, a nested one that empties its folder, and a file where a folder goes
        std::fs::write(folder.join("old.txt"), b"x").unwrap();
        std::fs::create_dir_all(folder.join("assets/ruby/gone/deeper")).unwrap();
        std::fs::write(folder.join("assets/ruby/gone/deeper/f.png"), b"x").unwrap();
        std::fs::remove_file(folder.join("data/ruby/horizon/block/x.json")).unwrap();
        std::fs::create_dir_all(folder.join("data/ruby/horizon/block/x.json")).unwrap();
        std::fs::write(folder.join("data/ruby/horizon/block/x.json/inner"), b"x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));

        let (again, _) = write_content(&root, &MOD, &defs, Some(&mut arc)).unwrap();
        assert_eq!(again, folder);
        assert_eq!(std::fs::metadata(&a).unwrap().modified().unwrap(), stamp);
        assert_eq!(
            listing(&folder),
            [
                "assets/ruby/a.txt",
                "data/ruby/horizon/block/x.json",
                "horizon-mod.toml"
            ]
        );
        assert!(!folder.join("assets/ruby/gone").exists());

        // a changed block rewrites only its file
        let mut changed = def("ruby:x");
        changed.light = Some(5);
        let json = folder.join("data/ruby/horizon/block/x.json");
        let before = std::fs::read(&json).unwrap();
        write_content(&root, &MOD, &[&changed], Some(&mut arc)).unwrap();
        assert_ne!(std::fs::read(&json).unwrap(), before);
        assert_eq!(std::fs::metadata(&a).unwrap().modified().unwrap(), stamp);
    }

    #[test]
    fn a_mod_without_a_kept_archive_still_gets_its_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let blocks = [def("ruby:x")];
        let defs: Vec<&BlockDef> = blocks.iter().collect();
        let (folder, warnings) = write_content(tmp.path(), &MOD, &defs, None).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(
            listing(&folder),
            ["data/ruby/horizon/block/x.json", "horizon-mod.toml"]
        );
    }

    #[test]
    fn a_folder_that_cannot_be_written_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("content");
        std::fs::write(&file, b"a file where the folder goes").unwrap();
        let err = write_content(&file, &MOD, &[], None).unwrap_err();
        assert!(err.contains("could not create"), "{err}");
    }

    #[test]
    fn long_lists_of_skipped_files_are_cut() {
        let names: Vec<String> = (0..8).map(|i| format!("f{i}")).collect();
        assert_eq!(describe(&names), "f0, f1, f2, f3, f4, and 3 more");
        assert_eq!(describe(&names[..2]), "f0, f1");
    }
}
