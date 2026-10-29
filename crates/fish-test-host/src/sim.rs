//! The simulated client: what script steps drive and what scripts observe.
//!
//! Steps record their input here (`ticks`, `frames`, commands, chat, keys) and `expect.chat`,
//! `expect.hud` and `expect.command_feedback` read what mods produced. Mod-facing dispatch of this
//! input is wired up together with the edition's hook views (roadmap item P4.5).

use std::collections::BTreeSet;

/// State of the simulated client.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
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
    /// Text drawn on the HUD (produced by mods).
    pub hud: Vec<String>,
    /// Feedback of commands (produced by mods).
    pub command_feedback: Vec<String>,
}

impl Sim {
    /// Advances `n` client ticks.
    pub fn tick(&mut self, n: u32) {
        self.ticks += u64::from(n);
    }

    /// Renders `n` frames.
    pub fn frame(&mut self, n: u32) {
        self.frames += u64::from(n);
    }

    /// Runs a client command.
    pub fn run_command(&mut self, command: &str) {
        self.commands.push(command.to_string());
    }

    /// Sends a chat message.
    pub fn send_chat(&mut self, text: &str) {
        self.sent_chat.push(text.to_string());
    }

    /// Presses or releases a key.
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
    fn records_input() {
        let mut s = Sim::default();
        s.tick(20);
        s.tick(5);
        s.frame(2);
        s.run_command("fish hello");
        s.send_chat("hi");
        s.press_key("H", true);
        s.press_key("J", true);
        s.press_key("H", false);
        assert_eq!((s.ticks, s.frames), (25, 2));
        assert_eq!(s.commands, ["fish hello"]);
        assert_eq!(s.sent_chat, ["hi"]);
        assert_eq!(s.keys_down.iter().collect::<Vec<_>>(), ["J"]);
    }
}
