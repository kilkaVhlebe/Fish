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
//! Unknown keys are errors. Within a step the actions run in a fixed order: `world`, `gamerule`, `key`,
//! `command`, `chat`, the game events, `ticks`, `server_ticks`, `frames`; then `expect` is checked.
//! Later phases add keys.
//!
//! ```toml
//! [[step]]
//! gamerule = { name = "mymod:limit", value = 4 }   # /gamerule in the current world
//! world = "other"                                  # saves this world's rules, opens another
//! [step.expect]
//! gamerules = { "mymod:limit" = 8 }                # values in the current world
//! ```
//!
//! Fish blocks live in a block world kept by the host; every block step works on it:
//!
//! ```toml
//! [[step]]
//! place_block = { pos = [1, 64, 1], block = "mymod:ruby" }   # item and actor are optional
//! use_block = { pos = [1, 64, 1], face = "up", hand = "main", item = "" }
//! break_block = { pos = [1, 64, 1], block = "mymod:ruby" }
//! random_tick = { pos = [1, 64, 1] }
//! scheduled_tick = { pos = [1, 64, 1] }
//! [step.expect]
//! blocks = { "1 64 1" = "air" }                      # world content
//! block_states = { "mymod:ruby" = [35723, 1] }       # first state, count
//! use_result = "success"                             # success, consume, fail or pass
//! content_files = ["data/mymod/horizon/block/ruby.json"]
//! ```
//!
//! Fish items are used, eaten, ticked and listed in the creative inventory the loader builds from a
//! small vanilla fixture (tabs `building_blocks`, `ingredients`, `search`, `hotbar`, `op_blocks`):
//!
//! ```toml
//! [[step]]
//! use_item = { item = "mymod:ruby", count = 1, hand = "main" }    # count and hand are optional
//! use_entity = { id = 7, entity = "minecraft:cow", item = "mymod:ruby", hand = "main" }
//! finish_using = { item = "mymod:snack", count = 1, hand = "main" }
//! inventory_tick = { item = "mymod:ruby", count = 1, slot = 0, selected = false }
//! [step.expect]
//! items = ["mymod:ruby"]                                           # registered items
//! tooltips = { "mymod:ruby" = ["§7A gem"] }                        # exact lines for a stack of one
//! creative_tabs = ["mymod:tab", "minecraft:ingredients"]           # tabs of the creative inventory
//! creative_entries = { "mymod:tab" = ["mymod:ruby"] }              # contained, in this order
//! no_creative_entries = { "minecraft:ingredients" = ["mymod:x"] }  # must not be listed
//! use_result = "success"                                           # of the last use_* step
//! ```

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

use crate::TestHost;
use crate::blocks;
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
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Fish game rules and the values they must have in the current world (`name = value`).
    #[serde(default)]
    pub gamerules: BTreeMap<String, toml::Value>,
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
    /// Whether the step's last handled server command must have succeeded.
    pub command_ok: Option<bool>,
    /// Substrings that must appear in the event log (`group/name outcome`).
    #[serde(default)]
    pub events: Vec<String>,
    /// Substrings that must NOT appear in the event log.
    #[serde(default)]
    pub no_events: Vec<String>,
    /// Blocks the simulated world must hold: `"x y z" = "namespace:path"` (`"air"` for none).
    #[serde(default)]
    pub blocks: BTreeMap<String, String>,
    /// The states of Fish blocks: `"namespace:path" = [first, count]`.
    #[serde(default)]
    pub block_states: BTreeMap<String, [u32; 2]>,
    /// The result of the step's last `use_block`: `success` or `consume` (a Fish block handled the
    /// click), `fail` (it cancelled it) or `pass`.
    pub use_result: Option<String>,
    /// Files (relative to a mod's content folder) that must exist in a folder the loader wrote.
    #[serde(default)]
    pub content_files: Vec<String>,
    /// Files that must not exist in any content folder.
    #[serde(default)]
    pub no_content_files: Vec<String>,
    /// Items that must be registered (`namespace:path`).
    #[serde(default)]
    pub items: Vec<String>,
    /// The tooltip lines an item adds for a stack of one, exactly: `"mod:item" = ["line", ...]`.
    #[serde(default)]
    pub tooltips: BTreeMap<String, Vec<String>>,
    /// Tabs that must exist in the creative inventory, Fish's and vanilla's.
    #[serde(default)]
    pub creative_tabs: Vec<String>,
    /// Items a creative tab must list, in this relative order: `"tab:id" = ["item", ...]`.
    #[serde(default)]
    pub creative_entries: BTreeMap<String, Vec<String>>,
    /// Items a creative tab must not list.
    #[serde(default)]
    pub no_creative_entries: BTreeMap<String, Vec<String>>,
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

/// The player uses an item on a block: `pos`, the face and hand, the item and (when the simulated
/// world holds no block there) the block.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UseStep {
    /// Block position.
    pub pos: [i32; 3],
    /// Block id when the world holds none there.
    #[serde(default = "stone")]
    pub block: String,
    /// Held item id.
    #[serde(default)]
    pub item: String,
    /// The face clicked: `down`, `up` (default), `north`, `south`, `west` or `east`.
    pub face: Option<String>,
    /// The hand: `main` (default) or `off`.
    pub hand: Option<String>,
}

/// The player uses an item in the air.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UseItemStep {
    /// The item's id.
    pub item: String,
    /// The size of the stack (default 1).
    #[serde(default = "one_u32")]
    pub count: u32,
    /// The hand: `main` (default) or `off`.
    pub hand: Option<String>,
}

/// The player uses an item on an entity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UseEntityStep {
    /// The entity's network id (default 1).
    #[serde(default = "one")]
    pub id: i64,
    /// The entity's type (default `minecraft:zombie`).
    #[serde(default = "zombie")]
    pub entity: String,
    /// The held item (empty: nothing).
    #[serde(default)]
    pub item: String,
    /// The hand: `main` (default) or `off`.
    pub hand: Option<String>,
}

/// The player finished eating or drinking an item.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinishUsingStep {
    /// The item's id.
    pub item: String,
    /// The size of the stack (default 1).
    #[serde(default = "one_u32")]
    pub count: u32,
    /// The hand: `main` (default) or `off`.
    pub hand: Option<String>,
}

/// An item sitting in the player's inventory for one tick.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryTickStep {
    /// The item's id.
    pub item: String,
    /// The size of the stack (default 1).
    #[serde(default = "one_u32")]
    pub count: u32,
    /// The inventory slot: 0..=35 the inventory, then armor and off hand (default 0).
    #[serde(default)]
    pub slot: i32,
    /// Whether it is the selected hotbar slot (default false).
    #[serde(default)]
    pub selected: bool,
}

fn one_u32() -> u32 {
    1
}

/// The hand number of a step's `hand` key; a name that is no hand is a mistake in the script.
fn hand_of(step: &str, name: Option<&str>) -> u8 {
    name.map_or(Some(0), blocks::hand_number)
        .unwrap_or_else(|| panic!("{step}: unknown hand {name:?}"))
}

/// A block the player places in the simulated world.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceStep {
    /// Block position.
    pub pos: [i32; 3],
    /// The block id.
    pub block: String,
    /// The held item; defaults to the block.
    pub item: Option<String>,
    /// The acting player; defaults to Steve.
    pub actor: Option<String>,
}

/// A position in the simulated world.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickStep {
    /// Block position.
    pub pos: [i32; 3],
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

/// A command typed on the server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerCommand {
    /// The line, without a slash.
    pub line: String,
    /// The player's permission level (default 4).
    #[serde(default = "owner")]
    pub level: u8,
    /// Who types it.
    #[serde(default = "steve")]
    pub actor: String,
}

fn owner() -> u8 {
    4
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

/// `/gamerule <name> <value>` typed for a Fish rule in the current world.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameRuleStep {
    /// The rule, `namespace:path`.
    pub name: String,
    /// The value: a boolean, number or string; absent queries the rule.
    pub value: Option<toml::Value>,
}

/// One step of a script.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Client ticks to advance.
    pub ticks: Option<u32>,
    /// Ticks of the integrated server to advance (a second game next to the window).
    pub server_ticks: Option<u32>,
    /// Frames to render.
    pub frames: Option<u32>,
    /// A client command, without a leading `/`.
    pub command: Option<String>,
    /// A chat message the player sends.
    pub chat: Option<String>,
    /// Opens the world of that name (a folder in `saves/`): the open world's rules are saved and the
    /// new one's read from its folder.
    pub world: Option<String>,
    /// `/gamerule` for a Fish rule in the current world.
    pub gamerule: Option<GameRuleStep>,
    /// A key event.
    pub key: Option<KeyStep>,
    /// A player joins (name).
    pub join: Option<String>,
    /// A player leaves (name).
    pub leave: Option<String>,
    /// The player uses an item on a block.
    pub use_block: Option<UseStep>,
    /// The player uses an item in the air.
    pub use_item: Option<UseItemStep>,
    /// The player uses an item on an entity.
    pub use_entity: Option<UseEntityStep>,
    /// The player finished eating or drinking an item.
    pub finish_using: Option<FinishUsingStep>,
    /// One tick of an item in the player's inventory.
    pub inventory_tick: Option<InventoryTickStep>,
    /// The player attacks a block.
    pub attack_block: Option<BlockStep>,
    /// The player attacks an entity.
    pub attack_entity: Option<EntityStep>,
    /// The player breaks a block.
    pub break_block: Option<BlockStep>,
    /// The player places a block in the simulated world.
    pub place_block: Option<PlaceStep>,
    /// A random tick picks the block at a position.
    pub random_tick: Option<TickStep>,
    /// A scheduled tick of the block at a position runs.
    pub scheduled_tick: Option<TickStep>,
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
    /// A command typed on the server (Fish command trees).
    pub server_command: Option<ServerCommand>,
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
    if let Some(b) = &step.place_block {
        let item = b.item.as_deref().unwrap_or(&b.block);
        let actor = b.actor.as_deref().unwrap_or(crate::events::PLAYER);
        host.place_block(b.pos, &b.block, item, actor);
    }
    if let Some(b) = &step.use_block {
        let item = if b.item.is_empty() {
            "minecraft:air"
        } else {
            &b.item
        };
        let face = b.face.as_deref().map_or(Some(1), blocks::face_number);
        let hand = b.hand.as_deref().map_or(Some(0), blocks::hand_number);
        let (Some(face), Some(hand)) = (face, hand) else {
            panic!("use_block: unknown face {:?} or hand {:?}", b.face, b.hand);
        };
        host.use_block_at(b.pos, face, hand, &b.block, item);
    }
    if let Some(i) = &step.use_item {
        host.use_item(&i.item, i.count, hand_of("use_item", i.hand.as_deref()));
    }
    if let Some(e) = &step.use_entity {
        let hand = hand_of("use_entity", e.hand.as_deref());
        let item = if e.item.is_empty() {
            "minecraft:air"
        } else {
            &e.item
        };
        host.use_item_on_entity(e.id, &e.entity, item, hand);
    }
    if let Some(i) = &step.finish_using {
        host.finish_using(&i.item, i.count, hand_of("finish_using", i.hand.as_deref()));
    }
    if let Some(i) = &step.inventory_tick {
        host.inventory_tick(&i.item, i.count, i.slot, i.selected);
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
    if let Some(t) = &step.random_tick {
        host.random_tick(t.pos);
    }
    if let Some(t) = &step.scheduled_tick {
        host.scheduled_tick(t.pos);
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
    if let Some(c) = &step.server_command {
        host.server_command(&c.actor, c.level, &c.line);
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

/// A TOML scalar as the text a `/gamerule` argument would have.
fn scalar_text(v: &toml::Value) -> Option<String> {
    match v {
        toml::Value::Boolean(b) => Some(b.to_string()),
        toml::Value::Integer(i) => Some(i.to_string()),
        toml::Value::Float(f) => Some(f.to_string()),
        toml::Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Checks `expect.gamerules` for one rule.
fn check_gamerule(
    host: &TestHost,
    step: usize,
    name: &str,
    want: &toml::Value,
) -> Option<ScriptFailure> {
    let failure = |expected: String, actual: String| ScriptFailure {
        step,
        what: "expect.gamerules".to_string(),
        expected,
        actual,
    };
    let Some(def) = host.game_rule_defs().into_iter().find(|d| d.name == name) else {
        return Some(failure(
            format!("a game rule named {name}"),
            "no mod registered it".to_string(),
        ));
    };
    let Some(text) = scalar_text(want) else {
        return Some(failure(
            format!("{name} to be a boolean, number or string"),
            format!("{want:?}"),
        ));
    };
    let wanted = match def.parse(&text) {
        Ok(v) => v,
        Err(why) => return Some(failure(format!("{name} = {text}"), why)),
    };
    let actual = host.game_rule(name);
    (actual.as_ref() != Some(&wanted)).then(|| {
        failure(
            format!("{name} = {wanted}"),
            actual.map_or_else(|| "unknown".to_string(), |v| format!("{name} = {v}")),
        )
    })
}

/// Runs `script` against a booted host; returns the first failure.
pub fn run(host: &mut TestHost, script: &Script) -> Result<(), ScriptFailure> {
    for (i, step) in script.steps.iter().enumerate() {
        let n = i + 1;
        if let Some(w) = &step.world {
            host.switch_world(w);
        }
        if let Some(g) = &step.gamerule {
            let text = match g.value.as_ref().map(scalar_text) {
                None => None,
                Some(Some(t)) => Some(t),
                Some(None) => {
                    return Err(ScriptFailure {
                        step: n,
                        what: "gamerule".to_string(),
                        expected: "a boolean, number or string value".to_string(),
                        actual: format!("{:?}", g.value),
                    });
                }
            };
            if host.gamerule(&g.name, text.as_deref()).is_none() {
                return Err(ScriptFailure {
                    step: n,
                    what: "gamerule".to_string(),
                    expected: format!("an open world and a Fish rule named {}", g.name),
                    actual: format!(
                        "world {:?}, rules {:?}",
                        host.sim.world,
                        host.game_rule_defs()
                            .into_iter()
                            .map(|d| d.name)
                            .collect::<Vec<_>>()
                    ),
                });
            }
        }
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
        if let Some(t) = step.server_ticks {
            host.tick_integrated_server(t);
        }
        if let Some(f) = step.frames {
            host.frame(f);
        }
        host.drain_trace();
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
        if let Some(want) = expect.command_ok
            && host.sim.command_ok != Some(want)
        {
            return Err(ScriptFailure {
                step: n,
                what: "expect.command_ok".to_string(),
                expected: want.to_string(),
                actual: format!("{:?}", host.sim.command_ok),
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
        if let Some(failure) = host.check_blocks(n, expect) {
            return Err(failure);
        }
        if let Some(failure) = host.check_items(n, expect) {
            return Err(failure);
        }
        for (name, want) in &expect.gamerules {
            if let Some(failure) = check_gamerule(host, n, name, want) {
                return Err(failure);
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

#[cfg(test)]
mod gamerule_keys {
    use super::*;

    #[test]
    fn world_and_gamerule_keys_parse_scalars_of_every_type() {
        let s = parse(
            r#"
[[step]]
world = "other"
gamerule = { name = "m:r", value = 2.5 }
[step.expect]
gamerules = { "m:a" = true, "m:b" = 3, "m:c" = 1.5, "m:d" = "hard" }
"#,
        )
        .unwrap();
        let step = &s.steps[0];
        assert_eq!(step.world.as_deref(), Some("other"));
        assert_eq!(step.gamerule.as_ref().unwrap().name, "m:r");
        let expect = step.expect.as_ref().unwrap();
        let texts: Vec<Option<String>> = expect.gamerules.values().map(scalar_text).collect();
        assert_eq!(
            texts,
            [
                Some("true".to_string()),
                Some("3".to_string()),
                Some("1.5".to_string()),
                Some("hard".to_string())
            ]
        );
        assert_eq!(scalar_text(&toml::Value::Array(vec![])), None);
        assert!(parse("[[step]]\ngamerule = { value = 1 }").is_err());
        assert!(parse("[[step]]\ngamerule = { name = \"a:b\", bogus = 1 }").is_err());
    }
}
