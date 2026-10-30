//! The simulated client: the game the hooks see, and what scripts observe.
//!
//! [`TestHost::tick`](crate::TestHost::tick) and friends drive the loader's real hook
//! dispatcher with these views, so mods run exactly as they do in the edition.

use std::collections::BTreeSet;
use std::path::PathBuf;

use fish_hooks::{ClientView, HudView, PlayerSnapshot, Role};

/// State of the simulated client.
#[derive(Debug, Clone, PartialEq)]
pub struct Sim {
    /// Client ticks advanced so far.
    pub ticks: u64,
    /// Frames rendered so far.
    pub frames: u64,
    /// Commands the player ran, in order.
    pub commands: Vec<String>,
    /// Chat messages the player sent, in order.
    pub sent_chat: Vec<String>,
    /// Keys currently held.
    pub keys_down: BTreeSet<String>,
    /// Chat shown to the player (produced by mods).
    pub chat: Vec<String>,
    /// Text drawn on the HUD during the last frame (produced by mods).
    pub hud: Vec<String>,
    /// Feedback of commands (produced by mods).
    pub command_feedback: Vec<String>,
    /// The simulated player; `None` simulates the main menu.
    pub player: Option<PlayerSnapshot>,
    /// Whether the last handled server command succeeded.
    pub command_ok: Option<bool>,
    /// Every game event raised, as `group/name outcome`.
    pub events: Vec<String>,
    /// The outcome of the last game event (`pass`, `handled`, `cancel`, `replace:<text>`).
    pub last_outcome: String,
    /// Chat lines the server broadcast.
    pub broadcast: Vec<String>,
    /// Chat lines the client received and showed.
    pub received: Vec<String>,
    /// Blocks broken, as `x y z block`.
    pub broken: Vec<String>,
    /// Players currently joined.
    pub players: BTreeSet<String>,
    /// The world the game runs (a folder below `saves/` of the game directory); `None` simulates
    /// the main menu. Game rules are kept per world.
    pub world: Option<String>,
    /// The blocks the script placed: position to block id (air is absent).
    pub blocks: std::collections::BTreeMap<[i32; 3], String>,
    /// The states the simulated game gave the Fish blocks, one run per block, sorted by first state.
    pub block_runs: Vec<fish_hooks::BlockRun>,
    /// The content folders `Dispatch::block_content` wrote at boot.
    pub block_dirs: Vec<PathBuf>,
    /// The outcome of the last `use_block` (`pass`, `handled`, `cancel`, `replace:<text>`).
    pub last_use: String,
}

impl Default for Sim {
    fn default() -> Self {
        Sim {
            ticks: 0,
            frames: 0,
            commands: Vec::new(),
            sent_chat: Vec::new(),
            keys_down: BTreeSet::new(),
            chat: Vec::new(),
            hud: Vec::new(),
            command_feedback: Vec::new(),
            command_ok: None,
            events: Vec::new(),
            last_outcome: String::new(),
            broadcast: Vec::new(),
            received: Vec::new(),
            broken: Vec::new(),
            players: BTreeSet::new(),
            blocks: std::collections::BTreeMap::new(),
            block_runs: Vec::new(),
            block_dirs: Vec::new(),
            last_use: String::new(),
            world: Some("world".to_string()),
            player: Some(PlayerSnapshot {
                x: 0.5,
                y: 64.0,
                z: 0.5,
                yaw: 0.0,
                pitch: 0.0,
                dimension: "minecraft:overworld".to_string(),
            }),
        }
    }
}

/// The game as the hooks see it, backed by a [`Sim`].
pub struct SimView<'a> {
    /// Which kind of game this is.
    pub role: Role,
    /// The game directory.
    pub dir: PathBuf,
    /// The simulated state.
    pub sim: &'a mut Sim,
}

impl ClientView for SimView<'_> {
    fn role(&self) -> Role {
        self.role
    }

    fn player(&self) -> Option<PlayerSnapshot> {
        self.sim.player.clone()
    }

    fn show_chat(&mut self, text: &str) {
        self.sim.chat.push(text.to_string());
        // Command feedback is chat too; the script keys observe both.
        self.sim.command_feedback.push(text.to_string());
    }

    fn game_dir(&self) -> PathBuf {
        self.dir.clone()
    }

    fn world_id(&self) -> Option<String> {
        self.sim
            .world
            .as_ref()
            .map(|w| world_dir(&self.dir, w).to_string_lossy().into_owned())
    }
}

/// The folder of the simulated world `name` in the game directory `dir`.
pub fn world_dir(dir: &std::path::Path, name: &str) -> PathBuf {
    dir.join("saves").join(name)
}

/// A HUD that records the text mods draw.
pub struct SimHud {
    /// Text drawn this frame.
    pub texts: Vec<String>,
    /// GUI size reported to mods.
    pub size: [f32; 2],
}

impl Default for SimHud {
    fn default() -> Self {
        SimHud {
            texts: Vec::new(),
            size: [427.0, 240.0],
        }
    }
}

impl HudView for SimHud {
    fn fill(&mut self, _: [f32; 4], _: u32) {}

    fn text(&mut self, text: &str, _: f32, _: f32, _: u32, _: bool) {
        self.texts.push(text.to_string());
    }

    fn text_width(&self, text: &str) -> f32 {
        6.0 * text.chars().count() as f32
    }

    fn gui_size(&self) -> [f32; 2] {
        self.size
    }
}

impl Sim {
    /// Records a key press or release.
    pub fn press_key(&mut self, name: &str, pressed: bool) {
        if pressed {
            self.keys_down.insert(name.to_string());
        } else {
            self.keys_down.remove(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_hud() {
        let mut s = Sim::default();
        s.press_key("H", true);
        s.press_key("J", true);
        s.press_key("H", false);
        assert_eq!(s.keys_down.iter().collect::<Vec<_>>(), ["J"]);
        let mut hud = SimHud::default();
        hud.fill([0.0; 4], 0);
        hud.text("abc", 0.0, 0.0, 0, true);
        assert_eq!(hud.texts, ["abc"]);
        assert_eq!(hud.text_width("abc"), 18.0);
        assert_eq!(hud.gui_size(), [427.0, 240.0]);
    }

    #[test]
    fn the_view_reflects_the_simulation() {
        let mut sim = Sim::default();
        let mut view = SimView {
            role: Role::Window,
            dir: PathBuf::from("g"),
            sim: &mut sim,
        };
        assert_eq!(view.role(), Role::Window);
        assert_eq!(view.player().unwrap().dimension, "minecraft:overworld");
        view.show_chat("hi");
        assert_eq!(view.game_dir(), PathBuf::from("g"));
        assert_eq!(sim.chat, ["hi"]);
        assert_eq!(sim.command_feedback, ["hi"]);
        sim.player = None;
        let view = SimView {
            role: Role::Window,
            dir: PathBuf::new(),
            sim: &mut sim,
        };
        assert!(view.player().is_none());
        assert_eq!(
            view.world_id().unwrap(),
            world_dir(std::path::Path::new(""), "world").to_string_lossy()
        );
        sim.world = None;
        let view = SimView {
            role: Role::Window,
            dir: PathBuf::new(),
            sim: &mut sim,
        };
        assert_eq!(view.world_id(), None);
    }
}
