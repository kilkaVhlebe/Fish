//! Example mod: hand-written export table claiming ABI minor 999 (no fish-api involved).

use fish_abi::{FISH_ABI_MAJOR, FishModV1};

static TABLE: FishModV1 = FishModV1 {
    size: size_of::<FishModV1>(),
    abi_major: FISH_ABI_MAJOR,
    abi_minor: 999,
    entrypoints: std::ptr::null(),
    entrypoint_count: 0,
};

#[unsafe(no_mangle)]
pub extern "C" fn fish_mod_v1() -> *const FishModV1 {
    &TABLE
}
