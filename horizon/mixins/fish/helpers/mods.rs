fn fish_mods(mods: &mut Vec<NativeMod>) {
    crate::game::fish_bridge::api::list_mods(mods);
}
