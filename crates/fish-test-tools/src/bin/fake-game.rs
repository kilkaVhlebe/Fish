//! A stand-in for `horizon-client`: records how it was started, then exits or waits.
//!
//! * `FAKE_GAME_RECORD=<file>`: write `{args, cwd, fish_home, pid}` as JSON there.
//! * `FAKE_GAME_EXIT=<n>`: exit with code `n` (default 0).
//! * `FAKE_GAME_SLEEP_SECS=<n>`: sleep first (to be killed).

fn main() {
    if let Some(path) = std::env::var_os("FAKE_GAME_RECORD") {
        let record = serde_json::json!({
            "args": std::env::args().skip(1).collect::<Vec<_>>(),
            "cwd": std::env::current_dir().ok().map(|d| d.display().to_string()),
            "fish_home": std::env::var("FISH_LOADER_HOME").ok(),
            "pid": std::process::id(),
        });
        let _ = std::fs::write(path, record.to_string());
    }
    if let Some(secs) = std::env::var("FAKE_GAME_SLEEP_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
    let code = std::env::var("FAKE_GAME_EXIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
