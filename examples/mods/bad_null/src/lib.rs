//! Example mod: returns a null export table.

use fish_abi::FishModV1;

#[unsafe(no_mangle)]
pub extern "C" fn fish_mod_v1() -> *const FishModV1 {
    std::ptr::null()
}
