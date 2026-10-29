//! Repository automation for Fish.

mod config;
mod docs;
mod edition;
mod horizon;
mod lcov;
mod pin;
mod roadmap;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    /// Fetch Horizon's source.
    #[command(subcommand)]
    Horizon(HorizonCmd),
    /// Manage horizon-mixins.
    #[command(subcommand)]
    Mixins(MixinsCmd),
    /// Distribution artifacts.
    #[command(subcommand)]
    Dist(DistCmd),
    /// Build the Fish Edition of Horizon.
    #[command(subcommand)]
    Edition(EditionCmd),
    /// Local in-game end to end test of the built edition (needs prepared game resources).
    E2e {
        /// Prepared resources folder (`.local/horizon-client/26.3` of a launcher version).
        #[arg(long)]
        resources: PathBuf,
        /// Path of horizon-ctl.
        #[arg(long)]
        horizon_ctl: PathBuf,
        /// Profile that was built.
        #[arg(long, value_enum, default_value = "release")]
        profile: edition::Profile,
    },
}

#[derive(Subcommand)]
enum HorizonCmd {
    /// Download the pinned commit into .horizon/source/<commit>/.
    Fetch,
    /// Pin fish.toml to the newest dated Nightly and rebuild the edition on it.
    Pin {
        /// Resolve the newest `nightly-YYYY-MM-DD` release (the only mode for now).
        #[arg(long)]
        latest_dated: bool,
        /// Only report whether fish.toml is behind; change nothing.
        #[arg(long)]
        check: bool,
        /// Update fish.toml but skip the rebuild.
        #[arg(long)]
        no_build: bool,
    },
}

#[derive(Subcommand)]
enum MixinsCmd {
    /// Install horizon-mixins at the pinned revision.
    Install,
}

#[derive(Subcommand)]
enum DistCmd {
    /// Build the single-file installer with `fish-launch` embedded.
    Installer,
}

#[derive(Subcommand)]
enum EditionCmd {
    /// Copy the source and the edition crates into .horizon/work/<commit>/src.
    Stage,
    /// Assemble the bundle and apply it with horizon-mixins.
    Mixins,
    /// Compile the edition binary.
    Build {
        /// Cargo profile.
        #[arg(long, value_enum, default_value = "release")]
        profile: edition::Profile,
    },
    /// Zip the binary into dist/.
    Package {
        /// Profile that was built.
        #[arg(long, value_enum, default_value = "release")]
        profile: edition::Profile,
    },
    /// Run the built edition's self test with the hello mod.
    Selftest {
        /// Profile that was built.
        #[arg(long, value_enum, default_value = "release")]
        profile: edition::Profile,
    },
    /// Run Horizon's own tests on pristine and staged source; fail only on regressions.
    Regressions,
    /// fetch, install mixins, stage, apply, build, package, self test.
    All {
        /// Cargo profile.
        #[arg(long, value_enum, default_value = "release")]
        profile: edition::Profile,
    },
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

/// Like [`run`], but the child's stdout is discarded.
fn run_silent(root: &Path, program: &str, args: &[&str]) -> Result<()> {
    eprintln!("$ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .stdout(Stdio::null())
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

fn load_config(root: &Path) -> Result<config::FishConfig> {
    config::parse(&std::fs::read_to_string(root.join("fish.toml")).context("reading fish.toml")?)
}

fn horizon_pin(root: &Path, latest_dated: bool, check: bool, no_build: bool) -> Result<()> {
    if !latest_dated {
        bail!("pass --latest-dated (pinning an explicit commit is done by editing fish.toml)");
    }
    let cfg = load_config(root)?;
    let pin = pin::resolve_latest_dated()?;
    if pin.commit == cfg.horizon_commit && pin.release_tag == cfg.horizon_release_tag {
        eprintln!(
            "fish.toml already pins {} ({})",
            pin.release_tag, pin.commit
        );
        return Ok(());
    }
    eprintln!(
        "newest dated Nightly: {} -> {} ({}); fish.toml pins {}",
        pin.release_tag, pin.commit, pin.source_tag, cfg.horizon_commit
    );
    if check {
        // Behind: exit code 1 so a workflow can open a pull request.
        bail!("fish.toml is behind the newest dated Nightly");
    }
    let path = root.join("fish.toml");
    let updated = pin::update_fish_toml(
        &std::fs::read_to_string(&path)?,
        &pin.commit,
        &pin.source_tag,
        &pin.release_tag,
    )?;
    std::fs::write(&path, updated)?;
    if no_build {
        return Ok(());
    }
    edition_cmd(
        root,
        EditionCmd::All {
            profile: edition::Profile::Fast,
        },
    )
}

fn edition_cmd(root: &Path, cmd: EditionCmd) -> Result<()> {
    let cfg = load_config(root)?;
    match cmd {
        EditionCmd::Stage => horizon::stage(root, &cfg).map(|_| ()),
        EditionCmd::Mixins => edition::apply_mixins(root, &cfg).map(|_| ()),
        EditionCmd::Regressions => edition::run_regressions(root, &cfg),
        EditionCmd::Build { profile } => edition::build(root, &cfg, profile).map(|_| ()),
        EditionCmd::Package { profile } => edition::package(root, &cfg, profile).map(|_| ()),
        EditionCmd::Selftest { profile } => {
            let report = edition::selftest(root, &cfg, profile)?;
            eprintln!("self test passed: {}", report["checks"]);
            Ok(())
        }
        EditionCmd::All { profile } => {
            horizon::fetch(root, &cfg)?;
            horizon::mixins_install(root, &cfg)?;
            horizon::stage(root, &cfg)?;
            edition::apply_mixins(root, &cfg)?;
            edition::build(root, &cfg, profile)?;
            edition::package(root, &cfg, profile)?;
            let report = edition::selftest(root, &cfg, profile)?;
            eprintln!("self test passed: {}", report["checks"]);
            Ok(())
        }
    }
}

/// Builds `fish-launch`, then an installer that carries it, and copies the installer to dist/.
fn dist_installer(root: &Path) -> Result<()> {
    run(
        root,
        "cargo",
        &["build", "--release", "--locked", "-p", "fish-launch"],
    )?;
    let exe = if cfg!(windows) { ".exe" } else { "" };
    let launch = root
        .join("target/release")
        .join(format!("fish-launch{exe}"));
    if !launch.is_file() {
        bail!("{} was not built", launch.display());
    }
    eprintln!("$ cargo build --release -p fish-installer --features embed-launch");
    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "fish-installer",
            "--features",
            "embed-launch",
        ])
        .env("FISH_LAUNCH_BIN", &launch)
        .current_dir(root)
        .status()
        .context("running cargo")?;
    if !status.success() {
        bail!("building the installer failed");
    }
    let built = root
        .join("target/release")
        .join(format!("fish-installer{exe}"));
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist)?;
    let out = dist.join(format!(
        "fish-installer-{}-{}{exe}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    std::fs::copy(&built, &out).with_context(|| format!("copying {}", built.display()))?;
    eprintln!("wrote {}", out.display());
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
        Cmd::Horizon(HorizonCmd::Pin {
            latest_dated,
            check,
            no_build,
        }) => horizon_pin(&root, latest_dated, check, no_build),
        Cmd::Horizon(HorizonCmd::Fetch) => horizon::fetch(&root, &load_config(&root)?).map(|_| ()),
        Cmd::Mixins(MixinsCmd::Install) => {
            horizon::mixins_install(&root, &load_config(&root)?).map(|_| ())
        }
        Cmd::Edition(cmd) => edition_cmd(&root, cmd),
        Cmd::Dist(DistCmd::Installer) => dist_installer(&root),
        Cmd::E2e {
            resources,
            horizon_ctl,
            profile,
        } => edition::e2e(
            &root,
            &load_config(&root)?,
            profile,
            &resources,
            &horizon_ctl,
        ),
    }
}
