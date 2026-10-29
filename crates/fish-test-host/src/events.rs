//! Game events in the simulation: the flows the edition's hooks implement, replayed in process.
//!
//! Each method raises the same [`GameEvent`]s, in the same order, as the bridge does in the
//! edition, so a script exercises the mod's listeners exactly like the game would.

use fish_hooks::{Dispatch, GameEvent, Kind, Outcome, Role};
use fish_loader::Side;

use crate::TestHost;

/// The default acting player of the simulation.
pub const PLAYER: &str = "Steve";
/// The default dimension of the simulation.
pub const OVERWORLD: &str = "minecraft:overworld";

fn outcome_name(o: &Outcome) -> String {
    match o {
        Outcome::Pass => "pass".to_string(),
        Outcome::Handled => "handled".to_string(),
        Outcome::Cancel => "cancel".to_string(),
        Outcome::Replace(t) => format!("replace:{t}"),
    }
}

fn uuid_of(name: &str) -> [u8; 16] {
    let mut h: u128 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h = (h ^ u128::from(b)).wrapping_mul(0x0000_0100_0000_01b3_0000_0000_0000_013b);
    }
    (h | 1).to_be_bytes()
}

impl TestHost {
    fn server_role(&self) -> Role {
        match self.side {
            Side::Client => Role::IntegratedServer,
            Side::Server => Role::DedicatedServer,
        }
    }

    /// Raises one game event; the outcome is also recorded in `sim.events`/`sim.last_outcome`.
    ///
    /// Events of client kinds run in the window's game, the others in the server (the integrated
    /// one on a client host). Chunk events and `client_stopped` are detached (no game view), as in
    /// the edition.
    pub fn game_event(&mut self, event: &GameEvent<'_>) -> Outcome {
        let name = event.kind.name();
        let client = name.starts_with("message/allow_receive")
            || name.starts_with("message/receive")
            || name.contains("send")
            || name.starts_with("lifecycle/client");
        let role = if client {
            self.game_role()
        } else {
            self.server_role()
        };
        let detached = matches!(
            event.kind,
            Kind::ChunkLoad | Kind::ChunkUnload | Kind::ClientStopped
        );
        let mut e = event.clone();
        e.role = role;
        let (mut view, dispatch) = self.view(role);
        let outcome = if detached {
            dispatch.game_event(None, &e)
        } else {
            dispatch.game_event(Some(&mut view), &e)
        };
        let line = format!("{name} {}", outcome_name(&outcome));
        self.sim.last_outcome = outcome_name(&outcome);
        self.sim.events.push(line);
        outcome
    }

    fn at(kind: Kind) -> GameEvent<'static> {
        let mut e = GameEvent::new(kind);
        e.actor = PLAYER;
        e.actor_uuid = uuid_of(PLAYER);
        e.dimension = OVERWORLD;
        e
    }

    /// Fires the lifecycle events of a server start (`starting`, the first tick, `started`,
    /// `world_load`) and of the client's start.
    pub fn start_game(&mut self) {
        self.game_event(&Self::at(Kind::ServerStarting));
        self.game_event(&Self::at(Kind::WorldLoad));
        self.game_event(&Self::at(Kind::ServerStarted));
        self.game_event(&Self::at(Kind::ClientStarted));
    }

    /// Fires the stop events: server `stopping`, `world_unload`, `stopped`, then the client's.
    pub fn stop_game(&mut self) {
        self.game_event(&Self::at(Kind::ClientStopping));
        self.game_event(&Self::at(Kind::ServerStopping));
        self.game_event(&Self::at(Kind::WorldUnload));
        self.game_event(&Self::at(Kind::ServerStopped));
        self.game_event(&Self::at(Kind::ClientStopped));
    }

    /// A player joins.
    pub fn join(&mut self, name: &str) {
        let mut e = Self::at(Kind::PlayerJoin);
        e.actor = name;
        e.actor_uuid = uuid_of(name);
        self.game_event(&e);
        self.sim.players.insert(name.to_string());
    }

    /// A player leaves.
    pub fn leave(&mut self, name: &str) {
        let mut e = Self::at(Kind::PlayerLeave);
        e.actor = name;
        e.actor_uuid = uuid_of(name);
        self.game_event(&e);
        self.sim.players.remove(name);
    }

    /// The player uses `item` on the block `block` at `pos`. The client's prediction fires first;
    /// the server decides if the client did not cancel. Returns the final outcome.
    pub fn use_block(&mut self, pos: [i32; 3], block: &str, item: &str) -> Outcome {
        let mut e = Self::at(Kind::UseBlock);
        e.pos = pos;
        e.face = 1;
        e.subject = block;
        e.item = item;
        e.is_client = true;
        let predicted = self.game_event(&e);
        if predicted.cancelled() {
            return predicted;
        }
        e.is_client = false;
        self.game_event(&e)
    }

    /// The player uses `item` in the air.
    pub fn use_item(&mut self, item: &str) -> Outcome {
        let mut e = Self::at(Kind::UseItem);
        e.item = item;
        self.game_event(&e)
    }

    /// The player attacks entity `id` of type `kind`.
    pub fn attack_entity(&mut self, id: i64, kind: &str) -> Outcome {
        let mut e = Self::at(Kind::AttackEntity);
        e.entity_id = id;
        e.subject = kind;
        self.game_event(&e)
    }

    /// The player attacks (starts mining) the block at `pos`.
    pub fn attack_block(&mut self, pos: [i32; 3], block: &str) -> Outcome {
        let mut e = Self::at(Kind::AttackBlock);
        e.pos = pos;
        e.face = 1;
        e.subject = block;
        self.game_event(&e)
    }

    /// The player breaks the block at `pos`: `block_break_before`, then either `canceled` (when a
    /// listener cancelled) or the break itself, whose `block_break_after` fires on the next tick.
    /// Returns true when the block broke.
    pub fn break_block(&mut self, pos: [i32; 3], block: &str) -> bool {
        let mut e = Self::at(Kind::BlockBreakBefore);
        e.pos = pos;
        e.subject = block;
        if self.game_event(&e).cancelled() {
            e.kind = Kind::BlockBreakCanceled;
            self.game_event(&e);
            return false;
        }
        self.sim
            .broken
            .push(format!("{} {} {} {block}", pos[0], pos[1], pos[2]));
        self.sim.pending_after.push((pos, block.to_string()));
        true
    }

    /// A living entity takes damage; returns true when it was applied.
    pub fn damage(&mut self, id: i64, kind: &str, amount: f32, source: &str) -> bool {
        let mut e = GameEvent::new(Kind::AllowDamage);
        e.entity_id = id;
        e.subject = kind;
        e.amount = amount;
        e.extra = source;
        e.dimension = OVERWORLD;
        if self.game_event(&e).cancelled() {
            return false;
        }
        e.kind = Kind::AfterDamage;
        self.game_event(&e);
        true
    }

    /// A living entity dies; returns true when it died.
    pub fn death(&mut self, id: i64, kind: &str) -> bool {
        let mut e = GameEvent::new(Kind::AllowDeath);
        e.entity_id = id;
        e.subject = kind;
        e.dimension = OVERWORLD;
        if self.game_event(&e).cancelled() {
            return false;
        }
        e.kind = Kind::AfterDeath;
        self.game_event(&e);
        true
    }

    /// `/reload`: `reload_start` now, `reload_end` on the next tick.
    pub fn reload(&mut self) {
        self.game_event(&Self::at(Kind::ReloadStart));
        self.sim.pending_reload_end = true;
    }

    /// A player's chat message reaches the server: `allow_chat`, then `chat` and the broadcast.
    /// Returns true when it was broadcast.
    pub fn chat_in(&mut self, actor: &str, text: &str) -> bool {
        let mut e = Self::at(Kind::AllowChat);
        e.actor = actor;
        e.subject = text;
        if self.game_event(&e).cancelled() {
            return false;
        }
        e.kind = Kind::Chat;
        self.game_event(&e);
        self.sim.broadcast.push(format!("<{actor}> {text}"));
        true
    }

    /// The local player types a chat line: `allow_send`, `modify_send` (may rewrite) and `send`.
    /// Returns what was sent, or `None` when cancelled. Lines starting with `/` are commands and
    /// go through [`command`](Self::command) instead, as in the game.
    pub fn chat_out(&mut self, text: &str) -> Option<String> {
        let mut e = GameEvent::new(Kind::AllowSend);
        e.subject = text;
        if self.game_event(&e).cancelled() {
            return None;
        }
        e.kind = Kind::ModifySend;
        e.rewritable = true;
        let sent = match self.game_event(&e) {
            Outcome::Replace(t) => t,
            _ => text.to_string(),
        };
        e.kind = Kind::Send;
        e.rewritable = false;
        e.subject = &sent;
        self.game_event(&e);
        self.sim.sent_chat.push(sent.clone());
        Some(sent)
    }

    /// A chat line arrives at the client: `allow_receive`, then `receive` and display.
    pub fn chat_received(&mut self, text: &str, source: &str) -> bool {
        let mut e = GameEvent::new(Kind::AllowReceive);
        e.subject = text;
        e.extra = source;
        if self.game_event(&e).cancelled() {
            return false;
        }
        e.kind = Kind::Receive;
        self.game_event(&e);
        self.sim.received.push(text.to_string());
        true
    }

    /// Fires what is due after a tick: `reload_end` and the `block_break_after` of earlier breaks.
    pub(crate) fn after_tick(&mut self) {
        if std::mem::take(&mut self.sim.pending_reload_end) {
            self.game_event(&Self::at(Kind::ReloadEnd));
        }
        for (pos, block) in std::mem::take(&mut self.sim.pending_after) {
            let mut e = Self::at(Kind::BlockBreakAfter);
            e.pos = pos;
            e.subject = &block;
            self.game_event(&e);
        }
    }
}
