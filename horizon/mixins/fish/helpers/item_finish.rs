fn fish_item_finish(game: &mut Game<'_>, stack: &ItemStack) {
    crate::game::fish_bridge::items::finished(game, stack);
}
