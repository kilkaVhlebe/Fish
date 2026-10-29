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
