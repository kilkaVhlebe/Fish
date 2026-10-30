//! The interface between Horizon (hooked at build time) and the Fish loader.
//!
//! Horizon's hook points (the edition's mixin bundle) call [`get`] and, when a [`Dispatch`] is
//! installed, forward to it, passing views of the game ([`ClientView`], [`HudView`]) that the
//! bridge implements over Horizon's own `Game`. The loader implements [`Dispatch`] and installs
//! it with [`install`] after boot.
//!
//! Every [`Dispatch`] method has an empty default body, so later phases only add methods. This
//! crate has no dependencies and forbids unsafe code; it is compiled into the edition.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::path::PathBuf;
use std::sync::OnceLock;

/// Which kind of `Game` a hook runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The window's own game (single player, LAN and remote servers).
    Window,
    /// The integrated server thread of a window.
    IntegratedServer,
    /// A dedicated server.
    DedicatedServer,
}

impl Role {
    /// True for [`Role::IntegratedServer`] and [`Role::DedicatedServer`].
    pub fn is_server(self) -> bool {
        !matches!(self, Role::Window)
    }
}

/// The local player as a hook reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSnapshot {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
    /// Yaw in degrees (0 faces south, 90 west).
    pub yaw: f32,
    /// Pitch in degrees.
    pub pitch: f32,
    /// Dimension id, e.g. `minecraft:overworld`.
    pub dimension: String,
}

/// The game, as a hook sees it.
pub trait ClientView {
    /// Which `Game` this is.
    fn role(&self) -> Role;
    /// The player, when in a world.
    fn player(&self) -> Option<PlayerSnapshot>;
    /// Shows a line in the player's chat (client only).
    fn show_chat(&mut self, text: &str);
    /// The game directory.
    fn game_dir(&self) -> PathBuf;
    /// The id of the level this game currently runs (`minecraft:overworld`).
    fn level_id(&self) -> String {
        "minecraft:overworld".to_string()
    }
    /// The world this game runs, for per-world data such as game rules: a stable id (the world's
    /// directory). `None` where there is no world of its own (the main menu, a remote server).
    fn world_id(&self) -> Option<String> {
        None
    }
}

/// Who typed a command.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandSource<'a> {
    /// The player's name (empty for the console).
    pub name: &'a str,
    /// The player's UUID bytes (zero when unknown).
    pub uuid: [u8; 16],
    /// Permission level 0..=4 (0 everyone, 2 game master, 4 owner).
    pub level: u8,
    /// True for a player, false for the server console or a command block.
    pub is_player: bool,
    /// Where the source stands, for relative coordinates (`~`).
    pub pos: Option<[f64; 3]>,
}

/// What running a command line through Fish produced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandOutcome {
    /// False when the line is not a Fish command: the game handles it.
    pub handled: bool,
    /// True when the command succeeded.
    pub ok: bool,
    /// The command's result value (what `/execute store` would read).
    pub result: i32,
    /// Lines to show the source: feedback on success, the error on failure.
    pub messages: Vec<String>,
}

/// A HUD draw target in GUI pixels.
pub trait HudView {
    /// Fills `rect` (`[x, y, width, height]`) with `argb`.
    fn fill(&mut self, rect: [f32; 4], argb: u32);
    /// Draws a line of text at `(x, y)` in opaque `rgb`.
    fn text(&mut self, text: &str, x: f32, y: f32, rgb: u32, shadow: bool);
    /// Width of `text` in GUI pixels.
    fn text_width(&self, text: &str) -> f32;
    /// The GUI size `[width, height]` in GUI pixels.
    fn gui_size(&self) -> [f32; 2];
}

/// A row of Options > Mods.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeModRow {
    /// Mod id (its settings folder and key lines).
    pub id: String,
    /// Title, e.g. `Hello 0.1.0 [Fish]`.
    pub title: String,
    /// Description.
    pub description: String,
    /// Keys: name, label, default key (`key.keyboard.z`).
    pub keys: Vec<[String; 3]>,
}

/// The type of a Fish game rule, with its bounds.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleKind {
    /// `true` or `false`.
    Bool,
    /// A whole number in `min..=max`.
    Int {
        /// Smallest value.
        min: i32,
        /// Largest value.
        max: i32,
    },
    /// A number in `min..=max`.
    Double {
        /// Smallest value.
        min: f64,
        /// Largest value.
        max: f64,
    },
    /// One of the named choices.
    Enum(Vec<String>),
}

/// The value of a Fish game rule.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleValue {
    /// A [`RuleKind::Bool`] value.
    Bool(bool),
    /// A [`RuleKind::Int`] value.
    Int(i32),
    /// A [`RuleKind::Double`] value.
    Double(f64),
    /// A [`RuleKind::Enum`] value: one of the choices.
    Text(String),
}

impl std::fmt::Display for RuleValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleValue::Bool(b) => write!(f, "{b}"),
            RuleValue::Int(i) => write!(f, "{i}"),
            RuleValue::Double(d) if d.fract() == 0.0 && d.abs() < 1.0e15 => write!(f, "{d:.1}"),
            RuleValue::Double(d) => write!(f, "{d}"),
            RuleValue::Text(t) => f.write_str(t),
        }
    }
}

/// A game rule a mod registered.
#[derive(Debug, Clone, PartialEq)]
pub struct GameRuleDef {
    /// The rule's id, `namespace:path`.
    pub name: String,
    /// The category it is listed under: `player`, `mobs`, `spawning`, `drops`, `updates`, `chat` or `misc`.
    pub category: String,
    /// Its type and bounds.
    pub kind: RuleKind,
    /// The value of a world that never set it.
    pub default: RuleValue,
}

impl GameRuleDef {
    /// Checks that `value` has the rule's type and lies within its bounds.
    pub fn check(&self, value: &RuleValue) -> Result<(), String> {
        let name = &self.name;
        match (&self.kind, value) {
            (RuleKind::Bool, RuleValue::Bool(_)) => Ok(()),
            (RuleKind::Int { min, max }, RuleValue::Int(v)) => {
                if (*min..=*max).contains(v) {
                    Ok(())
                } else {
                    Err(format!("{name} must be between {min} and {max}, not {v}"))
                }
            }
            (RuleKind::Double { min, max }, RuleValue::Double(v)) => {
                if v.is_finite() && (*min..=*max).contains(v) {
                    Ok(())
                } else {
                    Err(format!("{name} must be between {min} and {max}, not {v}"))
                }
            }
            (RuleKind::Enum(choices), RuleValue::Text(t)) => {
                if choices.iter().any(|c| c == t) {
                    Ok(())
                } else {
                    Err(format!(
                        "{name} must be one of {}, not {t}",
                        choices.join(", ")
                    ))
                }
            }
            (kind, _) => Err(format!(
                "{name} expects {}, got {}",
                kind.noun(),
                value.noun()
            )),
        }
    }

    /// Reads `text` as a value of the rule's type (what a command argument or a UI box holds).
    pub fn parse(&self, text: &str) -> Result<RuleValue, String> {
        let name = &self.name;
        let value = match &self.kind {
            RuleKind::Bool => match text {
                "true" => RuleValue::Bool(true),
                "false" => RuleValue::Bool(false),
                _ => return Err(format!("{name} takes true or false, not {text}")),
            },
            RuleKind::Int { .. } => RuleValue::Int(
                text.parse()
                    .map_err(|_| format!("{name} takes a whole number, not {text}"))?,
            ),
            RuleKind::Double { .. } => RuleValue::Double(
                text.parse()
                    .map_err(|_| format!("{name} takes a number, not {text}"))?,
            ),
            RuleKind::Enum(_) => RuleValue::Text(text.to_string()),
        };
        self.check(&value)?;
        Ok(value)
    }
}

impl RuleKind {
    fn noun(&self) -> &'static str {
        match self {
            RuleKind::Bool => "a boolean",
            RuleKind::Int { .. } => "an integer",
            RuleKind::Double { .. } => "a decimal",
            RuleKind::Enum(_) => "a choice",
        }
    }
}

impl RuleValue {
    fn noun(&self) -> &'static str {
        match self {
            RuleValue::Bool(_) => "a boolean",
            RuleValue::Int(_) => "an integer",
            RuleValue::Double(_) => "a decimal",
            RuleValue::Text(_) => "a choice",
        }
    }
}

/// The game events of phase P6, in the order of `fish_abi::game::EVENTS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The window's game ran its first frame.
    ClientStarted,
    /// The window is closing; the world is not saved yet.
    ClientStopping,
    /// The window closed and its world is saved. Detached: no game view.
    ClientStopped,
    /// A server (integrated, dedicated or in-frame) starts loading its world. `subject` = world name.
    ServerStarting,
    /// A server ran its first tick.
    ServerStarted,
    /// A server begins to stop; players are not saved yet.
    ServerStopping,
    /// A server finished saving and closed.
    ServerStopped,
    /// A level (dimension) became loaded. `dimension` = its id.
    WorldLoad,
    /// A level is about to be unloaded. `dimension` = its id.
    WorldUnload,
    /// A chunk entered the loaded set. `pos` = [chunk x, 0, chunk z]; detached.
    ChunkLoad,
    /// A chunk is about to leave the loaded set. `pos` = [chunk x, 0, chunk z]; detached.
    ChunkUnload,
    /// Data (datapacks/functions) reload starts.
    ReloadStart,
    /// Data reload finished (observed on the next server tick).
    ReloadEnd,
    /// A player uses a block with a hand. `pos`, `face`, `hand`, `subject` = block id, `item` = held item id. `Fail` cancels.
    UseBlock,
    /// A player uses the held item in the air. `hand`, `item`. `Fail` cancels.
    UseItem,
    /// A player uses an entity. `entity_id`, `subject` = entity type, `hand`, `item`. `Fail` cancels.
    UseEntity,
    /// A player starts attacking (mining) a block. `pos`, `face`, `subject` = block id. `Fail` cancels.
    AttackBlock,
    /// A player attacks an entity. `entity_id`, `subject` = entity type. `Fail` cancels.
    AttackEntity,
    /// A player is about to break a block. `pos`, `subject` = block id. `Fail` cancels and fires block_break_canceled.
    BlockBreakBefore,
    /// A player broke a block (observed on the next tick). `pos`, `subject` = the block that was there.
    BlockBreakAfter,
    /// A block break was cancelled by a listener. `pos`, `subject`.
    BlockBreakCanceled,
    /// A player picks an item (middle click). `item`. `Fail` cancels.
    PickItem,
    /// A living entity is about to take damage. `entity_id`, `subject` = entity type, `amount`, `extra` = damage kind. `Fail` cancels.
    AllowDamage,
    /// A living entity took damage. `entity_id`, `amount`, `extra`.
    AfterDamage,
    /// A living entity is about to die from damage. `Fail` cancels (approximated, see docs).
    AllowDeath,
    /// A living entity died. `entity_id`, `subject`, `pos`.
    AfterDeath,
    /// A player joined a server. `actor`, `actor_uuid`.
    PlayerJoin,
    /// A player left a server. `actor`, `actor_uuid`.
    PlayerLeave,
    /// A player respawned. `actor`.
    AfterRespawn,
    /// A player's data carried over a respawn. `actor`.
    CopyFrom,
    /// A player tries to sleep in a bed. `pos`, `actor`. `Fail` cancels.
    AllowSleeping,
    /// A player fell asleep. `pos`, `actor`.
    SleepStart,
    /// A player woke up. `actor`.
    SleepStop,
    /// A player's chat message reached a server. `actor`, `subject` = message. `Fail` drops it.
    AllowChat,
    /// A player's chat message was broadcast. `actor`, `subject`.
    Chat,
    /// A server sends a game (system) message. `subject` = text. `Fail` drops it.
    AllowGame,
    /// A server sent a game message. `subject`.
    Game,
    /// A server is about to run a player's command. `actor`, `subject` = command. `Fail` blocks it.
    AllowCommand,
    /// A server ran a player's command. `actor`, `subject`.
    Command,
    /// The client received a chat line. `subject` = plain text, `extra` = source (player/server/client). `Fail` hides it.
    AllowReceive,
    /// The client showed a received chat line. `subject`, `extra`.
    Receive,
    /// The player typed a chat line. `subject`. `Fail` cancels.
    AllowSend,
    /// The player's chat line may be rewritten with `replace_text`. `subject`.
    ModifySend,
    /// The player's chat line was sent. `subject`.
    Send,
    /// The client's connection starts configuring (before the world). `actor` = empty.
    NetClientInit,
    /// The client entered the play phase of a server connection.
    NetClientJoin,
    /// The client's server connection closed. `subject` = reason when known.
    NetClientDisconnect,
    /// A player's connection starts configuring on the server. `actor`, `actor_uuid`.
    NetServerInit,
    /// A player's connection entered the play phase on the server. `actor`, `actor_uuid`.
    NetServerJoin,
    /// A player's connection closed. `actor`, `actor_uuid`, `subject` = reason when known.
    NetServerDisconnect,
}

impl Kind {
    /// Every kind, in table order.
    pub const ALL: [Kind; 50] = [
        Kind::ClientStarted,
        Kind::ClientStopping,
        Kind::ClientStopped,
        Kind::ServerStarting,
        Kind::ServerStarted,
        Kind::ServerStopping,
        Kind::ServerStopped,
        Kind::WorldLoad,
        Kind::WorldUnload,
        Kind::ChunkLoad,
        Kind::ChunkUnload,
        Kind::ReloadStart,
        Kind::ReloadEnd,
        Kind::UseBlock,
        Kind::UseItem,
        Kind::UseEntity,
        Kind::AttackBlock,
        Kind::AttackEntity,
        Kind::BlockBreakBefore,
        Kind::BlockBreakAfter,
        Kind::BlockBreakCanceled,
        Kind::PickItem,
        Kind::AllowDamage,
        Kind::AfterDamage,
        Kind::AllowDeath,
        Kind::AfterDeath,
        Kind::PlayerJoin,
        Kind::PlayerLeave,
        Kind::AfterRespawn,
        Kind::CopyFrom,
        Kind::AllowSleeping,
        Kind::SleepStart,
        Kind::SleepStop,
        Kind::AllowChat,
        Kind::Chat,
        Kind::AllowGame,
        Kind::Game,
        Kind::AllowCommand,
        Kind::Command,
        Kind::AllowReceive,
        Kind::Receive,
        Kind::AllowSend,
        Kind::ModifySend,
        Kind::Send,
        Kind::NetClientInit,
        Kind::NetClientJoin,
        Kind::NetClientDisconnect,
        Kind::NetServerInit,
        Kind::NetServerJoin,
        Kind::NetServerDisconnect,
    ];

    /// `group/name` as it appears after `fish:` in the event id.
    pub fn name(self) -> &'static str {
        match self {
            Kind::ClientStarted => "lifecycle/client_started",
            Kind::ClientStopping => "lifecycle/client_stopping",
            Kind::ClientStopped => "lifecycle/client_stopped",
            Kind::ServerStarting => "lifecycle/server_starting",
            Kind::ServerStarted => "lifecycle/server_started",
            Kind::ServerStopping => "lifecycle/server_stopping",
            Kind::ServerStopped => "lifecycle/server_stopped",
            Kind::WorldLoad => "lifecycle/world_load",
            Kind::WorldUnload => "lifecycle/world_unload",
            Kind::ChunkLoad => "lifecycle/chunk_load",
            Kind::ChunkUnload => "lifecycle/chunk_unload",
            Kind::ReloadStart => "lifecycle/reload_start",
            Kind::ReloadEnd => "lifecycle/reload_end",
            Kind::UseBlock => "interaction/use_block",
            Kind::UseItem => "interaction/use_item",
            Kind::UseEntity => "interaction/use_entity",
            Kind::AttackBlock => "interaction/attack_block",
            Kind::AttackEntity => "interaction/attack_entity",
            Kind::BlockBreakBefore => "interaction/block_break_before",
            Kind::BlockBreakAfter => "interaction/block_break_after",
            Kind::BlockBreakCanceled => "interaction/block_break_canceled",
            Kind::PickItem => "interaction/pick_item",
            Kind::AllowDamage => "entity/allow_damage",
            Kind::AfterDamage => "entity/after_damage",
            Kind::AllowDeath => "entity/allow_death",
            Kind::AfterDeath => "entity/after_death",
            Kind::PlayerJoin => "entity/player_join",
            Kind::PlayerLeave => "entity/player_leave",
            Kind::AfterRespawn => "entity/after_respawn",
            Kind::CopyFrom => "entity/copy_from",
            Kind::AllowSleeping => "entity/allow_sleeping",
            Kind::SleepStart => "entity/sleep_start",
            Kind::SleepStop => "entity/sleep_stop",
            Kind::AllowChat => "message/allow_chat",
            Kind::Chat => "message/chat",
            Kind::AllowGame => "message/allow_game",
            Kind::Game => "message/game",
            Kind::AllowCommand => "message/allow_command",
            Kind::Command => "message/command",
            Kind::AllowReceive => "message/allow_receive",
            Kind::Receive => "message/receive",
            Kind::AllowSend => "message/allow_send",
            Kind::ModifySend => "message/modify_send",
            Kind::Send => "message/send",
            Kind::NetClientInit => "networking/client_init",
            Kind::NetClientJoin => "networking/client_join",
            Kind::NetClientDisconnect => "networking/client_disconnect",
            Kind::NetServerInit => "networking/server_init",
            Kind::NetServerJoin => "networking/server_join",
            Kind::NetServerDisconnect => "networking/server_disconnect",
        }
    }
}

/// What happened, as data. Fields a kind does not use are zero or empty.
#[derive(Debug, Clone, PartialEq)]
pub struct GameEvent<'a> {
    /// The event.
    pub kind: Kind,
    /// True for client-side predictions of an interaction.
    pub is_client: bool,
    /// 0 main hand, 1 off hand.
    pub hand: u8,
    /// Face 0..=5 (down, up, north, south, west, east) or 255.
    pub face: u8,
    /// Block position, or chunk x/z in `[0]`/`[2]`.
    pub pos: [i32; 3],
    /// Entity network id.
    pub entity_id: i64,
    /// Damage amount.
    pub amount: f32,
    /// Acting player's UUID bytes.
    pub actor_uuid: [u8; 16],
    /// Acting player's name.
    pub actor: &'a str,
    /// Dimension id.
    pub dimension: &'a str,
    /// Block, entity type, command or message.
    pub subject: &'a str,
    /// Item id.
    pub item: &'a str,
    /// Damage kind, chat source, ...
    pub extra: &'a str,
    /// Which role raised it, when there is no game view.
    pub role: Role,
    /// True when listeners may rewrite `subject` (see [`Outcome::Replace`]).
    pub rewritable: bool,
}

impl GameEvent<'_> {
    /// An event with nothing filled in.
    pub fn new(kind: Kind) -> GameEvent<'static> {
        GameEvent {
            kind,
            is_client: false,
            hand: 0,
            face: 255,
            pos: [0; 3],
            entity_id: 0,
            amount: 0.0,
            actor_uuid: [0; 16],
            actor: "",
            dimension: "",
            subject: "",
            item: "",
            extra: "",
            role: Role::Window,
            rewritable: false,
        }
    }
}

/// What the listeners of an event decided.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Outcome {
    /// Nobody objected: carry on.
    #[default]
    Pass,
    /// A listener handled the event (for allow events: explicitly allowed).
    Handled,
    /// A listener cancelled the event.
    Cancel,
    /// Listeners rewrote the text; carry on with this text.
    Replace(String),
}

impl Outcome {
    /// True for [`Outcome::Cancel`].
    pub fn cancelled(&self) -> bool {
        matches!(self, Outcome::Cancel)
    }
}

/// Which phase of a connection a custom payload travels in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetPhase {
    /// The configuration phase (before the world loads).
    Configuration,
    /// The play phase.
    Play,
}

/// A custom payload (`CustomPayload { channel, data }`) that arrived at this game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetPacket<'a> {
    /// The phase it arrived in.
    pub phase: NetPhase,
    /// The channel, `namespace:path`.
    pub channel: &'a str,
    /// The payload bytes.
    pub data: &'a [u8],
    /// On a server: the sending player's name (and UUID bytes). `None` on a client.
    pub from: Option<(&'a str, [u8; 16])>,
}

/// How Fish puts payloads on the wire. The edition's bridge implements it over Horizon's
/// connections; tests implement it in memory. Methods must not block.
pub trait NetOut: Send + Sync {
    /// Client side: sends a payload to the server this client is connected to.
    fn to_server(&self, phase: NetPhase, channel: &str, data: &[u8]) -> Result<(), String>;
    /// Server side: sends a payload to the named player, or to every player when `player` is `None`.
    fn to_player(
        &self,
        phase: NetPhase,
        player: Option<&str>,
        channel: &str,
        data: &[u8],
    ) -> Result<(), String>;
    /// Server side: kicks `player` with `reason`. Client side (`player` `None`): leaves the server.
    fn disconnect(&self, player: Option<&str>, reason: &str);
    /// Server side: names of the players currently connected in `phase`.
    fn players(&self, phase: NetPhase) -> Vec<String>;
}

static NET_OUT: OnceLock<&'static dyn NetOut> = OnceLock::new();

/// Installs the process-wide wire implementation; returns false if one is already installed.
pub fn install_net(out: &'static dyn NetOut) -> bool {
    NET_OUT.set(out).is_ok()
}

/// The installed wire implementation, if any.
pub fn net_out() -> Option<&'static dyn NetOut> {
    NET_OUT.get().copied()
}

/// What Horizon's hook points call. Every method defaults to doing nothing.
pub trait Dispatch: Send + Sync {
    /// Start of a game tick, in every `Game`.
    fn tick_start(&self, _game: &mut dyn ClientView) {}
    /// End of a game tick, after Horizon's own mods ticked, in every `Game`.
    fn tick_end(&self, _game: &mut dyn ClientView) {}
    /// Once a frame.
    fn frame(&self, _game: &mut dyn ClientView, _elapsed_secs: f32) {}
    /// The HUD pass.
    fn hud(&self, _game: &mut dyn ClientView, _hud: &mut dyn HudView) {}
    /// A client command (no slash). Returns true when a mod ran it.
    fn client_command(&self, _game: &mut dyn ClientView, _command: &str) -> bool {
        false
    }
    /// A key or mouse button event by vanilla name (`key.keyboard.z`).
    fn key(&self, _game: &mut dyn ClientView, _key: &str, _pressed: bool) {}
    /// A game event. `game` is `None` for events raised where no `Game` is at hand.
    fn game_event(&self, _game: Option<&mut dyn ClientView>, _event: &GameEvent<'_>) -> Outcome {
        Outcome::Pass
    }
    /// False when no listener is registered for `kind`: hot hooks use it to skip the work.
    fn wants(&self, _kind: Kind) -> bool {
        true
    }
    /// A command line (no leading slash) typed by `source`: runs it when it is a Fish command.
    fn run_command(
        &self,
        _game: &mut dyn ClientView,
        _source: &CommandSource<'_>,
        _line: &str,
    ) -> CommandOutcome {
        CommandOutcome::default()
    }
    /// Completions for the word being typed in `line` (static suggestion lists and literals).
    fn suggest_command(&self, _source: &CommandSource<'_>, _line: &str) -> Vec<String> {
        Vec::new()
    }
    /// The root names of every command Fish registered (for conflict checks against vanilla).
    fn command_roots(&self) -> Vec<String> {
        Vec::new()
    }
    /// The vanilla command `root` shadows Fish's command of the same name.
    fn shadowed_command(&self, _root: &str) {}
    /// A custom payload arrived. Returns true when a registered receiver consumed it; the game
    /// then ignores the packet (an unclaimed channel stays with the game).
    fn net_receive(&self, _game: &mut dyn ClientView, _packet: &NetPacket<'_>) -> bool {
        false
    }
    /// Lines appended to the F3 screen.
    fn debug_lines(&self, _out: &mut Vec<String>) {}
    /// Lines for the crash report's details (`key`, `value`): the Fish version, the mods, the failed mods.
    fn crash_details(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
    /// Options > Mods rows for Fish mods.
    fn list_mods(&self, _rows: &mut Vec<NativeModRow>) {}
    /// The settings buttons of mod `mod_id`'s page.
    fn settings_buttons(&self, _mod_id: &str, _out: &mut Vec<String>) {}
    /// Button `index` of mod `mod_id`'s settings page was clicked.
    fn settings_click(&self, _game: &mut dyn ClientView, _mod_id: &str, _index: usize) {}
    /// The game rules mods registered, in registration order.
    fn game_rules(&self, _out: &mut Vec<GameRuleDef>) {}
    /// The current value of Fish rule `name` in `world` (a [`ClientView::world_id`]); `None` when
    /// no mod registered the rule.
    fn game_rule_get(&self, _world: &str, _name: &str) -> Option<RuleValue> {
        None
    }
    /// Sets Fish rule `name` in `world` and returns the previous value. Fails with a message when
    /// the rule is unknown or `value` has the wrong type or lies outside the rule's bounds.
    fn game_rule_set(
        &self,
        _world: &str,
        _name: &str,
        _value: &RuleValue,
    ) -> Result<RuleValue, String> {
        Err("no Fish game rules".to_string())
    }
    /// A world was opened: reads its saved Fish rule values (`data/fish/game_rules.json`).
    fn game_rules_load(&self, _world: &str) {}
    /// A world is being saved: writes its Fish rule values beside the game's own.
    fn game_rules_save(&self, _world: &str) {}
}

/// What `/gamerule <fish rule> [<value>]` did, for the game (and the simulation) to word.
#[derive(Debug, Clone, PartialEq)]
pub enum GameRuleReply {
    /// No value was given: the rule's current value.
    Query {
        /// The rule.
        name: String,
        /// Its value, as text.
        value: String,
        /// The command result (a boolean as 0 or 1, an integer as is, a decimal truncated, an
        /// enum as the index of its choice).
        result: i32,
    },
    /// The rule was set.
    Set {
        /// The rule.
        name: String,
        /// The new value, as text.
        value: String,
        /// The command result, as for `Query`.
        result: i32,
    },
    /// The rule already had the value: nothing changed.
    Unchanged {
        /// The rule.
        name: String,
        /// The value, as text.
        value: String,
    },
    /// The value was refused; the text says why.
    Failed(String),
}

/// The result number a rule value stands for in a command (`/execute store`).
fn rule_result(def: &GameRuleDef, value: &RuleValue) -> i32 {
    match value {
        RuleValue::Bool(b) => i32::from(*b),
        RuleValue::Int(i) => *i,
        RuleValue::Double(d) => *d as i32,
        RuleValue::Text(t) => match &def.kind {
            RuleKind::Enum(choices) => choices.iter().position(|c| c == t).unwrap_or(0) as i32,
            _ => 0,
        },
    }
}

/// Runs `/gamerule <name> [<value>]` for a Fish rule in `world`. `None` when no mod registered
/// `name`: the game then treats it as it does any other unknown rule. `value` is the argument's
/// text (`true`, `7`, `1.5`, a choice).
pub fn game_rule_command(
    dispatch: &dyn Dispatch,
    world: &str,
    name: &str,
    value: Option<&str>,
) -> Option<GameRuleReply> {
    let mut defs = Vec::new();
    dispatch.game_rules(&mut defs);
    let def = defs.into_iter().find(|d| d.name == name)?;
    let current = dispatch
        .game_rule_get(world, name)
        .unwrap_or_else(|| def.default.clone());
    let Some(text) = value else {
        return Some(GameRuleReply::Query {
            name: def.name.clone(),
            value: current.to_string(),
            result: rule_result(&def, &current),
        });
    };
    let wanted = match def.parse(text) {
        Ok(v) => v,
        Err(why) => return Some(GameRuleReply::Failed(why)),
    };
    if wanted == current {
        return Some(GameRuleReply::Unchanged {
            name: def.name,
            value: wanted.to_string(),
        });
    }
    Some(match dispatch.game_rule_set(world, name, &wanted) {
        Ok(_) => GameRuleReply::Set {
            name: def.name.clone(),
            value: wanted.to_string(),
            result: rule_result(&def, &wanted),
        },
        Err(why) => GameRuleReply::Failed(why),
    })
}

static DISPATCH: OnceLock<&'static dyn Dispatch> = OnceLock::new();

/// Installs the dispatcher; returns false if one is already installed.
pub fn install(dispatch: &'static dyn Dispatch) -> bool {
    DISPATCH.set(dispatch).is_ok()
}

/// The installed dispatcher, if any.
#[inline]
pub fn get() -> Option<&'static dyn Dispatch> {
    DISPATCH.get().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Nothing;
    impl Dispatch for Nothing {}

    struct View;
    impl ClientView for View {
        fn role(&self) -> Role {
            Role::Window
        }
        fn player(&self) -> Option<PlayerSnapshot> {
            None
        }
        fn show_chat(&mut self, _: &str) {}
        fn game_dir(&self) -> PathBuf {
            PathBuf::new()
        }
    }

    struct Hud;
    impl HudView for Hud {
        fn fill(&mut self, _: [f32; 4], _: u32) {}
        fn text(&mut self, _: &str, _: f32, _: f32, _: u32, _: bool) {}
        fn text_width(&self, t: &str) -> f32 {
            t.len() as f32
        }
        fn gui_size(&self) -> [f32; 2] {
            [1.0, 1.0]
        }
    }

    #[test]
    fn defaults_do_nothing_and_decline() {
        let d = Nothing;
        let mut v = View;
        d.tick_start(&mut v);
        d.tick_end(&mut v);
        d.frame(&mut v, 0.016);
        d.hud(&mut v, &mut Hud);
        d.key(&mut v, "key.keyboard.z", true);
        d.settings_click(&mut v, "m", 0);
        assert_eq!(
            d.game_event(Some(&mut v), &GameEvent::new(Kind::UseBlock)),
            Outcome::Pass
        );
        assert!(d.wants(Kind::ChunkLoad));
        let mut lines = Vec::new();
        d.debug_lines(&mut lines);
        assert!(lines.is_empty());
        assert_eq!(
            d.run_command(
                &mut v,
                &CommandSource {
                    name: "a",
                    uuid: [0; 16],
                    level: 0,
                    is_player: true,
                    pos: None
                },
                "x"
            ),
            CommandOutcome::default()
        );
        assert!(d.command_roots().is_empty());
        assert!(
            d.suggest_command(
                &CommandSource {
                    name: "a",
                    uuid: [0; 16],
                    level: 0,
                    is_player: true,
                    pos: None
                },
                "x"
            )
            .is_empty()
        );
        d.shadowed_command("x");
        assert_eq!(v.level_id(), "minecraft:overworld");
        assert!(!d.client_command(&mut v, "fish hello"));
        let mut rows = Vec::new();
        d.list_mods(&mut rows);
        assert!(rows.is_empty());
        let mut buttons = Vec::new();
        d.settings_buttons("m", &mut buttons);
        assert!(buttons.is_empty());
        assert_eq!(Hud.text_width("abc"), 3.0);
        assert_eq!(Hud.gui_size(), [1.0, 1.0]);
        let mut rules = Vec::new();
        d.game_rules(&mut rules);
        assert!(rules.is_empty());
        assert_eq!(d.game_rule_get("w", "a:b"), None);
        assert!(d.game_rule_set("w", "a:b", &RuleValue::Bool(true)).is_err());
        d.game_rules_load("w");
        d.game_rules_save("w");
        assert_eq!(v.world_id(), None);
    }

    #[test]
    fn kinds_are_listed_once_in_table_order() {
        let mut seen = std::collections::HashSet::new();
        for (i, k) in Kind::ALL.iter().enumerate() {
            assert!(seen.insert(k.name()), "{k:?} twice");
            assert_eq!(Kind::ALL[i], *k);
        }
        assert!(GameEvent::new(Kind::Chat).face == 255 && !Outcome::Pass.cancelled());
        assert!(Outcome::Cancel.cancelled());
    }

    #[test]
    fn roles() {
        assert!(!Role::Window.is_server());
        assert!(Role::IntegratedServer.is_server());
        assert!(Role::DedicatedServer.is_server());
    }

    #[test]
    fn install_is_once() {
        static D: Nothing = Nothing;
        // `get` is None until installed; the first install wins.
        let first = install(&D);
        assert!(get().is_some());
        assert!(!install(&D));
        let _ = first;
    }

    fn def(kind: RuleKind, default: RuleValue) -> GameRuleDef {
        GameRuleDef {
            name: "m:r".to_string(),
            category: "misc".to_string(),
            kind,
            default,
        }
    }

    #[test]
    fn rule_values_are_checked_against_their_kind_and_bounds() {
        let b = def(RuleKind::Bool, RuleValue::Bool(false));
        assert_eq!(b.parse("true"), Ok(RuleValue::Bool(true)));
        assert_eq!(
            b.parse("yes").unwrap_err(),
            "m:r takes true or false, not yes"
        );
        assert_eq!(
            b.check(&RuleValue::Int(1)).unwrap_err(),
            "m:r expects a boolean, got an integer"
        );
        let i = def(RuleKind::Int { min: 0, max: 10 }, RuleValue::Int(3));
        assert_eq!(i.parse("7"), Ok(RuleValue::Int(7)));
        assert_eq!(
            i.parse("11").unwrap_err(),
            "m:r must be between 0 and 10, not 11"
        );
        assert_eq!(i.parse("x").unwrap_err(), "m:r takes a whole number, not x");
        let d = def(
            RuleKind::Double {
                min: -1.0,
                max: 1.0,
            },
            RuleValue::Double(0.5),
        );
        assert_eq!(d.parse("0.25"), Ok(RuleValue::Double(0.25)));
        assert!(d.parse("2").is_err());
        assert!(d.parse("nan").is_err());
        assert_eq!(d.parse("x").unwrap_err(), "m:r takes a number, not x");
        let e = def(
            RuleKind::Enum(vec!["easy".into(), "hard".into()]),
            RuleValue::Text("easy".into()),
        );
        assert_eq!(e.parse("hard"), Ok(RuleValue::Text("hard".into())));
        assert_eq!(
            e.parse("medium").unwrap_err(),
            "m:r must be one of easy, hard, not medium"
        );
        assert!(e.check(&RuleValue::Double(1.0)).is_err());
        assert!(d.check(&RuleValue::Text("a".into())).is_err());
        assert!(e.check(&RuleValue::Bool(true)).is_err());
    }

    #[test]
    fn rule_values_display_like_the_game_prints_them() {
        assert_eq!(RuleValue::Bool(true).to_string(), "true");
        assert_eq!(RuleValue::Int(-4).to_string(), "-4");
        assert_eq!(RuleValue::Double(2.0).to_string(), "2.0");
        assert_eq!(RuleValue::Double(0.125).to_string(), "0.125");
        assert_eq!(RuleValue::Text("hard".into()).to_string(), "hard");
    }

    /// A dispatcher with two rules and values kept in memory.
    struct Rules(std::sync::Mutex<std::collections::HashMap<String, RuleValue>>);

    impl Dispatch for Rules {
        fn game_rules(&self, out: &mut Vec<GameRuleDef>) {
            out.push(def(RuleKind::Int { min: 0, max: 10 }, RuleValue::Int(3)));
            let mut mode = def(
                RuleKind::Enum(vec!["easy".into(), "hard".into()]),
                RuleValue::Text("easy".into()),
            );
            mode.name = "m:mode".to_string();
            out.push(mode);
        }

        fn game_rule_get(&self, _world: &str, name: &str) -> Option<RuleValue> {
            self.0.lock().unwrap().get(name).cloned()
        }

        fn game_rule_set(
            &self,
            _world: &str,
            name: &str,
            value: &RuleValue,
        ) -> Result<RuleValue, String> {
            if name == "m:r" && *value == RuleValue::Int(9) {
                return Err("refused".to_string());
            }
            let old = self
                .0
                .lock()
                .unwrap()
                .insert(name.to_string(), value.clone());
            Ok(old.unwrap_or(RuleValue::Int(3)))
        }
    }

    #[test]
    fn the_gamerule_command_queries_sets_and_refuses() {
        let d = Rules(Default::default());
        let run = |name: &str, value: Option<&str>| game_rule_command(&d, "w", name, value);
        assert_eq!(run("m:nope", None), None);
        assert_eq!(
            run("m:r", None),
            Some(GameRuleReply::Query {
                name: "m:r".into(),
                value: "3".into(),
                result: 3
            })
        );
        assert_eq!(
            run("m:r", Some("3")),
            Some(GameRuleReply::Unchanged {
                name: "m:r".into(),
                value: "3".into()
            })
        );
        assert_eq!(
            run("m:r", Some("7")),
            Some(GameRuleReply::Set {
                name: "m:r".into(),
                value: "7".into(),
                result: 7
            })
        );
        assert_eq!(
            run("m:r", Some("11")),
            Some(GameRuleReply::Failed(
                "m:r must be between 0 and 10, not 11".into()
            ))
        );
        assert_eq!(
            run("m:r", Some("9")),
            Some(GameRuleReply::Failed("refused".into()))
        );
        assert_eq!(
            run("m:mode", Some("hard")),
            Some(GameRuleReply::Set {
                name: "m:mode".into(),
                value: "hard".into(),
                result: 1
            })
        );
        let double = def(
            RuleKind::Double { min: 0.0, max: 9.0 },
            RuleValue::Double(2.5),
        );
        assert_eq!(rule_result(&double, &RuleValue::Double(2.5)), 2);
        assert_eq!(rule_result(&double, &RuleValue::Text("x".into())), 0);
    }
}
