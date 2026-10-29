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
    /// The outcome of the step's last game event: `pass`, `handled`, `cancel` or `replace:<text>`.
    pub outcome: Option<String>,
    /// Substrings that must appear in the chat the server broadcast.
    #[serde(default)]
    pub broadcast: Vec<String>,
    /// Substrings that must appear in the chat lines the player sent.
    #[serde(default)]
    pub sent_chat: Vec<String>,
    /// Substrings that must appear in the chat lines the client showed.
    #[serde(default)]
    pub received: Vec<String>,
    /// Substrings that must appear in `x y z block` of broken blocks.
    #[serde(default)]
    pub broken: Vec<String>,
    /// Substrings that must appear in the event log (`group/name outcome`).
    #[serde(default)]
    pub events: Vec<String>,
    /// Substrings that must NOT appear in the event log.
    #[serde(default)]
    pub no_events: Vec<String>,
}

fn stone() -> String {
    "minecraft:stone".to_string()
}

fn steve() -> String {
    crate::events::PLAYER.to_string()
}

fn generic() -> String {
    "minecraft:generic".to_string()
}

fn zombie() -> String {
    "minecraft:zombie".to_string()
}

/// A block interaction: `pos`, the block there and the item in hand.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockStep {
    /// Block position.
    pub pos: [i32; 3],
    /// Block id.
    #[serde(default = "stone")]
    pub block: String,
    /// Held item id.
    #[serde(default)]
    pub item: String,
}

/// An entity target: network id and type.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityStep {
    /// Network id.
    #[serde(default = "one")]
    pub id: i64,
    /// Entity type.
    #[serde(rename = "type", default = "zombie")]
    pub kind: String,
    /// Damage amount (`damage` only).
    #[serde(default)]
    pub amount: f32,
    /// Damage kind (`damage` only).
    #[serde(default = "generic")]
    pub source: String,
}

fn one() -> i64 {
    1
}

/// A chat message reaching the server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatIn {
    /// The sender.
    #[serde(default = "steve")]
    pub actor: String,
    /// The message.
    pub text: String,
}

/// A chat line arriving at the client.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatReceived {
    /// The line.
    pub text: String,
    /// `player`, `server` or `client`.
    #[serde(default = "player")]
    pub source: String,
}

fn player() -> String {
    "player".to_string()
}

/// Any game event, field by field.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventStep {
    /// `group/name` or just `name` of the event, e.g. `interaction/use_block`.
    pub kind: String,
    /// Block position.
    #[serde(default)]
    pub pos: [i32; 3],
    /// Face index.
    pub face: Option<u8>,
    /// Hand: 0 main, 1 off.
    #[serde(default)]
    pub hand: u8,
    /// Entity id.
    #[serde(default)]
    pub entity_id: i64,
    /// Damage amount.
    #[serde(default)]
    pub amount: f32,
    /// Acting player.
    pub actor: Option<String>,
    /// Dimension.
    pub dimension: Option<String>,
    /// Subject.
    #[serde(default)]
    pub subject: String,
    /// Item.
    #[serde(default)]
    pub item: String,
    /// Extra.
    #[serde(default)]
    pub extra: String,
    /// Client-side prediction.
    #[serde(default)]
    pub is_client: bool,
}

/// One step of a script.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
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
    /// A player joins (name).
    pub join: Option<String>,
    /// A player leaves (name).
    pub leave: Option<String>,
    /// The player uses an item on a block.
    pub use_block: Option<BlockStep>,
    /// The player attacks a block.
    pub attack_block: Option<BlockStep>,
    /// The player attacks an entity.
    pub attack_entity: Option<EntityStep>,
    /// The player breaks a block.
    pub break_block: Option<BlockStep>,
    /// A living entity takes damage.
    pub damage: Option<EntityStep>,
    /// A living entity dies.
    pub death: Option<EntityStep>,
    /// A chat message reaches the server.
    pub chat_in: Option<ChatIn>,
    /// The local player types a chat line.
    pub chat_out: Option<String>,
    /// A chat line arrives at the client.
    pub chat_received: Option<ChatReceived>,
    /// `/reload` runs.
    pub reload: Option<bool>,
    /// The game starts (`true`) or stops (`false`).
    pub game: Option<bool>,
    /// Any game event.
    pub event: Option<EventStep>,
    /// Checks after the actions.
    pub expect: Option<Expect>,
}

/// A parsed script.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
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

fn kind_named(name: &str) -> Option<fish_hooks::Kind> {
    let short = name.rsplit('/').next().unwrap_or(name);
    fish_hooks::Kind::ALL.into_iter().find(|k| {
        k.name() == name || k.name().rsplit('/').next() == Some(short) && !name.contains('/')
    })
}

/// Performs the game-event actions of a step, in a fixed order.
fn game_actions(host: &mut TestHost, step: &Step) {
    if let Some(start) = step.game {
        if start {
            host.start_game();
        } else {
            host.stop_game();
        }
    }
    if let Some(n) = &step.join {
        host.join(n);
    }
    if let Some(n) = &step.leave {
        host.leave(n);
    }
    if let Some(b) = &step.use_block {
        if b.item.is_empty() {
            host.use_block(b.pos, &b.block, "minecraft:air");
        } else {
            host.use_block(b.pos, &b.block, &b.item);
        }
    }
    if let Some(b) = &step.attack_block {
        host.attack_block(b.pos, &b.block);
    }
    if let Some(e) = &step.attack_entity {
        host.attack_entity(e.id, &e.kind);
    }
    if let Some(b) = &step.break_block {
        host.break_block(b.pos, &b.block);
    }
    if let Some(e) = &step.damage {
        host.damage(e.id, &e.kind, e.amount, &e.source);
    }
    if let Some(e) = &step.death {
        host.death(e.id, &e.kind);
    }
    if let Some(c) = &step.chat_in {
        host.chat_in(&c.actor, &c.text);
    }
    if let Some(c) = &step.chat_out {
        host.chat_out(c);
    }
    if let Some(c) = &step.chat_received {
        host.chat_received(&c.text, &c.source);
    }
    if step.reload == Some(true) {
        host.reload();
    }
    if let Some(e) = &step.event {
        let kind = kind_named(&e.kind).unwrap_or_else(|| panic!("unknown event kind {:?}", e.kind));
        let mut g = fish_hooks::GameEvent::new(kind);
        g.pos = e.pos;
        g.face = e.face.unwrap_or(255);
        g.hand = e.hand;
        g.entity_id = e.entity_id;
        g.amount = e.amount;
        g.actor = e.actor.as_deref().unwrap_or(crate::events::PLAYER);
        g.dimension = e.dimension.as_deref().unwrap_or(crate::events::OVERWORLD);
        g.subject = &e.subject;
        g.item = &e.item;
        g.extra = &e.extra;
        g.is_client = e.is_client;
        host.game_event(&g);
    }
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
        game_actions(host, step);
        if let Some(t) = step.ticks {
            host.tick(t);
        }
        if let Some(f) = step.frames {
            host.frame(f);
        }
        let Some(expect) = &step.expect else { continue };
        let logs: Vec<String> = host.logs().into_iter().map(|r| r.message).collect();
        let checks: [(&str, &[String], &[String]); 8] = [
            ("expect.logs", &expect.logs, &logs),
            ("expect.chat", &expect.chat, &host.sim.chat),
            ("expect.hud", &expect.hud, &host.sim.hud),
            (
                "expect.command_feedback",
                &expect.command_feedback,
                &host.sim.command_feedback,
            ),
            ("expect.broadcast", &expect.broadcast, &host.sim.broadcast),
            ("expect.sent_chat", &expect.sent_chat, &host.sim.sent_chat),
            ("expect.received", &expect.received, &host.sim.received),
            ("expect.broken", &expect.broken, &host.sim.broken),
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
        if let Some(m) = missing(&expect.events, &host.sim.events) {
            return Err(ScriptFailure {
                step: n,
                what: "expect.events".to_string(),
                expected: format!("an event log line containing {m:?}"),
                actual: format!("{:?}", host.sim.events),
            });
        }
        if let Some(bad) = expect
            .no_events
            .iter()
            .find(|n| host.sim.events.iter().any(|h| h.contains(n.as_str())))
        {
            return Err(ScriptFailure {
                step: n,
                what: "expect.no_events".to_string(),
                expected: format!("no event log line containing {bad:?}"),
                actual: format!("{:?}", host.sim.events),
            });
        }
        if let Some(want) = &expect.outcome
            && &host.sim.last_outcome != want
        {
            return Err(ScriptFailure {
                step: n,
                what: "expect.outcome".to_string(),
                expected: want.clone(),
                actual: host.sim.last_outcome.clone(),
            });
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
