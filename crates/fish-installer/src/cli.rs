//! The command line front end.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::error::{Error, Result};
use crate::flow::{self, Installer, Kind, LaunchBinary, Options, Reporter, StderrReporter};
use crate::launcher::{self, LauncherCli, Probes, ProcessLauncherCli};
use crate::source::{self, HttpFetcher};

/// `fish-installer` command line.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "fish-installer",
    version,
    about = "Install Fish next to the Horizon launcher"
)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
    /// Path of horizon-launcher (default: searched for).
    #[arg(long, global = true)]
    pub launcher: Option<PathBuf>,
    /// Name of the instance Fish runs in.
    #[arg(long, global = true, default_value = "Fish")]
    pub instance: String,
    /// Install from this edition zip instead of the release index.
    #[arg(long, global = true)]
    pub edition: Option<PathBuf>,
    /// Make Play start the Fish instance.
    #[arg(long, global = true)]
    pub select: bool,
    /// Replace the wrapper of an instance that already has another one.
    #[arg(long, global = true)]
    pub force: bool,
    /// Accept an unsigned edition index (development only; also FISH_LOADER_ALLOW_UNSIGNED=1).
    #[arg(long, global = true)]
    pub allow_unsigned: bool,
    /// Do not ask questions.
    #[arg(long, short, global = true)]
    pub yes: bool,
}

/// Subcommands.
#[derive(Subcommand, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Install the newest edition and set up the instance.
    Install,
    /// Reinstall the recorded edition and everything around it.
    Repair,
    /// Install the newest edition and re-pin the instance.
    Update,
    /// Clear the wrapper and remove Fish's editions and binaries (the instance and worlds stay).
    Uninstall,
    /// Show what is installed.
    Status,
}

fn ask(prompt: &str, input: &mut dyn BufRead, out: &mut dyn Write) -> Result<String> {
    write!(out, "{prompt}").map_err(|e| Error::io("writing the prompt", e))?;
    out.flush().ok();
    let mut line = String::new();
    input
        .read_line(&mut line)
        .map_err(|e| Error::io("reading the answer", e))?;
    Ok(line.trim().to_string())
}

/// Finds the launcher, asking for a path if nothing works and questions are allowed.
pub fn find_launcher(
    cli: &Cli,
    probes: &Probes,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<PathBuf> {
    let works = |p: &Path| launcher::is_working_launcher(p);
    match launcher::discover(cli.launcher.as_deref(), probes, &works) {
        Ok(p) => Ok(p),
        Err(e) if cli.yes => Err(e),
        Err(e) => {
            writeln!(out, "{e}").ok();
            let answer = ask("Path to horizon-launcher: ", input, out)?;
            let p = PathBuf::from(answer.trim_matches('"'));
            if p.is_file() && works(&p) {
                Ok(p)
            } else {
                Err(e)
            }
        }
    }
}

/// The `fish-launch` binary to install: bundled, or the file next to this executable.
pub fn launch_binary() -> Result<LaunchBinary> {
    #[cfg(feature = "embed-launch")]
    {
        return Ok(LaunchBinary::Bytes(
            include_bytes!(env!("FISH_LAUNCH_BIN")).to_vec(),
        ));
    }
    #[cfg(not(feature = "embed-launch"))]
    {
        let me = std::env::current_exe().map_err(|e| Error::io("locating the installer", e))?;
        Ok(LaunchBinary::File(
            me.with_file_name(flow::launch_exe_name()),
        ))
    }
}

/// Everything `run` takes from the outside world, so tests can supply their own.
pub struct Ctx<'a> {
    /// Fish home.
    pub home: PathBuf,
    /// `FISH_LOADER_RELEASES_URL`.
    pub releases_url: Option<String>,
    /// `FISH_LOADER_ALLOW_UNSIGNED=1`.
    pub allow_unsigned_env: bool,
    /// The `fish-launch` to install.
    pub launch: LaunchBinary,
    /// The installer executable to copy into `bin/`.
    pub self_exe: Option<PathBuf>,
    /// Network access.
    pub fetcher: &'a dyn source::Fetcher,
    /// Release public key (empty: none).
    pub key: &'a str,
    /// `owner/repo` of the default release source.
    pub github: &'a str,
}

impl Ctx<'static> {
    /// The real process environment.
    pub fn from_process() -> Result<Ctx<'static>> {
        Ok(Ctx {
            home: fish_launch::fish_home(),
            releases_url: std::env::var("FISH_LOADER_RELEASES_URL").ok(),
            allow_unsigned_env: std::env::var("FISH_LOADER_ALLOW_UNSIGNED").is_ok_and(|v| v == "1"),
            launch: launch_binary()?,
            self_exe: std::env::current_exe().ok(),
            fetcher: &HttpFetcher,
            key: source::RELEASE_KEY,
            github: source::RELEASE_GITHUB,
        })
    }
}

/// Runs the command line; returns the exit code.
pub fn run(
    cli: &Cli,
    probes: &Probes,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    reporter: &dyn Reporter,
    ctx: Ctx<'_>,
) -> Result<i32> {
    let home = ctx.home.clone();
    if cli.command == Command::Status {
        let exe = find_launcher(
            &Cli {
                yes: true,
                ..cli.clone()
            },
            probes,
            input,
            out,
        )
        .ok();
        let cli_proc = exe.as_ref().map(ProcessLauncherCli::new);
        let s = flow::status(&home, cli_proc.as_ref().map(|c| c as &dyn LauncherCli));
        write!(out, "{s}").map_err(|e| Error::io("writing the status", e))?;
        return Ok(i32::from(s.state.is_none()));
    }
    let exe = find_launcher(cli, probes, input, out)?;
    let launcher = ProcessLauncherCli::new(&exe);
    let options = Options {
        home: home.clone(),
        instance: cli.instance.clone(),
        select: cli.select,
        force: cli.force,
        allow_unsigned: cli.allow_unsigned || ctx.allow_unsigned_env,
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        edition_zip: cli.edition.clone(),
        edition_id: None,
        releases_url: ctx.releases_url.clone(),
        launcher_exe: exe.display().to_string(),
    };
    let installer = Installer {
        launcher: &launcher,
        fetcher: ctx.fetcher,
        reporter,
        options,
        launch: ctx.launch,
        self_exe: ctx.self_exe,
        key: ctx.key,
        github: ctx.github,
    };
    match cli.command {
        Command::Install => installer.run(Kind::Install).map(|_| 0),
        Command::Repair => installer.run(Kind::Repair).map(|_| 0),
        Command::Update => installer.run(Kind::Update).map(|_| 0),
        Command::Uninstall => {
            if !cli.yes {
                let answer = ask(
                    &format!(
                        "Remove Fish's editions and binaries from {}? [y/N] ",
                        home.display()
                    ),
                    input,
                    out,
                )?;
                if !answer.eq_ignore_ascii_case("y") && !answer.eq_ignore_ascii_case("yes") {
                    writeln!(out, "Nothing was changed.").ok();
                    return Ok(1);
                }
            }
            installer.uninstall().map(|()| 0)
        }
        Command::Status => unreachable!("handled above"),
    }
}

/// Entry point of the binary.
pub fn main_cli(args: Vec<std::ffi::OsString>) -> i32 {
    #[cfg(feature = "gui")]
    if args.len() <= 1 {
        return crate::gui::run_window();
    }
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() { 2 } else { 0 };
        }
    };
    let ctx = match Ctx::from_process() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let stdin = std::io::stdin();
    match run(
        &cli,
        &Probes::here(),
        &mut stdin.lock(),
        &mut std::io::stdout(),
        &StderrReporter,
        ctx,
    ) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_parse() {
        let c = Cli::try_parse_from([
            "fish-installer",
            "install",
            "--instance",
            "Mine",
            "--select",
            "--force",
            "--allow-unsigned",
            "-y",
            "--edition",
            "e.zip",
            "--launcher",
            "l",
        ])
        .unwrap();
        assert_eq!(c.command, Command::Install);
        assert_eq!(
            (
                c.instance.as_str(),
                c.select,
                c.force,
                c.allow_unsigned,
                c.yes
            ),
            ("Mine", true, true, true, true)
        );
        assert_eq!(c.edition.as_deref(), Some(Path::new("e.zip")));
        let d = Cli::try_parse_from(["fish-installer", "status"]).unwrap();
        assert_eq!(d.instance, "Fish");
        for sub in ["repair", "update", "uninstall"] {
            assert!(Cli::try_parse_from(["fish-installer", sub]).is_ok());
        }
        assert!(Cli::try_parse_from(["fish-installer", "bogus"]).is_err());
    }

    #[test]
    fn the_prompt_is_used_only_when_questions_are_allowed() {
        let nothing = Probes {
            env: None,
            path: vec![],
            running: vec![],
            default_location: None,
        };
        let cli = Cli::try_parse_from(["fish-installer", "install", "--yes"]).unwrap();
        let mut out = Vec::new();
        assert!(find_launcher(&cli, &nothing, &mut "/x\n".as_bytes(), &mut out).is_err());
        let ask_cli = Cli { yes: false, ..cli };
        let mut out = Vec::new();
        let err = find_launcher(
            &ask_cli,
            &nothing,
            &mut "/does/not/exist\n".as_bytes(),
            &mut out,
        )
        .unwrap_err();
        assert!(String::from_utf8_lossy(&out).contains("Path to horizon-launcher:"));
        assert!(err.to_string().contains("no working horizon-launcher"));
    }
}
