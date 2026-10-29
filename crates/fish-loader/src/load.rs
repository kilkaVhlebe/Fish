//! Loading a mod's native library and validating its export table.

use std::path::Path;

use fish_abi::{
    ENTRYPOINT_KIND_INIT, ENTRYPOINT_KIND_OBJECT, FISH_ABI_MAJOR, FISH_ABI_MINOR, FishEntrypointV1,
    FishModV1, MOD_SYMBOL_C, ModExportFn, is_compatible,
};
use fish_metadata::Manifest;

use crate::inner::EntryRec;

/// Entrypoint keys that run `init` functions.
pub(crate) const INIT_KEYS: [&str; 4] = ["pre_launch", "main", "client", "server"];

/// Opens a library with the safest flags for the platform.
fn open_library(path: &Path) -> Result<libloading::Library, String> {
    #[cfg(windows)]
    // SAFETY: loading a library runs its initialisers; mods are trusted code by design.
    let lib = unsafe {
        use libloading::os::windows::{
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, Library,
        };
        Library::load_with_flags(
            path,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
        .map(libloading::Library::from)
    };
    #[cfg(unix)]
    // SAFETY: as above.
    let lib = unsafe {
        use libloading::os::unix::{Library, RTLD_LOCAL, RTLD_NOW};
        Library::open(Some(path), RTLD_NOW | RTLD_LOCAL).map(libloading::Library::from)
    };
    lib.map_err(|e| format!("could not load {}: {e}", path.display()))
}

/// A loaded library with its verified entrypoints.
pub(crate) struct Loaded {
    pub library: libloading::Library,
    pub entries: Vec<EntryRec>,
}

/// Loads `path`, checks the ABI and that every entrypoint the manifest names is exported.
pub(crate) fn load(path: &Path, manifest: &Manifest) -> Result<Loaded, String> {
    let id = manifest.id.as_str();
    let library = open_library(path)?;
    // SAFETY: the symbol has the documented signature; a library without it is rejected.
    let export: libloading::Symbol<'_, ModExportFn> = unsafe { library.get(MOD_SYMBOL_C) }
        .map_err(|_| format!("{id}'s library does not export fish_mod_v1 (was it built with fish_api::export_mod!?)"))?;
    // SAFETY: `fish_mod_v1` takes no arguments and returns a pointer to static data.
    let table: *const FishModV1 = unsafe { export() };
    if table.is_null() {
        return Err(format!("{id}'s fish_mod_v1 returned null"));
    }
    // SAFETY: non-null; the ABI guarantees the table starts with `size`, `abi_major`, `abi_minor`.
    let header = unsafe { &*table };
    if !is_compatible(
        FISH_ABI_MAJOR,
        FISH_ABI_MINOR,
        header.abi_major,
        header.abi_minor,
    ) {
        return Err(format!(
            "{id} was built for Fish ABI {}.{}; this Fish serves {FISH_ABI_MAJOR}.{FISH_ABI_MINOR}",
            header.abi_major, header.abi_minor
        ));
    }
    if header.size < size_of::<FishModV1>() {
        return Err(format!(
            "{id}'s export table is too small ({} bytes)",
            header.size
        ));
    }
    let raw: &[FishEntrypointV1] = if header.entrypoint_count == 0 {
        &[]
    } else if header.entrypoints.is_null() {
        return Err(format!("{id}'s export table has a null entrypoint list"));
    } else {
        // SAFETY: the mod promises `entrypoint_count` valid entries for the life of the library.
        unsafe { std::slice::from_raw_parts(header.entrypoints, header.entrypoint_count) }
    };
    let mut entries = Vec::with_capacity(raw.len());
    for e in raw {
        // SAFETY: entry strings are static data in the mod.
        let (key, name) = unsafe { (e.key.as_str(), e.name.as_str()) };
        let (Ok(key), Ok(name)) = (key, name) else {
            return Err(format!(
                "{id}'s export table has an entrypoint that is not UTF-8"
            ));
        };
        let valid = match e.kind {
            ENTRYPOINT_KIND_INIT => e.init.is_some(),
            ENTRYPOINT_KIND_OBJECT => e.object.is_some(),
            _ => false,
        };
        if !valid {
            return Err(format!("{id}'s entrypoint {key}={name} is malformed"));
        }
        entries.push(EntryRec {
            key: key.to_string(),
            name: name.to_string(),
            init: e.init,
            object: e.object,
        });
    }
    for (key, names) in &manifest.entrypoints {
        for name in names {
            let Some(entry) = entries.iter().find(|e| &e.key == key && &e.name == name) else {
                return Err(format!(
                    "{id} declares entrypoint {key}={name} but its library does not export it"
                ));
            };
            let wants_init = INIT_KEYS.contains(&key.as_str());
            if wants_init && entry.init.is_none() {
                return Err(format!(
                    "{id} declares {key}={name}, which its library exports as an object entrypoint"
                ));
            }
            if !wants_init && entry.object.is_none() {
                return Err(format!(
                    "{id} declares {key}={name}, which its library exports as an init entrypoint"
                ));
            }
        }
    }
    Ok(Loaded { library, entries })
}
