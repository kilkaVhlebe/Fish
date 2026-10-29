fn fish_boot() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    fish_loader::edition::boot_for_edition(&args, crate::logging::game_home);
}
