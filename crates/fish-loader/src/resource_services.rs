//! Host side of `fish:resource`: a mod's own resource files from its archive, and the files of the
//! game's active resource root.

use std::ffi::c_void;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};

use fish_abi::client::StrSink;
use fish_abi::resources::{BytesSink, MAX_FILE, ResourceServiceV1, valid_path};
use fish_abi::{FishHandle, FishServiceHeader, FishStatus, FishStr};

use crate::inner::Inner;

fn guard(f: impl FnOnce() -> FishStatus) -> FishStatus {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(FishStatus::PANICKED)
}

/// Hands `bytes` to the mod's sink.
///
/// # Safety
/// `sink` and `user` come from the mod for this call.
unsafe fn give(sink: BytesSink, user: *mut c_void, bytes: &[u8]) {
    // SAFETY: the caller's contract; the bytes live through the call.
    unsafe { sink(user, bytes.as_ptr(), bytes.len()) };
}

unsafe extern "C" fn read_own(
    host: *mut c_void,
    token: u64,
    path: FishStr,
    sink: BytesSink,
    user: *mut c_void,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`; `path` is valid for the call.
        let inner = unsafe { &*host.cast::<Inner>() };
        let Ok(path) = (unsafe { path.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if inner.record(token).is_none() || !valid_path(path) {
            return FishStatus::INVALID_ARGUMENT;
        }
        let bytes = {
            let mut store = inner.blocks.lock().unwrap_or_else(|e| e.into_inner());
            let Some(archive) = store.archive_mut(token) else {
                return FishStatus::NOT_FOUND;
            };
            match archive
                .entries()
                .iter()
                .find(|e| !e.is_dir && e.name == path)
            {
                None => return FishStatus::NOT_FOUND,
                Some(e) if e.size > MAX_FILE => return FishStatus::INVALID_ARGUMENT,
                Some(_) => {}
            }
            match archive.read_entry(path) {
                Ok(bytes) => bytes,
                Err(_) => return FishStatus::NOT_FOUND,
            }
        };
        // SAFETY: the mod's sink for this call; the store's lock is released.
        unsafe { give(sink, user, &bytes) };
        FishStatus::OK
    })
}

unsafe extern "C" fn list_own(
    host: *mut c_void,
    token: u64,
    prefix: FishStr,
    sink: StrSink,
    user: *mut c_void,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let inner = unsafe { &*host.cast::<Inner>() };
        let Ok(prefix) = (unsafe { prefix.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if inner.record(token).is_none() {
            return FishStatus::INVALID_ARGUMENT;
        }
        let names: Vec<String> = {
            let mut store = inner.blocks.lock().unwrap_or_else(|e| e.into_inner());
            store
                .archive_mut(token)
                .map(|a| {
                    a.entries()
                        .iter()
                        .filter(|e| !e.is_dir && valid_path(&e.name) && e.name.starts_with(prefix))
                        .map(|e| e.name.clone())
                        .collect()
                })
                .unwrap_or_default()
        };
        for name in &names {
            // SAFETY: the mod's sink for this call; the string lives through it.
            unsafe { sink(user, FishStr::new(name)) };
        }
        FishStatus::OK
    })
}

unsafe extern "C" fn read_game(
    host: *mut c_void,
    view: FishHandle,
    path: FishStr,
    sink: BytesSink,
    user: *mut c_void,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let inner = unsafe { &*host.cast::<Inner>() };
        let Ok(path) = (unsafe { path.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if !valid_path(path) {
            return FishStatus::INVALID_ARGUMENT;
        }
        let root = match inner.handles.client(view, |c| c.resource_root()) {
            Ok(Some(root)) => root,
            Ok(None) => return FishStatus::UNSUPPORTED,
            Err(status) => return status,
        };
        let Ok(file) = std::fs::File::open(root.join(path)) else {
            return FishStatus::NOT_FOUND;
        };
        let mut bytes = Vec::new();
        if file.take(MAX_FILE + 1).read_to_end(&mut bytes).is_err() {
            return FishStatus::NOT_FOUND;
        }
        if bytes.len() as u64 > MAX_FILE {
            return FishStatus::INVALID_ARGUMENT;
        }
        // SAFETY: the mod's sink for this call.
        unsafe { give(sink, user, &bytes) };
        FishStatus::OK
    })
}

/// The `fish:resource` table.
pub(crate) static RESOURCE: ResourceServiceV1 = ResourceServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ResourceServiceV1>(),
        version: 1,
    },
    read_own,
    list_own,
    read_game,
};

#[cfg(test)]
mod tests {

    use std::path::PathBuf;

    use fish_hooks::{ClientView, PlayerSnapshot, Role};
    use fish_metadata::Side;

    use super::*;
    use crate::boot::{BootConfig, Loader, boot};

    /// A loader with `ruby` (resource files) and `gem` (none), its host pointer and directory.
    fn host() -> (Loader, *mut c_void, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("fish-mods");
        std::fs::create_dir_all(&mods).unwrap();
        for (id, files) in [
            (
                "ruby",
                vec![
                    ("assets/ruby/sounds.json", b"{\"a\":{}}".as_slice()),
                    ("assets/ruby/lang/en_us.json", b"{}".as_slice()),
                    ("data/ruby/loot_table/x.json", b"[]".as_slice()),
                ],
            ),
            ("gem", Vec::new()),
        ] {
            let mut spec = fish_pack::PackSpec::new(format!(
                "schema = 1\nid = \"{id}\"\nversion = \"1.0.0\"\nname = \"{id}\"\n"
            ));
            for (name, bytes) in files {
                spec.add_bytes(name, bytes.to_vec()).unwrap();
            }
            fish_pack::write(spec, &mods.join(format!("{id}.fish"))).unwrap();
        }
        let mut cfg = BootConfig::new(Side::Client, dir.path());
        cfg.log_sink = Some(std::sync::Arc::new(|_| {}));
        let loader = boot(cfg).unwrap();
        let ptr = std::sync::Arc::as_ptr(&loader.inner)
            .cast_mut()
            .cast::<c_void>();
        (loader, ptr, dir)
    }

    fn token(loader: &Loader, id: &str) -> u64 {
        loader
            .inner
            .records()
            .iter()
            .find(|r| r.manifest.id.as_str() == id)
            .unwrap()
            .token
    }

    unsafe extern "C" fn collect_bytes(user: *mut c_void, data: *const u8, len: usize) {
        // SAFETY: `user` is the test's `Vec<u8>`; the data is valid for `len` bytes.
        unsafe {
            (*user.cast::<Vec<u8>>()).extend_from_slice(std::slice::from_raw_parts(data, len))
        };
    }

    unsafe extern "C" fn collect_name(user: *mut c_void, name: FishStr) {
        // SAFETY: `user` is the test's `Vec<String>`; the string is valid for the call.
        unsafe { (*user.cast::<Vec<String>>()).push(name.as_str().unwrap().to_string()) };
    }

    fn own(h: *mut c_void, t: u64, path: &str) -> (FishStatus, Vec<u8>) {
        let mut out = Vec::new();
        // SAFETY: the table's own function with valid arguments.
        let status = unsafe {
            read_own(
                h,
                t,
                FishStr::new(path),
                collect_bytes,
                (&raw mut out).cast(),
            )
        };
        (status, out)
    }

    #[test]
    fn a_mod_reads_and_lists_its_own_files() {
        let (loader, h, _dir) = host();
        let (ruby, gem) = (token(&loader, "ruby"), token(&loader, "gem"));
        assert_eq!(
            own(h, ruby, "assets/ruby/sounds.json"),
            (FishStatus::OK, b"{\"a\":{}}".to_vec())
        );
        assert_eq!(
            own(h, ruby, "assets/ruby/none.json").0,
            FishStatus::NOT_FOUND
        );
        assert_eq!(
            own(h, ruby, "fish.mod.toml").0,
            FishStatus::INVALID_ARGUMENT
        );
        assert_eq!(own(h, ruby, "assets/../x").0, FishStatus::INVALID_ARGUMENT);
        assert_eq!(own(h, gem, "assets/gem/x.json").0, FishStatus::NOT_FOUND);
        assert_eq!(
            own(h, 999, "assets/ruby/sounds.json").0,
            FishStatus::INVALID_ARGUMENT
        );
        let list = |t: u64, prefix: &str| -> (FishStatus, Vec<String>) {
            let mut names = Vec::new();
            // SAFETY: the table's own function with valid arguments.
            let status = unsafe {
                list_own(
                    h,
                    t,
                    FishStr::new(prefix),
                    collect_name,
                    (&raw mut names).cast(),
                )
            };
            (status, names)
        };
        let (status, names) = list(ruby, "assets/");
        assert_eq!(status, FishStatus::OK);
        assert_eq!(
            names,
            ["assets/ruby/lang/en_us.json", "assets/ruby/sounds.json"]
        );
        assert_eq!(list(ruby, "").1.len(), 3);
        assert!(list(gem, "").1.is_empty());
        assert_eq!(list(999, "").0, FishStatus::INVALID_ARGUMENT);
        // ruby has a content folder without blocks: its resources reach the game
        let folders = loader.inner.blocks_content();
        assert_eq!(folders.len(), 1);
        assert!(folders[0].join("data/ruby/loot_table/x.json").is_file());
    }

    struct Game(Option<PathBuf>);
    impl ClientView for Game {
        fn role(&self) -> Role {
            Role::Window
        }
        fn player(&self) -> Option<PlayerSnapshot> {
            None
        }
        fn show_chat(&mut self, _: &str) {}
        fn game_dir(&self) -> PathBuf {
            PathBuf::new()
        }
        fn resource_root(&self) -> Option<PathBuf> {
            self.0.clone()
        }
    }

    #[test]
    fn the_game_root_is_read_through_the_view() {
        let (loader, h, dir) = host();
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("assets/minecraft/lang")).unwrap();
        std::fs::write(
            root.join("assets/minecraft/lang/en_us.json"),
            b"{\"k\":\"v\"}",
        )
        .unwrap();
        let read = |view: FishHandle, path: &str| -> (FishStatus, Vec<u8>) {
            let mut out = Vec::new();
            // SAFETY: the table's own function with valid arguments.
            let status = unsafe {
                read_game(
                    h,
                    view,
                    FishStr::new(path),
                    collect_bytes,
                    (&raw mut out).cast(),
                )
            };
            (status, out)
        };
        let mut game = Game(Some(root));
        let got = loader.inner.handles.client_scope(&mut game, |view| {
            (
                read(view, "assets/minecraft/lang/en_us.json"),
                read(view, "assets/minecraft/lang/xx.json").0,
                read(view, "../escape").0,
            )
        });
        assert_eq!(got.0, (FishStatus::OK, b"{\"k\":\"v\"}".to_vec()));
        assert_eq!(got.1, FishStatus::NOT_FOUND);
        assert_eq!(got.2, FishStatus::INVALID_ARGUMENT);
        let mut rootless = Game(None);
        let status = loader
            .inner
            .handles
            .client_scope(&mut rootless, |view| read(view, "assets/m/x").0);
        assert_eq!(status, FishStatus::UNSUPPORTED);
        // a handle outside any callback
        assert_ne!(
            read(crate::block_services::NO_GAME, "assets/m/x").0,
            FishStatus::OK
        );
    }
}
