//! Example mod: a library that does not export `fish_mod_v1`.

#[unsafe(no_mangle)]
pub extern "C" fn not_a_fish_mod() -> u32 {
    7
}
