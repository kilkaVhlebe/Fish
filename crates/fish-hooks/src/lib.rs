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
}

impl Kind {
    /// Every kind, in table order.
    pub const ALL: [Kind; 44] = [
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
}
