fn fish_block_place(game: &mut Game<'_>, pos: [i32; 3], state: BlockStateId, stack: &ItemStack) {
    crate::game::fish_bridge::blocks::placed(game, pos, state.0, &stack.item);
}
