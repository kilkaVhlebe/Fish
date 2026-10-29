//! What the edition's bridge calls at process start.

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::dispatch::{LoaderDispatch, install_global};
use crate::{BootConfig, Loader, Side, boot, selftest};

/// Decides side and game directory from the command line, as Horizon's own `main` does.
///
/// Returns `None` for `--help`/`-h` (before `--dedicated`): nothing may start then.
pub fn side_and_home(
    args: &[String],
    game_home: impl FnOnce() -> PathBuf,
) -> Option<(Side, PathBuf)> {
    let mut game_args = args.iter().take_while(|a| *a != "--dedicated");
    if game_args.any(|a| a == "--help" || a == "-h") {
        return None;
    }
    if args.iter().any(|a| a == "--dedicated") {
        Some((Side::Server, PathBuf::from(".")))
    } else {
        Some((Side::Client, game_home()))
    }
}

static BOOTED: OnceLock<Option<&'static Loader>> = OnceLock::new();

/// Boots Fish for the edition, once per process: environment configuration, global install,
/// optional self test.
///
/// Called by the bridge at the start of the game's `run`, once Horizon's logging is up. `args`
/// are the process arguments without the program name. With `FISH_LOADER_SELFTEST` set the self
/// test runs and the process exits before the game starts.
pub fn boot_for_edition(
    args: &[String],
    game_home: impl FnOnce() -> PathBuf,
) -> Option<&'static Loader> {
    *BOOTED.get_or_init(|| {
        let (side, home) = side_and_home(args, game_home)?;
        let loader = match boot(BootConfig::from_env(side, home)) {
            Ok(l) => l,
            Err(e) => {
                log::error!("Fish could not start: {e}");
                return None;
            }
        };
        let loader = install_global(loader);
        if let Some(path) = std::env::var_os("FISH_LOADER_SELFTEST") {
            let dispatch = LoaderDispatch::new(loader.clone());
            let code = selftest::run_and_write(std::path::Path::new(&path), loader, &dispatch);
            std::process::exit(code);
        }
        Some(loader)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn side_and_home_follow_horizons_main() {
        let home = || PathBuf::from("/home/game");
        assert_eq!(
            side_and_home(&s(&[]), home),
            Some((Side::Client, PathBuf::from("/home/game")))
        );
        assert_eq!(
            side_and_home(&s(&["--name", "Dev"]), home).unwrap().0,
            Side::Client
        );
        assert_eq!(
            side_and_home(&s(&["--dedicated", "--nogui"]), home),
            Some((Side::Server, PathBuf::from(".")))
        );
        // --help before --dedicated stops everything; after it, it belongs to the server
        assert_eq!(side_and_home(&s(&["--help"]), home), None);
        assert_eq!(side_and_home(&s(&["-h", "--dedicated"]), home), None);
        assert_eq!(
            side_and_home(&s(&["--dedicated", "--help"]), home)
                .unwrap()
                .0,
            Side::Server
        );
    }

    #[test]
    fn help_boots_nothing_and_is_remembered() {
        // Runs in this test binary only (the OnceLock is process-wide): --help must not boot.
        assert!(boot_for_edition(&s(&["--help"]), || PathBuf::from("/unused")).is_none());
        assert!(
            boot_for_edition(&s(&[]), || PathBuf::from("/unused")).is_none(),
            "the first answer sticks"
        );
    }
}
