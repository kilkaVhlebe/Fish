//! `tests/*.toml` scripts (schema v1), as run by `cargo fish test`.
//!
//! ```toml
//! [[step]]
//! ticks = 20                         # advance the simulated game
//! command = "fish hello"             # run a client command
//! [step.expect]
//! logs = ["Hello from main"]         # substrings that must have been logged
//! stopped = ["panicky"]              # mods that must have failed
//! ```
//!
//! Unknown keys are errors. Within a step the actions run in a fixed order: `key`, `command`,
//! `chat`, `ticks`, `frames`; then `expect` is checked. Later phases add keys.

use std::fmt;

use serde::Deserialize;

use crate::TestHost;
use fish_loader::ModState;

/// A key press or release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyStep {
    /// Key name, e.g. `H`.
    pub name: String,
    /// True for press, false for release.
    pub pressed: bool,
}

/// What must hold after a step.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Substrings that must appear in log messages (all mods, since boot).
    #[serde(default)]
    pub logs: Vec<String>,
    /// Substrings that must appear in chat shown to the player.
    #[serde(default)]
    pub chat: Vec<String>,
    /// Substrings that must appear in text drawn on the HUD.
    #[serde(default)]
    pub hud: Vec<String>,
    /// Substrings that must appear in command feedback.
    #[serde(default)]
    pub command_feedback: Vec<String>,
    /// Mods that must be in the failed state.
    #[serde(default)]
    pub stopped: Vec<String>,
}

/// One step of a script.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Client ticks to advance.
    pub ticks: Option<u32>,
    /// Frames to render.
    pub frames: Option<u32>,
    /// A client command, without a leading `/`.
    pub command: Option<String>,
    /// A chat message the player sends.
    pub chat: Option<String>,
    /// A key event.
    pub key: Option<KeyStep>,
    /// Checks after the actions.
    pub expect: Option<Expect>,
}

/// A parsed script.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    /// Optional display name.
    pub name: Option<String>,
    /// The steps, in order.
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
}

/// Parses a script; unknown keys are errors.
pub fn parse(text: &str) -> Result<Script, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}

/// The first failed expectation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFailure {
    /// 1-based step number.
    pub step: usize,
    /// Which expectation (`expect.logs`, …).
    pub what: String,
    /// What the script wanted.
    pub expected: String,
    /// What was observed.
    pub actual: String,
}

impl fmt::Display for ScriptFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "step {}: {} failed\n  expected: {}\n  actual:   {}",
            self.step, self.what, self.expected, self.actual
        )
    }
}

impl std::error::Error for ScriptFailure {}

fn missing(needles: &[String], haystack: &[String]) -> Option<String> {
    needles
        .iter()
        .find(|n| !haystack.iter().any(|h| h.contains(n.as_str())))
        .cloned()
}

/// Runs `script` against a booted host; returns the first failure.
pub fn run(host: &mut TestHost, script: &Script) -> Result<(), ScriptFailure> {
    for (i, step) in script.steps.iter().enumerate() {
        let n = i + 1;
        if let Some(k) = &step.key {
            host.key(&k.name, k.pressed);
        }
        if let Some(c) = &step.command {
            host.command(c);
        }
        if let Some(c) = &step.chat {
            host.chat(c);
        }
        if let Some(t) = step.ticks {
            host.tick(t);
        }
        if let Some(f) = step.frames {
            host.frame(f);
        }
        let Some(expect) = &step.expect else { continue };
        let logs: Vec<String> = host.logs().into_iter().map(|r| r.message).collect();
        let checks: [(&str, &[String], &[String]); 4] = [
            ("expect.logs", &expect.logs, &logs),
            ("expect.chat", &expect.chat, &host.sim.chat),
            ("expect.hud", &expect.hud, &host.sim.hud),
            (
                "expect.command_feedback",
                &expect.command_feedback,
                &host.sim.command_feedback,
            ),
        ];
        for (what, wanted, seen) in checks {
            if let Some(m) = missing(wanted, seen) {
                return Err(ScriptFailure {
                    step: n,
                    what: what.to_string(),
                    expected: format!("a message containing {m:?}"),
                    actual: if seen.is_empty() {
                        "nothing".to_string()
                    } else {
                        format!("{seen:?}")
                    },
                });
            }
        }
        for id in &expect.stopped {
            let state = host.state(id);
            if !matches!(state, Some(ModState::Failed(_))) {
                return Err(ScriptFailure {
                    step: n,
                    what: "expect.stopped".to_string(),
                    expected: format!("mod {id} stopped"),
                    actual: match state {
                        Some(s) => format!("mod {id} is {s:?}"),
                        None => format!("mod {id} is not loaded"),
                    },
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_key() {
        let s = parse(
            r#"
name = "demo"
[[step]]
ticks = 20
frames = 2
command = "fish hello"
chat = "hi"
key = { name = "H", pressed = true }
[step.expect]
logs = ["a"]
chat = ["b"]
hud = ["c"]
command_feedback = ["d"]
stopped = ["e"]
[[step]]
"#,
        )
        .unwrap();
        assert_eq!(s.name.as_deref(), Some("demo"));
        assert_eq!(s.steps.len(), 2);
        assert_eq!(s.steps[0].ticks, Some(20));
        assert_eq!(
            s.steps[0].key,
            Some(KeyStep {
                name: "H".into(),
                pressed: true
            })
        );
        assert_eq!(s.steps[0].expect.as_ref().unwrap().stopped, ["e"]);
        assert_eq!(s.steps[1], Step::default());
    }

    #[test]
    fn unknown_keys_are_errors() {
        assert!(
            parse("[[step]]\nticks = 1\nbogus = 2")
                .unwrap_err()
                .contains("unknown field")
        );
        assert!(parse("surprise = 1").is_err());
        assert!(parse("[[step]]\n[step.expect]\nnope = []").is_err());
        assert!(parse("[[step]]\nkey = { name = \"H\" }").is_err());
        assert!(parse("[[step]]\nticks = \"x\"").is_err());
    }

    #[test]
    fn failure_display_names_step_and_values() {
        let f = ScriptFailure {
            step: 3,
            what: "expect.logs".into(),
            expected: "x".into(),
            actual: "y".into(),
        };
        assert_eq!(
            f.to_string(),
            "step 3: expect.logs failed\n  expected: x\n  actual:   y"
        );
    }
}
