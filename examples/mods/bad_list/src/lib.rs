//! Example mod: claims entrypoints but gives a null list.

use fish_abi::{FISH_ABI_MAJOR, FISH_ABI_MINOR, FishModV1};

static TABLE: FishModV1 = FishModV1 {
    size: size_of::<FishModV1>(),
    abi_major: FISH_ABI_MAJOR,
    abi_minor: FISH_ABI_MINOR,
    entrypoints: std::ptr::null(),
    entrypoint_count: 2,
};

#[unsafe(no_mangle)]
pub extern "C" fn fish_mod_v1() -> *const FishModV1 {
    &TABLE
}
