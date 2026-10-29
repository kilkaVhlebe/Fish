fn fish_boot() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    fish_hooks::install_net(&crate::fish_lib::NET);
    fish_loader::edition::boot_for_edition(&args, crate::logging::game_home);
    crate::fish_lib::refresh_crash_details();
}
