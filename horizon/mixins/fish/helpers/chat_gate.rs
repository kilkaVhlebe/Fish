fn fish_chat_gate(hub: &Hub, i: usize, message: &str, command: bool) -> std::ops::ControlFlow<bool> {
    let Some(g) = hub.guests.get(i) else {
        return std::ops::ControlFlow::Continue(());
    };
    if crate::fish_lib::allow_chat(&g.name, g.uuid, message, command) {
        std::ops::ControlFlow::Continue(())
    } else {
        std::ops::ControlFlow::Break(false)
    }
}
