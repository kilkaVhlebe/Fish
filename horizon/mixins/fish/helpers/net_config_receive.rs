fn fish_net_config_receive(payload: &cfg::CustomPayload) -> std::ops::ControlFlow<()> {
    if payload.channel == "minecraft:brand" || payload.channel == horizon_protocol::quiet::CHANNEL {
        return std::ops::ControlFlow::Continue(());
    }
    if crate::fish_lib::client_config_payload(&payload.channel, &payload.data) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
