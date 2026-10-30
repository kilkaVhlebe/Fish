// Fish's views of Horizon's `Game`, implemented over the helpers of `native_hooks`.

use std::path::PathBuf;

use fish_hooks::{ClientView, HudView, PlayerSnapshot, Role};
use horizon_client::hud::HudBatch;

use crate::game::Game;
use crate::game::execution_role::ExecutionRole;

pub(in crate::game) fn role_of(game: &Game<'_>) -> Role {
    match game.options.execution_role {
        ExecutionRole::Client => Role::Window,
        ExecutionRole::IntegratedServer => Role::IntegratedServer,
        ExecutionRole::DedicatedServer => Role::DedicatedServer,
    }
}

fn player_of(game: &Game<'_>) -> Option<PlayerSnapshot> {
    game.native_player().map(|p| PlayerSnapshot {
        x: p.x,
        y: p.y,
        z: p.z,
        yaw: p.yaw,
        pitch: p.pitch,
        dimension: p.dimension.to_string(),
    })
}

fn dir_of(game: &Game<'_>) -> PathBuf {
    if game.options.execution_role == ExecutionRole::DedicatedServer {
        PathBuf::from(".")
    } else {
        horizon_client::logging::game_home()
    }
}

/// A `Game` as the loader's views see it.
pub(in crate::game) struct GameView<'g, 'a> {
    pub(in crate::game) game: &'g mut Game<'a>,
}

impl ClientView for GameView<'_, '_> {
    fn role(&self) -> Role {
        role_of(self.game)
    }

    fn player(&self) -> Option<PlayerSnapshot> {
        player_of(self.game)
    }

    fn show_chat(&mut self, text: &str) {
        if self.game.native_window() {
            self.game.native_chat(text);
        }
    }

    fn game_dir(&self) -> PathBuf {
        dir_of(self.game)
    }

    fn level_id(&self) -> String {
        self.game.dim.id().to_string()
    }

    fn world_id(&self) -> Option<String> {
        super::rules::world_of(self.game)
    }
}

/// A read-only copy of what the HUD pass needs, so the draw target can borrow the game.
/// Chat lines mods show meanwhile are queued and flushed after dispatch.
pub(in crate::game) struct SnapshotView {
    role: Role,
    player: Option<PlayerSnapshot>,
    dir: PathBuf,
    window: bool,
    world: Option<String>,
    pub(in crate::game) chat: Vec<String>,
}

impl SnapshotView {
    pub(in crate::game) fn of(game: &Game<'_>) -> SnapshotView {
        SnapshotView {
            role: role_of(game),
            player: player_of(game),
            dir: dir_of(game),
            window: game.native_window(),
            world: super::rules::world_of(game),
            chat: Vec::new(),
        }
    }
}

impl ClientView for SnapshotView {
    fn role(&self) -> Role {
        self.role
    }

    fn player(&self) -> Option<PlayerSnapshot> {
        self.player.clone()
    }

    fn show_chat(&mut self, text: &str) {
        if self.window {
            self.chat.push(text.to_string());
        }
    }

    fn game_dir(&self) -> PathBuf {
        self.dir.clone()
    }

    fn world_id(&self) -> Option<String> {
        self.world.clone()
    }
}

/// The HUD draw target: Horizon's batch plus the game's fonts and atlas.
pub(in crate::game) struct DrawView<'g, 'a, 'b> {
    pub(in crate::game) game: &'g Game<'a>,
    pub(in crate::game) batch: &'b mut HudBatch,
}

impl HudView for DrawView<'_, '_, '_> {
    fn fill(&mut self, rect: [f32; 4], argb: u32) {
        self.game.native_fill(self.batch, rect, argb);
    }

    fn text(&mut self, text: &str, x: f32, y: f32, rgb: u32, shadow: bool) {
        self.game.native_text(self.batch, text, [x, y], rgb, shadow);
    }

    fn text_width(&self, text: &str) -> f32 {
        self.game.native_text_width(text)
    }

    fn gui_size(&self) -> [f32; 2] {
        [self.batch.width, self.batch.height]
    }
}
