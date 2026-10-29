fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let code = fish_launch::main_with(&args, &fish_launch::fish_home());
    std::process::exit(code);
}
