fn fish_pick_item(stack: &ItemStack) -> std::ops::ControlFlow<bool> {
    if crate::fish_lib::pick_item(&stack.item) {
        std::ops::ControlFlow::Continue(())
    } else {
        std::ops::ControlFlow::Break(false)
    }
}
