fn main() {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read the current directory: {e}");
            std::process::exit(1);
        }
    };
    let code = match cargo_fish::run(std::env::args_os(), &cwd, &mut std::io::stdout()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(code);
}
