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

/// Builds `hello.fish` with cargo-fish; returns its path.
pub fn build_hello(root: &Path) -> Result<PathBuf> {
    crate::run(root, "cargo", &["build", "--locked", "-p", "cargo-fish"])?;
    let exe = if cfg!(windows) {
        "cargo-fish.exe"
    } else {
        "cargo-fish"
    };
    let status = Command::new(root.join("target/debug").join(exe))
        .current_dir(root.join("examples/mods/hello"))
        .args(["fish", "build"])
        .status()
        .context("running cargo-fish build")?;
    if !status.success() {
        bail!("building hello.fish failed");
    }
    Ok(root.join("target/fish/hello-0.1.0.fish"))
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
    let scratch = root.join("target/e2e");
    if scratch.exists() {
        fs::remove_dir_all(&scratch)?;
    }
    let home = scratch.join("home");
    fs::create_dir_all(&home)?;
    // --no-pause: the game stops ticking when its window loses focus, and nothing here focuses it.
    let mut launch_cmd = Command::new(horizon_ctl);
    launch_cmd
        .args(["launch", "--game"])
        .arg(&game)
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
        .env("HORIZON_HOME", &home)
        .env("FISH_LOADER_ADD_MODS", &hello);
    let launched = output_within(launch_cmd, 600).and_then(|out| {
        if out.status.success() {
            Ok(())
        } else {
            bail!(
                "horizon-ctl launch failed ({}):\n{}{}",
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        }
    });
    let outcome = launched.and_then(|()| {
        ctl(horizon_ctl, &home, &["wait", "200"])?;
        ctl(horizon_ctl, &home, &["command", "fish hello"])?;
        ctl(horizon_ctl, &home, &["wait", "5"])?;
        let shot = scratch.join("e2e.png");
        ctl(
            horizon_ctl,
            &home,
            &["screenshot", &shot.display().to_string()],
        )?;
        let log = fs::read_to_string(home.join("logs/latest.log")).context("reading latest.log")?;
        verify_e2e_log(&log)?;
        if !shot.is_file() {
            bail!("no screenshot was written to {}", shot.display());
        }
        Ok(())
    });
    // Whatever happened, do not leave a game window open.
    let _ = ctl(horizon_ctl, &home, &["quit"]);
    outcome
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
