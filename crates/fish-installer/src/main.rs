fn main() {
    std::process::exit(fish_installer::cli::main_cli(std::env::args_os().collect()));
}
