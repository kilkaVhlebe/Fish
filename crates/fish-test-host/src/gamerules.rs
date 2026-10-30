//! Game rules in the simulation: per-world values kept by the loader, the `/gamerule` command and
//! switching worlds (which saves the old world's rules and loads the new one's, as the game does).

use fish_hooks::{Dispatch, GameRuleDef, GameRuleReply, RuleValue, game_rule_command};

use crate::TestHost;
use crate::sim::world_dir;

impl TestHost {
    /// The id (directory) of the simulated world; `None` at the main menu.
    pub fn world_id(&self) -> Option<String> {
        let name = self.sim.world.as_ref()?;
        Some(
            world_dir(self.game_dir(), name)
                .to_string_lossy()
                .into_owned(),
        )
    }

    pub(crate) fn rules_dispatch(&self) -> &dyn Dispatch {
        self.dispatch
            .as_ref()
            .expect("TestHost::boot() has not been called")
    }

    /// Every rule a mod registered.
    pub fn game_rule_defs(&self) -> Vec<GameRuleDef> {
        let mut defs = Vec::new();
        self.rules_dispatch().game_rules(&mut defs);
        defs
    }

    /// The value of rule `name` in the current world (the default at the main menu); `None` for an
    /// unknown rule.
    pub fn game_rule(&self, name: &str) -> Option<RuleValue> {
        match self.world_id() {
            Some(world) => self.rules_dispatch().game_rule_get(&world, name),
            None => self
                .game_rule_defs()
                .into_iter()
                .find(|d| d.name == name)
                .map(|d| d.default),
        }
    }

    /// Runs `/gamerule <name> [<value>]` for a Fish rule in the current world and shows the reply
    /// in chat the way the game words it. `None` when no world is open or no mod registered `name`
    /// (the game's own command then handles the line).
    pub fn gamerule(&mut self, name: &str, value: Option<&str>) -> Option<GameRuleReply> {
        let world = self.world_id()?;
        let reply = game_rule_command(self.rules_dispatch(), &world, name, value)?;
        let (ok, line) = match &reply {
            GameRuleReply::Query { name, value, .. } => (
                true,
                format!("Gamerule {name} is currently set to: {value}"),
            ),
            GameRuleReply::Set { name, value, .. } => {
                (true, format!("Gamerule {name} is now set to: {value}"))
            }
            GameRuleReply::Unchanged { name, value } => {
                (false, format!("Gamerule {name} is already set to: {value}"))
            }
            GameRuleReply::Failed(why) => (false, why.clone()),
        };
        self.sim.command_ok = Some(ok);
        self.sim.chat.push(line.clone());
        self.sim.command_feedback.push(line);
        self.drain_trace();
        Some(reply)
    }

    /// Opens world `name`: saves the rules of the world that was open, then reads the new
    /// world's from its folder. Switching to the open world reloads it from disk.
    pub fn switch_world(&mut self, name: &str) {
        if let Some(old) = self.world_id() {
            self.rules_dispatch().game_rules_save(&old);
        }
        self.sim.world = Some(name.to_string());
        let new = self.world_id().expect("a world was just opened");
        self.rules_dispatch().game_rules_load(&new);
    }

    /// Leaves the world (the main menu), saving its rules.
    pub fn close_world(&mut self) {
        if let Some(old) = self.world_id() {
            self.rules_dispatch().game_rules_save(&old);
        }
        self.sim.world = None;
    }
}
