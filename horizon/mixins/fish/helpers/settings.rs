fn fish_settings(mod_id: &str, buttons: &mut Vec<String>) {
    crate::game::fish_bridge::api::settings_buttons(mod_id, buttons);
}
