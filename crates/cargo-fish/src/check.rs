//! `cargo fish check`: load a mod in a throwaway host, in a child process.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use fish_loader::BootReport;
use fish_test_host::TestHost;

/// Boots a host with only `file` and prints the [`BootReport`] as JSON (the child side).
pub fn child(file: &Path) -> Result<()> {
    let host = TestHost::new();
    let host = if file.is_dir() {
        host.add_dev_mod(file)
    } else {
        host.add_mod(file)
    };
    let host = host.boot().map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("{}", serde_json::to_string(&host.loader().report())?);
    Ok(())
}

/// Summarises a report; returns the lines and whether everything is fine.
pub fn summarize(file: &Path, report: &BootReport) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut ok = report.errors.is_empty();
    for m in &report.mods {
        match &m.reason {
            None => lines.push(format!(
                "{}: mod {} {} loaded ({})",
                file.display(),
                m.id,
                m.version,
                m.status
            )),
            Some(reason) => {
                ok = false;
                lines.push(format!(
                    "{}: mod {} {} FAILED: {reason}",
                    file.display(),
                    m.id,
                    m.version
                ));
            }
        }
    }
    if report.mods.is_empty() {
        ok = false;
        lines.push(format!("{}: no mod was loaded", file.display()));
    }
    lines.extend(report.warnings.iter().map(|w| format!("warning: {w}")));
    lines.extend(report.errors.iter().map(|e| format!("error: {e}")));
    (lines, ok)
}

/// Runs the check in a child process (`exe __check-child <file>`) so that a crashing library
/// cannot take the tool down. Returns the process exit code (0 = fine).
pub fn check(exe: &Path, file: &Path, out: &mut dyn std::io::Write) -> Result<i32> {
    let output = Command::new(exe)
        .arg("__check-child")
        .arg(file)
        .output()
        .context("starting the check process")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let report = stdout
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<BootReport>(l).ok());
    match (output.status.success(), report) {
        (true, Some(report)) => {
            let (lines, ok) = summarize(file, &report);
            for l in lines {
                writeln!(out, "{l}")?;
            }
            Ok(i32::from(!ok))
        }
        _ => {
            writeln!(
                out,
                "{}: the check process failed ({}); the mod probably crashed the host\n{}",
                file.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim_end()
            )?;
            Ok(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fish_loader::{AbiReport, ModReport};

    fn report(mods: Vec<ModReport>, errors: Vec<String>) -> BootReport {
        BootReport {
            fish: "0.1.0".into(),
            abi: AbiReport { major: 0, minor: 1 },
            side: "client".into(),
            mods,
            warnings: vec!["careful".into()],
            errors,
        }
    }

    fn m(status: &str, reason: Option<&str>) -> ModReport {
        ModReport {
            id: "demo".into(),
            version: "1.0.0".into(),
            status: status.into(),
            reason: reason.map(str::to_string),
        }
    }

    #[test]
    fn summaries() {
        let f = Path::new("demo.fish");
        let (lines, ok) = summarize(f, &report(vec![m("ok", None)], vec![]));
        assert!(ok);
        assert_eq!(lines[0], "demo.fish: mod demo 1.0.0 loaded (ok)");
        assert_eq!(lines[1], "warning: careful");

        let (lines, ok) = summarize(
            f,
            &report(vec![m("failed", Some("boom"))], vec!["e".into()]),
        );
        assert!(!ok);
        assert!(lines[0].ends_with("FAILED: boom") && lines.last().unwrap() == "error: e");

        let (lines, ok) = summarize(f, &report(vec![], vec![]));
        assert!(!ok && lines[0].contains("no mod was loaded"));
    }
}
