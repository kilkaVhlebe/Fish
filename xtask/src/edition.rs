//! The Fish Edition pipeline: bundle, apply mixins, build, package, self test.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::config::FishConfig;
use crate::horizon::{self, copy_tree, source_dir, work_dir};

/// Build profile of the edition binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Profile {
    /// Horizon's release profile (thin LTO).
    Release,
    /// Horizon's `fast` profile: optimised, quick to link.
    Fast,
}

impl Profile {
    /// The cargo profile name.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Release => "release",
            Profile::Fast => "fast",
        }
    }
}

/// The Horizon version baked into the loader: `<line>.0-nightly.<YYYYMMDD>+<short commit>`.
pub fn horizon_version(cfg: &FishConfig) -> String {
    let short = &cfg.horizon_commit[..cfg.horizon_commit.len().min(7)];
    let date = Regex::new(r"(\d{4})-(\d{2})-(\d{2})")
        .expect("static regex")
        .captures(&cfg.horizon_source_tag)
        .map(|c| format!("{}{}{}", &c[1], &c[2], &c[3]));
    match date {
        Some(d) => format!("{}.0-nightly.{d}+{short}", cfg.horizon_release_line),
        None => format!("{}.0-dev+{short}", cfg.horizon_release_line),
    }
}

/// Edition id: `<fish version>+horizon.<short commit>`.
pub fn edition_id(cfg: &FishConfig) -> String {
    format!("{}+horizon.{}", cfg.version, &cfg.horizon_commit[..7])
}

/// File name of the edition archive for `platform`.
pub fn archive_name(cfg: &FishConfig, platform: &str) -> String {
    format!(
        "fish-edition-{}-horizon-{}-{platform}.zip",
        cfg.version,
        &cfg.horizon_commit[..7]
    )
}

/// Concatenates `modules/bridge/*.rs` into one payload of inline modules.
///
/// horizon-mixins refuses external `mod` declarations and `include!`, so a multi-file module is
/// assembled into a single source before planning.
pub fn assemble_bridge(dir: &Path) -> Result<String> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();
    if files.is_empty() {
        bail!("{} contains no .rs files", dir.display());
    }
    let mut out = String::from(
        "// Fish's bridge between Horizon's hook points and the Fish loader.\n// (Plain comments only: horizon-mixins refuses inner doc attributes in module payloads.)\n\n",
    );
    for file in files {
        let name = file
            .file_stem()
            .and_then(|s| s.to_str())
            .context("bridge file name")?;
        let body = fs::read_to_string(&file)?;
        out.push_str(&format!(
            "pub(crate) mod {name} {{\n{}\n}}\n\n",
            body.trim_end()
        ));
    }
    Ok(out)
}

/// Fills the `@placeholders@` of the bundle manifest.
pub fn fill_manifest(template: &str, cfg: &FishConfig) -> String {
    template
        .replace("@FISH_VERSION@", &cfg.version)
        .replace("@HORIZON_COMMIT@", &cfg.horizon_commit)
}

/// SHA-256 of every file below `dir` (names and contents, sorted).
pub fn hash_tree(dir: &Path) -> Result<String> {
    let mut files: Vec<PathBuf> = WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    files.sort();
    let mut h = Sha256::new();
    for f in files {
        let rel = f.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(fs::read(&f)?);
    }
    Ok(hex(&h.finalize()))
}

/// SHA-256 of a file, hex.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut h = Sha256::new();
    h.update(fs::read(path).with_context(|| format!("reading {}", path.display()))?);
    Ok(hex(&h.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Copies `horizon/mixins/fish` to the work directory with the placeholders filled and the
/// bridge assembled; returns the bundle's parent (what `--mods` takes).
pub fn assemble_bundle(root: &Path, cfg: &FishConfig) -> Result<PathBuf> {
    let parent = work_dir(root, &cfg.horizon_commit).join("bundle");
    let bundle = parent.join("fish");
    if parent.exists() {
        fs::remove_dir_all(&parent)?;
    }
    let from = root.join("horizon/mixins/fish");
    copy_tree(&from.join("helpers"), &bundle.join("helpers"))?;
    let template = fs::read_to_string(from.join("horizon-mixin.toml"))?;
    fs::create_dir_all(bundle.join("modules"))?;
    fs::write(
        bundle.join("horizon-mixin.toml"),
        fill_manifest(&template, cfg),
    )?;
    fs::write(
        bundle.join("modules/bridge.rs"),
        assemble_bridge(&from.join("modules/bridge"))?,
    )?;
    // Single-file support modules (`modules/<name>.rs`) go through unchanged.
    for entry in fs::read_dir(from.join("modules"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "rs") {
            fs::copy(
                &path,
                bundle
                    .join("modules")
                    .join(path.file_name().context("module name")?),
            )?;
        }
    }
    Ok(parent)
}

/// Makes `to` hold exactly the files of `from`, touching only files whose content differs, so
/// Cargo (which goes by file times) recompiles only what the bundle changed. Returns
/// `(copied, removed)`.
pub fn sync_tree(from: &Path, to: &Path) -> Result<(usize, usize)> {
    let mut copied = 0;
    let mut wanted = std::collections::BTreeSet::new();
    for entry in WalkDir::new(from) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(from)?.to_path_buf();
        let target = to.join(&rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            wanted.insert(rel);
            let new = fs::read(entry.path())?;
            if fs::read(&target).ok().as_deref() != Some(new.as_slice()) {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&target, new)?;
                copied += 1;
            }
        }
    }
    let mut removed = 0;
    if to.is_dir() {
        for entry in WalkDir::new(to).contents_first(true) {
            let entry = entry?;
            let rel = entry.path().strip_prefix(to)?;
            if entry.file_type().is_file() && !wanted.contains(rel) {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
    }
    Ok((copied, removed))
}

/// Applies the bundle to the staged source with horizon-mixins; returns the staged workspace.
pub fn apply_mixins(root: &Path, cfg: &FishConfig) -> Result<PathBuf> {
    let work = work_dir(root, &cfg.horizon_commit);
    let src = work.join("src");
    if !src.join("Cargo.toml").is_file() {
        bail!("nothing staged: run `cargo xtask edition stage` first");
    }
    let bundle = assemble_bundle(root, cfg)?;
    let staged = work.join("staged");
    // horizon-mixins cannot replace a stage on Windows and wants a fresh directory; the result is
    // then synced into `staged` so unchanged files keep their times (and their compiled crates).
    let fresh = work.join("staged-new");
    if fresh.exists() {
        fs::remove_dir_all(&fresh)?;
    }
    let bin = horizon::mixins_install(root, cfg)?;
    let status = Command::new(&bin)
        .arg("apply")
        .arg("--workspace")
        .arg(&src)
        .arg("--mods")
        .arg(&bundle)
        .arg("--out")
        .arg(&fresh)
        .status()
        .context("running horizon-mixins apply")?;
    if !status.success() {
        bail!("horizon-mixins apply failed ({status}); its diagnostic is above");
    }
    let (copied, removed) = sync_tree(&fresh, &staged)?;
    fs::remove_dir_all(&fresh)?;
    eprintln!("staged workspace updated: {copied} file(s) changed, {removed} removed");
    Ok(staged)
}

/// Builds the edition binary; returns its path.
pub fn build(root: &Path, cfg: &FishConfig, profile: Profile) -> Result<PathBuf> {
    let staged = work_dir(root, &cfg.horizon_commit).join("staged");
    if !staged.join("Cargo.toml").is_file() {
        bail!("nothing applied: run `cargo xtask edition mixins` first");
    }
    let target = horizon::horizon_dir(root).join("target");
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&staged)
        .args([
            "build",
            "--locked",
            "--profile",
            profile.name(),
            "--features",
            "horizon-client/mods",
            "-p",
            "horizon-client",
            "--bin",
            "horizon-client",
        ])
        .env("CARGO_TARGET_DIR", &target)
        .env("FISH_HORIZON_VERSION", horizon_version(cfg));
    if cfg!(windows) {
        cmd.env("RUSTFLAGS", "-C target-feature=+crt-static");
    }
    eprintln!(
        "building the edition ({}), this takes a while",
        profile.name()
    );
    let status = cmd.status().context("running cargo build")?;
    if !status.success() {
        bail!("building the edition failed ({status})");
    }
    let exe = if cfg!(windows) {
        "horizon-client.exe"
    } else {
        "horizon-client"
    };
    let bin = target.join(profile.name()).join(exe);
    if !bin.is_file() {
        bail!("{} was not produced", bin.display());
    }
    Ok(bin)
}

#[derive(Serialize)]
struct Abi {
    major: u16,
    minor: u16,
}

#[derive(Serialize)]
struct HorizonInfo<'a> {
    commit: &'a str,
    source_tag: &'a str,
    release_tag: &'a str,
    version: String,
}

#[derive(Serialize)]
struct BundleInfo {
    id: &'static str,
    sha256: String,
}

#[derive(Serialize)]
struct BinaryInfo {
    name: String,
    sha256: String,
}

#[derive(Serialize)]
struct EditionManifest<'a> {
    schema: u32,
    id: String,
    fish_version: &'a str,
    abi: Abi,
    horizon: HorizonInfo<'a>,
    platform: String,
    server: bool,
    rustc: String,
    mixins_rev: &'a str,
    profile: &'static str,
    mixin_bundles: Vec<BundleInfo>,
    binary: BinaryInfo,
}

/// `fish-edition.json` for a built binary.
pub fn edition_manifest(
    cfg: &FishConfig,
    binary: &Path,
    bundle_sha: String,
    profile: Profile,
) -> Result<String> {
    let rustc = Command::new("rustc")
        .arg("--version")
        .output()
        .context("rustc --version")?;
    let rustc = String::from_utf8_lossy(&rustc.stdout)
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .to_string();
    let manifest = EditionManifest {
        schema: 1,
        id: edition_id(cfg),
        fish_version: &cfg.version,
        abi: Abi {
            major: cfg.abi.0,
            minor: cfg.abi.1,
        },
        horizon: HorizonInfo {
            commit: &cfg.horizon_commit,
            source_tag: &cfg.horizon_source_tag,
            release_tag: &cfg.horizon_release_tag,
            version: horizon_version(cfg),
        },
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        server: false,
        rustc,
        mixins_rev: &cfg.mixins_rev,
        profile: profile.name(),
        mixin_bundles: vec![BundleInfo {
            id: "fish",
            sha256: bundle_sha,
        }],
        binary: BinaryInfo {
            name: binary
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            sha256: hash_file(binary)?,
        },
    };
    Ok(serde_json::to_string_pretty(&manifest)?)
}

/// Zips the binary, `fish-edition.json` and the license files into `dist/`.
pub fn package(root: &Path, cfg: &FishConfig, profile: Profile) -> Result<PathBuf> {
    let exe = if cfg!(windows) {
        "horizon-client.exe"
    } else {
        "horizon-client"
    };
    let binary = horizon::horizon_dir(root)
        .join("target")
        .join(profile.name())
        .join(exe);
    if !binary.is_file() {
        bail!(
            "{} is missing: run `cargo xtask edition build` first",
            binary.display()
        );
    }
    let bundle_sha = hash_tree(&work_dir(root, &cfg.horizon_commit).join("bundle/fish"))?;
    let manifest = edition_manifest(cfg, &binary, bundle_sha, profile)?;
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let out = root.join("dist").join(archive_name(cfg, &platform));
    fs::create_dir_all(out.parent().expect("dist has a parent"))?;
    let pristine = source_dir(root, &cfg.horizon_commit);
    let mut zip = zip::ZipWriter::new(fs::File::create(&out)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);
    let entries: [(&str, Vec<u8>); 5] = [
        (exe, fs::read(&binary)?),
        ("fish-edition.json", manifest.into_bytes()),
        ("LICENSE-HORIZON", fs::read(pristine.join("LICENSE"))?),
        (
            "THIRD_PARTY_NOTICES.md",
            fs::read(pristine.join("THIRD_PARTY_NOTICES.md"))?,
        ),
        ("LICENSE-FISH", fs::read(root.join("LICENSE"))?),
    ];
    for (name, data) in entries {
        zip.start_file(name, options)?;
        zip.write_all(&data)?;
    }
    zip.finish()?;
    eprintln!("wrote {}", out.display());
    Ok(out)
}

/// Builds the example mod in `examples/mods/<dir>` with cargo-fish; returns its `.fish`.
pub fn build_example_mod(root: &Path, dir: &str, file: &str) -> Result<PathBuf> {
    crate::run(root, "cargo", &["build", "--locked", "-p", "cargo-fish"])?;
    let exe = if cfg!(windows) {
        "cargo-fish.exe"
    } else {
        "cargo-fish"
    };
    let status = Command::new(root.join("target/debug").join(exe))
        .current_dir(root.join("examples/mods").join(dir))
        .args(["fish", "build"])
        .status()
        .context("running cargo-fish build")?;
    if !status.success() {
        bail!("building {dir} failed");
    }
    Ok(root.join("target/fish").join(file))
}

/// Builds `hello.fish` with cargo-fish; returns its path.
pub fn build_hello(root: &Path) -> Result<PathBuf> {
    build_example_mod(root, "hello", "hello-0.1.0.fish")
}

/// Runs the edition binary's self test with the hello mod; returns the report.
pub fn selftest(root: &Path, cfg: &FishConfig, profile: Profile) -> Result<serde_json::Value> {
    let exe = if cfg!(windows) {
        "horizon-client.exe"
    } else {
        "horizon-client"
    };
    let binary = horizon::horizon_dir(root)
        .join("target")
        .join(profile.name())
        .join(exe);
    if !binary.is_file() {
        bail!(
            "{} is missing: run `cargo xtask edition build` first",
            binary.display()
        );
    }
    let hello = build_hello(root)?;
    let scratch = root.join("target/selftest");
    if scratch.exists() {
        fs::remove_dir_all(&scratch)?;
    }
    fs::create_dir_all(&scratch)?;
    let report_path = scratch.join("report.json");
    let output = Command::new(&binary)
        .env("HORIZON_HOME", scratch.join("home"))
        .env("FISH_LOADER_ADD_MODS", &hello)
        .env("FISH_LOADER_SELFTEST", &report_path)
        .stdin(Stdio::null())
        .output()
        .context("running the edition")?;
    let text = fs::read_to_string(&report_path).with_context(|| {
        format!(
            "the edition exited with {} without writing a report\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    let report: serde_json::Value = serde_json::from_str(&text)?;
    verify_report(&report, cfg)?;
    if !output.status.success() {
        bail!("the edition's self test exited with {}", output.status);
    }
    Ok(report)
}

/// Test names with their outcome (`true` = passed) from `cargo test` output.
pub fn parse_test_results(output: &str) -> std::collections::BTreeMap<String, bool> {
    let re = Regex::new(r"^test (\S+) \.\.\. (ok|FAILED)$").expect("static regex");
    output
        .lines()
        .filter_map(|l| {
            re.captures(l.trim_end())
                .map(|c| (c[1].to_string(), &c[2] == "ok"))
        })
        .collect()
}

/// Tests that pass on the pristine source and fail (or vanish) on the staged one.
pub fn regressions(
    pristine: &std::collections::BTreeMap<String, bool>,
    staged: &std::collections::BTreeMap<String, bool>,
) -> Vec<String> {
    pristine
        .iter()
        .filter(|(_, ok)| **ok)
        .filter(|(name, _)| staged.get(*name) != Some(&true))
        .map(|(name, _)| name.clone())
        .collect()
}

fn run_horizon_tests(dir: &Path, target: &Path) -> Result<String> {
    let output = Command::new("cargo")
        .current_dir(dir)
        .args([
            "test",
            "--locked",
            "--no-fail-fast",
            "-p",
            "horizon-client",
            "--lib",
            "--bins",
        ])
        .env("CARGO_TARGET_DIR", target)
        .output()
        .context("running cargo test")?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Horizon's own tests on the pristine and the staged source; fails only on regressions.
pub fn run_regressions(root: &Path, cfg: &FishConfig) -> Result<()> {
    let pristine_dir = source_dir(root, &cfg.horizon_commit);
    let staged_dir = work_dir(root, &cfg.horizon_commit).join("staged");
    if !staged_dir.join("Cargo.toml").is_file() {
        bail!("nothing applied: run `cargo xtask edition mixins` first");
    }
    let base = horizon::horizon_dir(root);
    let pristine = parse_test_results(&run_horizon_tests(
        &pristine_dir,
        &base.join("target-pristine"),
    )?);
    let staged = parse_test_results(&run_horizon_tests(
        &staged_dir,
        &base.join("target-staged"),
    )?);
    eprintln!(
        "{} tests on the pristine source, {} on the staged one",
        pristine.len(),
        staged.len()
    );
    let broken = regressions(&pristine, &staged);
    if broken.is_empty() {
        return Ok(());
    }
    for name in &broken {
        eprintln!("regression: {name}");
    }
    bail!(
        "{} test(s) pass on pristine Horizon and fail on the edition",
        broken.len()
    )
}

/// Checks the self test report the way `edition selftest` requires.
pub fn verify_report(report: &serde_json::Value, cfg: &FishConfig) -> Result<()> {
    if report["ok"] != true {
        bail!("the self test reported failures: {}", report["checks"]);
    }
    if report["mods"][0]["id"] != "hello" || report["mods"][0]["status"] != "ok" {
        bail!("hello did not load: {}", report["mods"]);
    }
    if report["checks"]
        .as_array()
        .is_none_or(|c| c.is_empty() || c.iter().any(|c| c["ok"] != true))
    {
        bail!("a check failed: {}", report["checks"]);
    }
    if report["fish"] != cfg.version.as_str() {
        bail!(
            "the edition reports Fish {}, expected {}",
            report["fish"],
            cfg.version
        );
    }
    Ok(())
}

/// Lines of a game log that mention `needle`.
pub fn log_lines_with<'a>(log: &'a str, needle: &str) -> Vec<&'a str> {
    log.lines().filter(|l| l.contains(needle)).collect()
}

/// The block `distance` blocks along the view of `player` (its `eye`, `yaw` and `pitch` as
/// `horizon-ctl state` reports them), from the eyes.
pub fn block_ahead(player: &serde_json::Value, distance: f64) -> Option<[i64; 3]> {
    let eye = player["eye"].as_array()?;
    let eye = [
        eye.first()?.as_f64()?,
        eye.get(1)?.as_f64()?,
        eye.get(2)?.as_f64()?,
    ];
    let (yaw, pitch) = (
        player["yaw"].as_f64()?.to_radians(),
        player["pitch"].as_f64()?.to_radians(),
    );
    let dir = [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ];
    Some([0, 1, 2].map(|i| (eye[i] + dir[i] * distance).floor() as i64))
}

/// Checks the e2e expectations against `latest.log`: the hello mod's tick 100 and its command.
pub fn verify_e2e_log(log: &str) -> Result<()> {
    let tick = log_lines_with(log, "tick 100");
    if !tick.iter().any(|l| l.contains("[fish/hello]")) {
        bail!("latest.log has no `[fish/hello] tick 100` line");
    }
    if log_lines_with(log, "Fish says hello").is_empty() {
        bail!("latest.log has no `Fish says hello` line");
    }
    Ok(())
}

/// Checks what the `gamerule_demo` mod logged during the e2e scenarios: its rules at the start,
/// the late registration refusal and the change the `/gamerule` command made.
pub fn verify_gamerule_log(log: &str) -> Result<()> {
    let demo = |needle: &str| {
        log.lines()
            .any(|l| l.contains("[fish/gamerule_demo]") && l.contains(needle))
    };
    for needle in [
        "rules at start: allow_stone_break=Ok(true) sparkle_limit=Ok(8)",
        "late game rule refused: invalid argument: registries are frozen",
        "sparkle_limit changed Int(8) -> Int(12)",
    ] {
        if !demo(needle) {
            bail!("latest.log has no `[fish/gamerule_demo] ... {needle} ...` line");
        }
    }
    Ok(())
}

/// The `content_demo` lines of `log` that contain every needle, or the first missing one.
fn content_demo_lines(log: &str, needles: &[&str]) -> Result<()> {
    for needle in needles {
        if !log
            .lines()
            .any(|l| l.contains("[fish/content_demo]") && l.contains(needle))
        {
            bail!("latest.log has no `[fish/content_demo] ... {needle} ...` line");
        }
    }
    Ok(())
}

/// Checks what the `content_demo` mod logged during the e2e scenarios: the state ids Horizon gave
/// its block, the late registration refusal, and the use, place and break of the ruby block.
pub fn verify_blocks_log(log: &str) -> Result<()> {
    content_demo_lines(
        log,
        &[
            "ruby states ",
            "late block refused: invalid argument: registries are frozen",
            "ruby block used at",
            "ruby block placed at",
            "ruby block broken at",
        ],
    )?;
    if log
        .lines()
        .any(|l| l.contains("[fish/content_demo]") && l.contains("ruby states unknown"))
    {
        bail!("the loader did not learn the state ids of the ruby block");
    }
    Ok(())
}

/// What the second run of the e2e world logged: the ruby block still has its state ids.
pub fn verify_blocks_reload_log(log: &str) -> Result<()> {
    content_demo_lines(log, &["ruby states "])?;
    if log.lines().any(|l| l.contains("ruby states unknown")) {
        bail!("the loader did not learn the state ids of the ruby block after the reload");
    }
    Ok(())
}

/// Checks what the `events_demo` mod logged during the e2e scenarios: the lifecycle and join events,
/// the chat filter and rewrite, the block-break veto and the data reload.
pub fn verify_events_log(log: &str) -> Result<()> {
    let demo = |needle: &str| {
        log.lines()
            .any(|l| l.contains("[fish/events_demo]") && l.contains(needle))
    };
    for needle in [
        "client started",
        "server starting: FishE2E",
        "world loaded: minecraft:overworld",
        "server started",
        "welcome ",
        "dropped a message from",
        "refusing to break bedrock",
        "break canceled at",
        "reload started",
        "reload finished",
    ] {
        if !demo(needle) {
            bail!("latest.log has no `[fish/events_demo] ... {needle} ...` line");
        }
    }
    if !log
        .lines()
        .any(|l| l.contains("[CHAT]") && l.contains("hello ><> world"))
    {
        bail!("the chat line with `:fish:` was not rewritten to `><>`");
    }
    if log
        .lines()
        .any(|l| l.contains("[CHAT]") && l.contains("badword"))
    {
        bail!("the chat line with `badword` was not dropped");
    }
    Ok(())
}

/// What a clean shutdown of the e2e game logged: server stopping and stopped, the player leaving,
/// then the client stopping and stopped.
pub fn verify_shutdown_log(log: &str) -> Result<()> {
    let mut at = 0;
    for needle in [
        "server stopping",
        "goodbye ",
        "server stopped",
        "client stopping",
        "client stopped",
    ] {
        let Some(i) = log
            .lines()
            .skip(at)
            .position(|l| l.contains("[fish/events_demo]") && l.contains(needle))
        else {
            bail!("latest.log has no `[fish/events_demo] ... {needle} ...` line in order");
        };
        at += i + 1;
    }
    Ok(())
}

/// Runs `cmd` to completion, killing it after `secs` seconds; returns its output.
///
/// Output goes through files, not pipes: a child that outlives the command (horizon-ctl `launch`
/// leaves the game running) would otherwise keep a pipe open and block the read forever.
pub fn output_within(mut cmd: Command, secs: u64) -> Result<std::process::Output> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let base = std::env::temp_dir().join(format!(
        "fish-xtask-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let (out_path, err_path) = (base.with_extension("out"), base.with_extension("err"));
    cmd.stdout(fs::File::create(&out_path)?)
        .stderr(fs::File::create(&err_path)?)
        .stdin(Stdio::null());
    let mut child = cmd.spawn().context("starting a process")?;
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed().as_secs() >= secs {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    let stdout = fs::read(&out_path).unwrap_or_default();
    let stderr = fs::read(&err_path).unwrap_or_default();
    let _ = fs::remove_file(&out_path);
    let _ = fs::remove_file(&err_path);
    match status {
        Some(status) => Ok(std::process::Output {
            status,
            stdout,
            stderr,
        }),
        None => bail!("timed out after {secs}s"),
    }
}

fn ctl(ctl: &Path, home: &Path, args: &[&str]) -> Result<String> {
    eprintln!("$ horizon-ctl {}", args.join(" "));
    let mut cmd = Command::new(ctl);
    cmd.args(args).env("HORIZON_HOME", home);
    let out = output_within(cmd, 120)
        .with_context(|| format!("running horizon-ctl {}", args.join(" ")))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        bail!(
            "horizon-ctl {} failed ({}):\n{text}",
            args.join(" "),
            out.status
        );
    }
    Ok(text)
}

/// Local in-game end to end test: launch the edition with prepared resources, wait 200 ticks, run
/// `fish hello`, and check the log and a screenshot.
pub fn e2e(
    root: &Path,
    cfg: &FishConfig,
    profile: Profile,
    resources: &Path,
    horizon_ctl: &Path,
) -> Result<()> {
    let exe = if cfg!(windows) {
        "horizon-client.exe"
    } else {
        "horizon-client"
    };
    let game = horizon::horizon_dir(root)
        .join("target")
        .join(profile.name())
        .join(exe);
    if !game.is_file() {
        bail!(
            "{} is missing: run `cargo xtask edition build` first",
            game.display()
        );
    }
    let _ = cfg;
    let hello = build_hello(root)?;
    let demo = build_example_mod(root, "events-demo", "events_demo-0.1.0.fish")?;
    let rules = build_example_mod(root, "gamerule-demo", "gamerule_demo-0.1.0.fish")?;
    let content = build_example_mod(root, "content-demo", "content_demo-0.1.0.fish")?;
    let mods =
        std::env::join_paths([&hello, &demo, &rules, &content]).context("joining the mod paths")?;
    let scratch = root.join("target/e2e");
    if scratch.exists() {
        fs::remove_dir_all(&scratch)?;
    }
    let home = scratch.join("home");
    fs::create_dir_all(&home)?;
    let launched = launch_game(horizon_ctl, &game, &scratch, resources, &mods);
    let outcome = launched.and_then(|()| {
        ctl(horizon_ctl, &home, &["wait", "200"])?;
        ctl(horizon_ctl, &home, &["command", "fish hello"])?;
        // Chat: one line the demo drops, one it rewrites.
        ctl(horizon_ctl, &home, &["chat", "this has a badword in it"])?;
        ctl(horizon_ctl, &home, &["chat", "hello :fish: world"])?;
        // A Fish command registered on the server, run as the player (feedback comes back as chat).
        let feedback = ctl(horizon_ctl, &home, &["command", "fishdemo hello"])?;
        if !feedback.contains("hello ") {
            bail!("`fishdemo hello` gave no feedback: {feedback}");
        }
        let failure = ctl(horizon_ctl, &home, &["command", "fishdemo fail"])?;
        if !failure.contains("on purpose") {
            bail!("`fishdemo fail` did not report its error: {failure}");
        }
        // Bedrock two blocks along the view from the eyes (the crosshair's block), then mined: the
        // demo vetoes the break.
        let state: serde_json::Value = serde_json::from_str(&ctl(horizon_ctl, &home, &["state"])?)
            .context("parsing horizon-ctl state")?;
        let at =
            block_ahead(&state["player"], 2.0).context("the player's eye and view in `state`")?;
        let place = format!("setblock {} {} {} minecraft:bedrock", at[0], at[1], at[2]);
        ctl(horizon_ctl, &home, &["command", &place])?;
        ctl(horizon_ctl, &home, &["wait", "5"])?;
        ctl(horizon_ctl, &home, &["mouse", "left", "click"])?;
        ctl(horizon_ctl, &home, &["wait", "10"])?;
        // A game rule of a mod: the command tree knows it (`/help`), `/gamerule` queries and sets it,
        // a value out of its bounds is refused, and the mod sees the change.
        let rule = "gamerule_demo:sparkle_limit";
        let gamerule = |line: &str| ctl(horizon_ctl, &home, &["command", line]);
        let help = gamerule("help gamerule")?;
        if !help.contains(rule) {
            bail!("`/help gamerule` does not list {rule}: {help}");
        }
        let query = gamerule(&format!("gamerule {rule}"))?;
        if !(query.contains(rule) && query.contains("currently set to") && query.contains('8')) {
            bail!("`/gamerule {rule}` did not report the default 8: {query}");
        }
        let set = gamerule(&format!("gamerule {rule} 12"))?;
        if !(set.contains("now set to") && set.contains("12")) {
            bail!("`/gamerule {rule} 12` did not confirm: {set}");
        }
        gamerule(&format!("gamerule {rule} 99"))?;
        let kept = gamerule(&format!("gamerule {rule}"))?;
        if !kept.contains("12") {
            bail!("a value out of bounds changed {rule}: {kept}");
        }
        let mode = gamerule("gamerule gamerule_demo:difficulty_mode hard")?;
        if !mode.contains("hard") {
            bail!("`/gamerule gamerule_demo:difficulty_mode hard` did not confirm: {mode}");
        }
        // A block of a mod in the ground under the player (looking straight down): set in the
        // world (the game knows its id), used with a right click, placed from its item next to
        // it and broken; the mod logs each.
        ctl(horizon_ctl, &home, &["look", "0", "90"])?;
        ctl(horizon_ctl, &home, &["wait", "5"])?;
        let state: serde_json::Value = serde_json::from_str(&ctl(horizon_ctl, &home, &["state"])?)
            .context("parsing horizon-ctl state")?;
        let ruby_at =
            block_ahead(&state["player"], 2.0).context("the player's eye and view in `state`")?;
        let [x, y, z] = ruby_at.map(|c| c.to_string());
        let ruby = "content_demo:ruby_block";
        let setblock = |at: [String; 3], block: &str| {
            let line = format!("setblock {} {} {} {block}", at[0], at[1], at[2]);
            ctl(horizon_ctl, &home, &["command", &line])
        };
        setblock([x.clone(), y.clone(), z.clone()], ruby)?;
        ctl(horizon_ctl, &home, &["wait", "5"])?;
        let seen = ctl(horizon_ctl, &home, &["block", &x, &y, &z])?;
        if !seen.contains(ruby) {
            bail!("`/setblock {x} {y} {z} {ruby}` did not put the block there: {seen}");
        }
        ctl(horizon_ctl, &home, &["interact", &x, &y, &z])?;
        ctl(horizon_ctl, &home, &["wait", "5"])?;
        // Next to it: firm ground and free air above it, for the item to be placed on.
        let (east, above) = ((ruby_at[0] + 1).to_string(), (ruby_at[1] + 1).to_string());
        setblock([east.clone(), y.clone(), z.clone()], "minecraft:stone")?;
        setblock([east.clone(), above.clone(), z.clone()], "minecraft:air")?;
        ctl(horizon_ctl, &home, &["command", &format!("give @s {ruby}")])?;
        ctl(
            horizon_ctl,
            &home,
            &["place", &east, &above, &z, "--item", ruby],
        )?;
        ctl(horizon_ctl, &home, &["wait", "5"])?;
        ctl(horizon_ctl, &home, &["break", &east, &above, &z])?;
        ctl(horizon_ctl, &home, &["wait", "10"])?;
        ctl(horizon_ctl, &home, &["wait", "5"])?;
        // A data reload, then the F3 screen (it lists `events-demo is listening`) for the screenshot.
        ctl(horizon_ctl, &home, &["command", "reload"])?;
        ctl(horizon_ctl, &home, &["wait", "40"])?;
        ctl(horizon_ctl, &home, &["key", "f3", "tap"])?;
        ctl(horizon_ctl, &home, &["wait", "30"])?;
        let shot = scratch.join("e2e.png");
        ctl(
            horizon_ctl,
            &home,
            &["screenshot", &shot.display().to_string()],
        )?;
        let log = fs::read_to_string(home.join("logs/latest.log")).context("reading latest.log")?;
        verify_e2e_log(&log)?;
        verify_events_log(&log)?;
        verify_gamerule_log(&log)?;
        verify_blocks_log(&log)?;
        if !shot.is_file() {
            bail!("no screenshot was written to {}", shot.display());
        }
        Ok(ruby_at)
    });
    // Whatever happened, do not leave a game window open; a clean quit logs the shutdown events.
    let _ = ctl(horizon_ctl, &home, &["quit"]);
    let ruby_at = outcome.and_then(|at| {
        verify_shutdown_log(&wait_for_stop(&home)?)?;
        Ok(at)
    })?;
    // The world was saved with the ruby block in it: a second run opens it again.
    launch_game(horizon_ctl, &game, &scratch, resources, &mods)?;
    let reopened = ctl(horizon_ctl, &home, &["wait", "100"]).and_then(|_| {
        let at = ruby_at.map(|c| c.to_string());
        let block = ctl(horizon_ctl, &home, &["block", &at[0], &at[1], &at[2]])?;
        if !block.contains("content_demo:ruby_block") {
            bail!(
                "the saved world lost the ruby block at {}: {block}",
                at.join(" ")
            );
        }
        let log = fs::read_to_string(home.join("logs/latest.log")).context("reading latest.log")?;
        verify_blocks_reload_log(&log)
    });
    let _ = ctl(horizon_ctl, &home, &["quit"]);
    let _ = wait_for_stop(&home);
    reopened
}

/// Waits for the game to log `client stopped` (a clean quit) and returns its log.
fn wait_for_stop(home: &Path) -> Result<String> {
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let log = fs::read_to_string(home.join("logs/latest.log")).unwrap_or_default();
        if log.contains("client stopped") {
            return Ok(log);
        }
    }
    bail!("the game did not log `client stopped` after quit")
}

/// Launches the edition (`horizon-ctl launch`) in `scratch` with `mods` and waits for its world.
fn launch_game(
    horizon_ctl: &Path,
    game: &Path,
    scratch: &Path,
    resources: &Path,
    mods: &std::ffi::OsStr,
) -> Result<()> {
    let home = scratch.join("home");
    // --no-pause: the game stops ticking when its window loses focus, and nothing here focuses it.
    let mut launch_cmd = Command::new(horizon_ctl);
    launch_cmd
        .args(["launch", "--game"])
        .arg(game)
        .args([
            "--world",
            "FishE2E",
            "--creative",
            "--offscreen",
            "--wait",
            "--resources",
        ])
        .arg(resources)
        .arg("--no-pause")
        // The game keeps its saves and options in its working directory: the scratch one, so a
        // run always starts a fresh world and leaves the repository alone.
        .current_dir(scratch)
        .env("HORIZON_HOME", &home)
        .env("FISH_LOADER_ADD_MODS", mods);
    let out = output_within(launch_cmd, 600)?;
    if !out.status.success() {
        bail!(
            "horizon-ctl launch failed ({}):\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> FishConfig {
        crate::config::parse(
            "[fish]\nversion=\"0.1.0\"\nabi={major=0,minor=1}\n[horizon]\ncommit=\"6f8c7a0e5d531d3834c4d0be4659a1db8e7e5c2a\"\nsource_tag=\"nightly-source/2026-10-28-6f8c7a0\"\nrelease_line=\"0.2\"\nrelease_tag=\"\"\n[mixins]\ngit=\"g\"\nrev=\"r\"\n",
        )
        .unwrap()
    }

    #[test]
    fn names_and_versions() {
        let c = cfg();
        assert_eq!(horizon_version(&c), "0.2.0-nightly.20261028+6f8c7a0");
        assert_eq!(edition_id(&c), "0.1.0+horizon.6f8c7a0");
        assert_eq!(
            archive_name(&c, "windows-x86_64"),
            "fish-edition-0.1.0-horizon-6f8c7a0-windows-x86_64.zip"
        );
        let mut no_date = cfg();
        no_date.horizon_source_tag = "custom".into();
        assert_eq!(horizon_version(&no_date), "0.2.0-dev+6f8c7a0");
        assert!(semver_ok(&horizon_version(&c)));
        assert_eq!(Profile::Fast.name(), "fast");
        assert_eq!(Profile::Release.name(), "release");
    }

    fn semver_ok(v: &str) -> bool {
        // MAJOR.MINOR.PATCH-pre+build
        Regex::new(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$")
            .unwrap()
            .is_match(v)
    }

    #[test]
    fn placeholders_are_filled() {
        let out = fill_manifest("v=\"@FISH_VERSION@\" r=\"@HORIZON_COMMIT@\"", &cfg());
        assert_eq!(
            out,
            "v=\"0.1.0\" r=\"6f8c7a0e5d531d3834c4d0be4659a1db8e7e5c2a\""
        );
    }

    #[test]
    fn the_bridge_is_assembled_into_inline_modules() {
        let dir = std::env::temp_dir().join(format!("fish-xtask-bridge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("b.rs"), "pub fn b() {}\n").unwrap();
        fs::write(dir.join("a.rs"), "pub fn a() {}\n").unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let out = assemble_bridge(&dir).unwrap();
        let (a, b) = (out.find("mod a {").unwrap(), out.find("mod b {").unwrap());
        assert!(a < b, "sorted by file name");
        assert!(out.contains("pub(crate) mod a {\npub fn a() {}\n}"));
        assert!(!out.contains("ignored"));
        let empty = dir.join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(assemble_bridge(&empty).is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn tree_hash_depends_on_names_and_contents() {
        let dir = std::env::temp_dir().join(format!("fish-xtask-hash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a"), "1").unwrap();
        fs::write(dir.join("sub/b"), "2").unwrap();
        let h1 = hash_tree(&dir).unwrap();
        assert_eq!(h1.len(), 64);
        assert_eq!(h1, hash_tree(&dir).unwrap());
        fs::write(dir.join("sub/b"), "3").unwrap();
        assert_ne!(h1, hash_tree(&dir).unwrap());
        assert_eq!(hash_file(&dir.join("a")).unwrap().len(), 64);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_output_is_parsed_and_compared() {
        let pristine = parse_test_results(
            "running 4 tests\ntest a::one ... ok\ntest a::two ... ok\ntest a::three ... FAILED\ntest a::four ... ok\nnoise\n",
        );
        assert_eq!(pristine.len(), 4);
        assert!(!pristine["a::three"]);
        let staged = parse_test_results(
            "test a::one ... ok\ntest a::two ... FAILED\ntest a::three ... ok\n",
        );
        // two fails now; three failed before (not a regression); four vanished (a regression)
        assert_eq!(regressions(&pristine, &staged), ["a::four", "a::two"]);
        assert!(regressions(&pristine, &pristine).is_empty());
    }

    #[test]
    fn sync_touches_only_changed_files() {
        let base = std::env::temp_dir().join(format!("fish-xtask-sync-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (from, to) = (base.join("from"), base.join("to"));
        fs::create_dir_all(from.join("d")).unwrap();
        fs::write(from.join("same"), "1").unwrap();
        fs::write(from.join("d/changed"), "new").unwrap();
        fs::write(from.join("added"), "a").unwrap();
        fs::create_dir_all(to.join("d")).unwrap();
        fs::write(to.join("same"), "1").unwrap();
        fs::write(to.join("d/changed"), "old").unwrap();
        fs::write(to.join("gone"), "x").unwrap();
        let before = fs::metadata(to.join("same")).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(sync_tree(&from, &to).unwrap(), (2, 1));
        assert_eq!(
            fs::metadata(to.join("same")).unwrap().modified().unwrap(),
            before
        );
        assert_eq!(fs::read_to_string(to.join("d/changed")).unwrap(), "new");
        assert!(to.join("added").is_file() && !to.join("gone").exists());
        assert_eq!(sync_tree(&from, &to).unwrap(), (0, 0));
        // a destination that does not exist yet is created
        assert_eq!(sync_tree(&from, &base.join("fresh")).unwrap().0, 3);
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn output_within_captures_and_times_out() {
        let mut ok = Command::new(if cfg!(windows) { "cmd" } else { "sh" });
        ok.args(if cfg!(windows) {
            ["/C", "echo hi"]
        } else {
            ["-c", "echo hi"]
        });
        let out = output_within(ok, 30).unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi");
        let mut slow = Command::new(if cfg!(windows) { "ping" } else { "sleep" });
        slow.args(if cfg!(windows) {
            vec!["-n", "30", "127.0.0.1"]
        } else {
            vec!["30"]
        });
        let err = output_within(slow, 1).unwrap_err();
        assert!(err.to_string().contains("timed out after 1s"), "{err}");
    }

    #[test]
    fn e2e_log_expectations() {
        let good = "[12:00:00] [Render thread/INFO]: [fish/hello] tick 100\n[12:00:01] [Render thread/INFO]: [fish/hello] Fish says hello\n";
        assert!(verify_e2e_log(good).is_ok());
        assert!(
            verify_e2e_log("Fish says hello")
                .unwrap_err()
                .to_string()
                .contains("tick 100")
        );
        assert!(
            verify_e2e_log("(other) tick 100\nFish says hello").is_err(),
            "tick 100 must come from fish/hello"
        );
        assert!(
            verify_e2e_log("[fish/hello] tick 100")
                .unwrap_err()
                .to_string()
                .contains("Fish says hello")
        );
        assert_eq!(log_lines_with("a\nb c\nc", "c"), ["b c", "c"]);
    }

    #[test]
    fn gamerule_log_expectations() {
        let line = |s: &str| format!("[12:00:00] [Server thread/INFO]: [fish/gamerule_demo] {s}\n");
        let mut log = line("rules at start: allow_stone_break=Ok(true) sparkle_limit=Ok(8) x");
        log += &line("late game rule refused: invalid argument: registries are frozen");
        assert!(
            verify_gamerule_log(&log)
                .unwrap_err()
                .to_string()
                .contains("sparkle_limit changed")
        );
        log += &line("sparkle_limit changed Int(8) -> Int(12)");
        assert!(verify_gamerule_log(&log).is_ok());
        // another mod's line does not count
        let other = log.replace("gamerule_demo", "other");
        assert!(verify_gamerule_log(&other).is_err());
    }

    #[test]
    fn blocks_log_expectations() {
        let line = |s: &str| format!("[12:00:00] [Server thread/INFO]: [fish/content_demo] {s}\n");
        let mut log = line("ruby states 35724..35725");
        log += &line("late block refused: invalid argument: registries are frozen");
        log += &line("ruby block used at 1 2 3 by Steve face Some(Up)");
        log += &line("ruby block placed at 1 3 3");
        assert!(
            verify_blocks_log(&log)
                .unwrap_err()
                .to_string()
                .contains("ruby block broken at")
        );
        log += &line("ruby block broken at 1 3 3");
        assert!(verify_blocks_log(&log).is_ok());
        assert!(verify_blocks_log(&log.replace("content_demo", "other")).is_err());
        // the second run only needs the state ids again
        assert!(verify_blocks_reload_log(&line("ruby states 35724..35725")).is_ok());
        assert!(verify_blocks_reload_log("").is_err());
        let unknown = line("ruby states unknown: not found");
        assert!(verify_blocks_reload_log(&unknown).is_err());
        assert!(verify_blocks_log(&(log + &unknown)).is_err());
    }

    #[test]
    fn the_block_ahead_follows_the_view() {
        let player = serde_json::json!({
            "eye": [-29.5, 66.62, 0.5], "yaw": -163.8, "pitch": -0.9
        });
        assert_eq!(block_ahead(&player, 2.0), Some([-29, 66, -2]));
        assert_eq!(block_ahead(&serde_json::json!({}), 2.0), None);
    }

    #[test]
    fn events_log_expectations() {
        let line = |s: &str| format!("[12:00:00] [Server thread/INFO]: [fish/events_demo] {s}\n");
        let mut log = String::new();
        for s in [
            "client started",
            "server starting: FishE2E",
            "world loaded: minecraft:overworld",
            "server started",
            "welcome Bob",
            "dropped a message from Bob",
            "refusing to break bedrock at 1 2 3",
            "break canceled at 1 2 3",
            "reload started",
            "reload finished",
        ] {
            log += &line(s);
        }
        assert!(
            verify_events_log(&log)
                .unwrap_err()
                .to_string()
                .contains("`:fish:`")
        );
        log += "[12:00:01] [Render thread/INFO]: [CHAT] <Bob> hello ><> world\n";
        assert!(verify_events_log(&log).is_ok());
        let missing = log.replace("reload finished", "x");
        assert!(
            verify_events_log(&missing)
                .unwrap_err()
                .to_string()
                .contains("reload finished")
        );
        let dropped = format!("{log}[CHAT] <Bob> a badword\n");
        assert!(verify_events_log(&dropped).is_err());

        let mut down = String::new();
        for s in [
            "server stopping",
            "goodbye Bob",
            "server stopped",
            "client stopping",
            "client stopped",
        ] {
            down += &line(s);
        }
        assert!(verify_shutdown_log(&down).is_ok());
        let shuffled = format!("{}{}", line("client stopped"), line("server stopping"));
        assert!(verify_shutdown_log(&shuffled).is_err());
    }

    #[test]
    fn report_verification() {
        let c = cfg();
        let good = serde_json::json!({
            "ok": true, "fish": "0.1.0",
            "mods": [{"id": "hello", "status": "ok"}],
            "checks": [{"name": "x", "ok": true}]
        });
        assert!(verify_report(&good, &c).is_ok());
        let mut bad = good.clone();
        bad["ok"] = false.into();
        assert!(
            verify_report(&bad, &c)
                .unwrap_err()
                .to_string()
                .contains("failures")
        );
        let mut bad = good.clone();
        bad["mods"][0]["status"] = "failed".into();
        assert!(
            verify_report(&bad, &c)
                .unwrap_err()
                .to_string()
                .contains("hello did not load")
        );
        let mut bad = good.clone();
        bad["checks"] = serde_json::json!([]);
        assert!(verify_report(&bad, &c).is_err());
        let mut bad = good;
        bad["fish"] = "9.9.9".into();
        assert!(
            verify_report(&bad, &c)
                .unwrap_err()
                .to_string()
                .contains("expected 0.1.0")
        );
    }
}
