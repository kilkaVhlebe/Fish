fn fish_chat_broadcast(hub: &Hub, message: &ChatMessage) {
    let name = hub
        .guests
        .iter()
        .find(|g| g.uuid == message.sender)
        .map_or("", |g| g.name.as_str());
    crate::fish_lib::chat_sent(name, message.sender, &message.content);
}
