//! Example mod: exports an entrypoint whose kind does not match its function pointers.

use fish_abi::{FISH_ABI_MAJOR, FISH_ABI_MINOR, FishEntrypointV1, FishModV1, FishStr};

static ENTRIES: [FishEntrypointV1; 1] = [FishEntrypointV1 {
    key: FishStr::from_static("main"),
    name: FishStr::from_static("init"),
    kind: 9,
    init: None,
    object: None,
}];

static TABLE: FishModV1 = FishModV1 {
    size: size_of::<FishModV1>(),
    abi_major: FISH_ABI_MAJOR,
    abi_minor: FISH_ABI_MINOR,
    entrypoints: ENTRIES.as_ptr(),
    entrypoint_count: 1,
};

#[unsafe(no_mangle)]
pub extern "C" fn fish_mod_v1() -> *const FishModV1 {
    &TABLE
}
