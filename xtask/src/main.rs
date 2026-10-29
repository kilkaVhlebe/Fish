//! Repository automation for Fish.

mod config;
mod docs;
mod lcov;
mod roadmap;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "Fish repository automation")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// fmt check + clippy + tests + docs-check, failing fast.
    Ci,
    /// Verify docs parity (en/ru files, SUMMARY links, roadmap, TODO ban).
    DocsCheck,
    /// Run cargo-llvm-cov and enforce per-crate gates from fish.toml.
    Coverage,
    /// Verify fish.toml against the workspace and fish-abi.
    CheckConfig,
    /// Tick roadmap items (e.g. `P0.1 P0.2`) in both languages.
    Tick { ids: Vec<String> },
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has a parent")
        .to_path_buf()
}

fn run(root: &Path, program: &str, args: &[&str]) -> Result<()> {
    eprintln!("$ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status()
        .with_context(|| format!("spawning {program}"))?;
    if !status.success() {
        bail!("`{program} {}` failed with {status}", args.join(" "));
    }
    Ok(())
}

fn report(what: &str, errors: Vec<String>) -> Result<()> {
    if errors.is_empty() {
        eprintln!("{what}: ok");
        return Ok(());
    }
    for e in &errors {
        eprintln!("error: {e}");
    }
    bail!("{what} failed with {} error(s)", errors.len())
}

fn docs_check(root: &Path) -> Result<()> {
    report("docs-check", docs::check(root)?)
}

fn check_config(root: &Path) -> Result<()> {
    report("check-config", config::check(root)?)
}

fn coverage(root: &Path) -> Result<()> {
    let cfg = config::parse(&std::fs::read_to_string(root.join("fish.toml"))?)?;
    run(
        root,
        "cargo",
        &[
            "llvm-cov",
            "--workspace",
            "--exclude",
            "xtask",
            "--lcov",
            "--output-path",
            "target/lcov.info",
        ],
    )?;
    let lcov = std::fs::read_to_string(root.join("target/lcov.info"))?;
    let data = lcov::aggregate(&lcov, &cfg.coverage_exclude);
    for (name, l) in &data {
        eprintln!("{name}: {:.1}% ({}/{})", l.percent(), l.hit, l.found);
    }
    let missing_ok = |name: &str| !root.join("crates").join(name).is_dir();
    report("coverage", lcov::check(&data, &cfg.coverage, &missing_ok))
}

fn ci(root: &Path) -> Result<()> {
    run(root, "cargo", &["fmt", "--all", "--check"])?;
    run(
        root,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(root, "cargo", &["test", "--workspace", "--locked"])?;
    check_config(root)?;
    docs_check(root)
}

fn tick(root: &Path, ids: &[String]) -> Result<()> {
    for lang in ["en", "ru"] {
        let path = root.join(format!("docs/{lang}/src/roadmap.md"));
        let mut text = std::fs::read_to_string(&path)?;
        for id in ids {
            text = roadmap::tick(&text, id)
                .with_context(|| format!("{id} is not an unchecked item in docs/{lang} roadmap"))?;
        }
        std::fs::write(&path, text)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let root = root();
    match Cli::parse().cmd {
        Cmd::Ci => ci(&root),
        Cmd::DocsCheck => docs_check(&root),
        Cmd::Coverage => coverage(&root),
        Cmd::CheckConfig => check_config(&root),
        Cmd::Tick { ids } => tick(&root, &ids),
    }
}
