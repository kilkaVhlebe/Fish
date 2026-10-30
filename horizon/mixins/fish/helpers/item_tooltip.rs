fn fish_item_tooltip(
    stack: &ItemStack,
    lines: Vec<(horizon_client::text::Text, u32)>,
) -> Vec<(horizon_client::text::Text, u32)> {
    crate::game::fish_bridge::items::tooltip(stack, lines)
}
