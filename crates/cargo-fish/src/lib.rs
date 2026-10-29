//! `cargo fish`: create, build, check and test Fish mods.

pub mod build;
pub mod check;
pub mod new;
pub mod testcmd;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

use build::BuildOptions;
use new::Template;

/// Command line of `cargo fish`.
#[derive(Parser, Debug)]
#[command(
    name = "cargo-fish",
    bin_name = "cargo fish",
    version,
    about = "Build, check and test Fish mods"
)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub cmd: Cmd,
}

/// Subcommands.
#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Create a new mod project.
    New {
        /// Crate name (lowercase; `-` becomes `_` in the mod id).
        name: String,
        /// Project template.
        #[arg(long, value_enum, default_value = "basic")]
        template: Template,
        /// Path of a Fish checkout; makes `fish-api` a path dependency.
        #[arg(long)]
        fish_path: Option<PathBuf>,
    },
    /// Build the mod and pack `target/fish/<id>-<version>.fish`.
    Build {
        /// Build with the release profile.
        #[arg(long)]
        release: bool,
        /// Platform key to build for (e.g. `linux-x86_64`).
        #[arg(long)]
        target_platform: Option<String>,
        /// Cargo target directory.
        #[arg(long)]
        target_dir: Option<PathBuf>,
    },
    /// Load a `.fish` file (or mod directory) in a throwaway host and report problems.
    Check {
        /// The mod to check.
        file: PathBuf,
    },
    /// Build, then run `tests/*.toml` scripts.
    Test {
        /// Only run scripts whose file name contains this text.
        filter: Option<String>,
        /// Build with the release profile.
        #[arg(long)]
        release: bool,
        /// Cargo target directory.
        #[arg(long)]
        target_dir: Option<PathBuf>,
    },
    /// Internal: the child process of `check`.
    #[command(name = "__check-child", hide = true)]
    CheckChild {
        /// The mod to load.
        file: PathBuf,
    },
}

/// Drops the `fish` word cargo inserts when running `cargo fish ...`.
pub fn normalize_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut v: Vec<OsString> = args.into_iter().collect();
    if v.get(1).is_some_and(|a| a == "fish") {
        v.remove(1);
    }
    v
}

/// Runs the command line; returns the process exit code.
pub fn run(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    out: &mut dyn std::io::Write,
) -> Result<i32> {
    let exe = std::env::current_exe()?;
    run_with_exe(args, cwd, &exe, out)
}

/// Like [`run`], with the executable used for the `check` child process.
pub fn run_with_exe(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    exe: &Path,
    out: &mut dyn std::io::Write,
) -> Result<i32> {
    let cli = match Cli::try_parse_from(normalize_args(args)) {
        Ok(c) => c,
        Err(e) => {
            write!(out, "{e}")?;
            return Ok(if e.use_stderr() { 2 } else { 0 });
        }
    };
    match cli.cmd {
        Cmd::New {
            name,
            template,
            fish_path,
        } => {
            let root = new::create(cwd, &name, template, fish_path.as_deref())?;
            writeln!(out, "Created {}", root.display())?;
            Ok(0)
        }
        Cmd::Build {
            release,
            target_platform,
            target_dir,
        } => {
            let built = build::build(
                cwd,
                &BuildOptions {
                    release,
                    target_platform,
                    target_dir,
                },
            )?;
            writeln!(
                out,
                "Packed {} {} -> {}",
                built.id,
                built.version,
                built.path.display()
            )?;
            Ok(0)
        }
        Cmd::Check { file } => check::check(exe, &file, out),
        Cmd::Test {
            filter,
            release,
            target_dir,
        } => {
            let built = build::build(
                cwd,
                &BuildOptions {
                    release,
                    target_platform: None,
                    target_dir,
                },
            )?;
            let scripts = testcmd::find_scripts(cwd, filter.as_deref())?;
            if scripts.is_empty() {
                writeln!(
                    out,
                    "no test scripts found in {}",
                    cwd.join("tests").display()
                )?;
                return Ok(0);
            }
            let (_, failed) = testcmd::run_all(&built.path, &scripts, out)?;
            Ok(i32::from(failed > 0))
        }
        Cmd::CheckChild { file } => {
            check::child(&file)?;
            Ok(0)
        }
    }
}
