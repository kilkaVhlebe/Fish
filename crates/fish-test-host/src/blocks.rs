//! Fish blocks in the simulation: an in-memory block world, the state ids Horizon would assign and
//! the events the game raises for placing, using, breaking and ticking a Fish block.

use std::ops::Range;
use std::path::PathBuf;

use fish_hooks::{BlockDef, BlockEvent, BlockEventKind, BlockRun, Dispatch, Outcome};

use crate::TestHost;
use crate::events::{outcome_name, uuid_of};
use crate::script::{Expect, ScriptFailure};

/// The first state id of a content-mod block: where the states of vanilla's blocks end.
pub const FIRST_FISH_STATE: u32 = 35_723;

/// The six neighbours of a block, in the order of the face numbers (down, up, north, south, west, east).
const NEIGHBOURS: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];

/// The face number of `name` (`down`, `up`, `north`, `south`, `west`, `east`).
pub fn face_number(name: &str) -> Option<u8> {
    ["down", "up", "north", "south", "west", "east"]
        .iter()
        .position(|f| *f == name)
        .and_then(|i| u8::try_from(i).ok())
}

/// The hand number of `name` (`main` is 0, `off` is 1).
pub fn hand_number(name: &str) -> Option<u8> {
    match name {
        "main" => Some(0),
        "off" => Some(1),
        _ => None,
    }
}

/// The `x y z` text of a position, as `expect.blocks` keys write it.
fn pos_text(pos: [i32; 3]) -> String {
    format!("{} {} {}", pos[0], pos[1], pos[2])
}

/// Parses an `x y z` key.
fn parse_pos(text: &str) -> Option<[i32; 3]> {
    let mut parts = text.split_whitespace().map(|p| p.parse::<i32>());
    let pos = [
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    ];
    parts.next().is_none().then_some(pos)
}

/// A block id as (namespace, path): Horizon orders blocks by mod, then by block.
fn id_parts(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("", id))
}

/// The added properties of a block in the order the game lays its states out: sorted by name,
/// the last one changing fastest (vanilla's order; the simulated template has no properties).
fn layout_order(properties: &[fish_hooks::StatePropertyDef]) -> Vec<&fish_hooks::StatePropertyDef> {
    let mut sorted: Vec<&fish_hooks::StatePropertyDef> = properties.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    sorted
}

/// The text of state `index` of a block with the added properties `properties`, as the game
/// reports it: `k=v` pairs sorted by key.
pub fn combination(properties: &[fish_hooks::StatePropertyDef], mut index: u32) -> String {
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for p in layout_order(properties).into_iter().rev() {
        let n = p.values.len() as u32;
        pairs.push((&p.name, &p.values[(index % n) as usize]));
        index /= n;
    }
    fish_hooks::properties_text(pairs)
}

/// The index of the state of a block with every added property at its default.
pub fn default_index(properties: &[fish_hooks::StatePropertyDef]) -> u32 {
    layout_order(properties)
        .into_iter()
        .fold(0, |at, p| at * p.values.len() as u32 + p.default as u32)
}

impl TestHost {
    /// Assigns the states of the Fish blocks like Horizon does (blocks sorted by mod and path, one
    /// state per combination of their added state properties in vanilla's order, from
    /// [`FIRST_FISH_STATE`]), writes the content folders and tells the loader the runs and the
    /// properties of every state. Runs at the end of boot.
    pub(crate) fn build_blocks(&mut self) {
        let dispatch = self.rules_dispatch();
        let mut defs = Vec::new();
        dispatch.blocks(&mut defs);
        defs.sort_by(|a, b| id_parts(&a.id).cmp(&id_parts(&b.id)));
        let mut runs: Vec<BlockRun> = Vec::new();
        let mut properties: Vec<(u32, String)> = Vec::new();
        let mut next = FIRST_FISH_STATE;
        for def in &defs {
            let count = fish_hooks::property_combinations(&def.properties) as u32;
            for i in 0..count {
                properties.push((next + i, combination(&def.properties, i)));
            }
            runs.push(BlockRun {
                block: def.id.clone(),
                first: next,
                count,
            });
            next += count;
        }
        let dirs = dispatch.block_content();
        dispatch.blocks_built(&runs);
        dispatch.block_properties(&properties);
        self.sim.block_runs = runs;
        self.sim.block_dirs = dirs;
        self.sim.state_properties = properties.into_iter().collect();
    }

    /// Every block a mod registered.
    pub fn block_defs(&self) -> Vec<BlockDef> {
        let mut defs = Vec::new();
        self.rules_dispatch().blocks(&mut defs);
        defs
    }

    /// The content folders the loader wrote for the Fish blocks.
    pub fn block_content_dirs(&self) -> &[PathBuf] {
        &self.sim.block_dirs
    }

    /// The states the simulated game gave block `id`.
    pub fn block_state_range(&self, id: &str) -> Option<Range<u32>> {
        let run = self.sim.block_runs.iter().find(|r| r.block == id)?;
        Some(run.first..run.first + run.count)
    }

    /// The block the simulated world holds at `pos`; `None` for air.
    pub fn block_at(&self, pos: [i32; 3]) -> Option<&str> {
        self.sim.blocks.get(&pos).map(String::as_str)
    }

    /// The state of the Fish block at `pos`; `None` for air, vanilla blocks and blocks the loader
    /// does not know. A block placed by the script is in its default state until a mod sets
    /// another.
    pub fn fish_state(&self, pos: [i32; 3]) -> Option<u32> {
        let id = self.sim.blocks.get(&pos)?;
        let run = self.sim.block_runs.iter().find(|r| &r.block == id)?;
        let state = match self.sim.block_states.get(&pos) {
            Some(s) if (run.first..run.first + run.count).contains(s) => *s,
            _ => {
                let def = self.block_defs().into_iter().find(|d| &d.id == id)?;
                run.first + default_index(&def.properties)
            }
        };
        self.rules_dispatch().is_block_state(state).then_some(state)
    }

    /// The properties of the Fish block state at `pos` (`k=v,...`), as the simulated game reports
    /// them.
    pub fn block_properties_at(&self, pos: [i32; 3]) -> Option<&str> {
        let state = self.fish_state(pos)?;
        self.sim.state_properties.get(&state).map(String::as_str)
    }

    /// Raises one block event in the server's game (the integrated one on a client host) and
    /// records it in `sim.events` as `block/<name> <outcome>`. Like the game (patch 0007), the
    /// events of the block simulation (neighbour changes, random and scheduled ticks) come without
    /// a game view.
    pub fn block_event(&mut self, mut event: BlockEvent<'_>) -> Outcome {
        event.role = self.server_role();
        let role = event.role;
        let (mut view, dispatch) = self.view(role);
        let simulated = matches!(
            event.kind,
            BlockEventKind::NeighborChanged
                | BlockEventKind::RandomTick
                | BlockEventKind::ScheduledTick
        );
        let outcome = if simulated {
            dispatch.block_event(None, &event)
        } else {
            dispatch.block_event(Some(&mut view), &event)
        };
        self.sim.events.push(format!(
            "block/{} {}",
            event.kind.name(),
            outcome_name(&outcome)
        ));
        self.drain_trace();
        outcome
    }

    /// Raises `kind` for the Fish block `state` at `pos` caused by `actor`, then
    /// `NeighborChanged` for every Fish block around `pos`.
    pub(crate) fn fish_block_changed(
        &mut self,
        kind: BlockEventKind,
        state: u32,
        pos: [i32; 3],
        actor: &str,
    ) {
        let mut event = BlockEvent::new(kind, state, pos);
        event.actor = actor;
        event.actor_uuid = uuid_of(actor);
        self.block_event(event);
        self.neighbours_changed(pos);
    }

    /// `NeighborChanged` (`other_pos` = `pos`) for each of the six neighbours that is a Fish block.
    fn neighbours_changed(&mut self, pos: [i32; 3]) {
        for d in NEIGHBOURS {
            let at = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
            if let Some(state) = self.fish_state(at) {
                let mut event = BlockEvent::new(BlockEventKind::NeighborChanged, state, at);
                event.other_pos = pos;
                self.block_event(event);
            }
        }
    }

    /// `actor` places `block` at `pos` holding `item`: the block enters the world; a Fish block
    /// gets its `Place` event, and the Fish blocks around it `NeighborChanged`.
    pub fn place_block(&mut self, pos: [i32; 3], block: &str, item: &str, actor: &str) {
        self.sim.blocks.insert(pos, block.to_string());
        self.sim.block_states.remove(&pos);
        let Some(state) = self.fish_state(pos) else {
            return;
        };
        let mut event = BlockEvent::new(BlockEventKind::Place, state, pos);
        event.actor = actor;
        event.actor_uuid = uuid_of(actor);
        event.item = item;
        self.block_event(event);
        self.neighbours_changed(pos);
    }

    /// A random tick picks the block at `pos`. Returns true when it was a Fish block.
    pub fn random_tick(&mut self, pos: [i32; 3]) -> bool {
        self.tick_block(BlockEventKind::RandomTick, pos)
    }

    /// A scheduled tick of the block at `pos` runs. Returns true when it was a Fish block.
    pub fn scheduled_tick(&mut self, pos: [i32; 3]) -> bool {
        self.tick_block(BlockEventKind::ScheduledTick, pos)
    }

    fn tick_block(&mut self, kind: BlockEventKind, pos: [i32; 3]) -> bool {
        let Some(state) = self.fish_state(pos) else {
            return false;
        };
        self.block_event(BlockEvent::new(kind, state, pos));
        true
    }

    /// The first failed block expectation of `expect`, for step `step`.
    pub(crate) fn check_blocks(&self, step: usize, expect: &Expect) -> Option<ScriptFailure> {
        let fail = |what: &str, expected: String, actual: String| ScriptFailure {
            step,
            what: what.to_string(),
            expected,
            actual,
        };
        for (key, want) in &expect.blocks {
            let Some(pos) = parse_pos(key) else {
                return Some(fail(
                    "expect.blocks",
                    "a position `x y z`".to_string(),
                    format!("{key:?}"),
                ));
            };
            let actual = self.block_at(pos).unwrap_or("air");
            if actual != want {
                return Some(fail(
                    "expect.blocks",
                    format!("{want} at {}", pos_text(pos)),
                    format!("{actual} at {}", pos_text(pos)),
                ));
            }
        }
        for (id, want) in &expect.block_states {
            let actual = self
                .block_state_range(id)
                .map(|r| [r.start, r.end - r.start]);
            if actual != Some(*want) {
                return Some(fail(
                    "expect.block_states",
                    format!("{id} states {want:?}"),
                    actual.map_or_else(
                        || format!("{id} has no states"),
                        |a| format!("{id} states {a:?}"),
                    ),
                ));
            }
        }
        for (key, want) in &expect.block_properties {
            let Some(pos) = parse_pos(key) else {
                return Some(fail(
                    "expect.block_properties",
                    "a position `x y z`".to_string(),
                    format!("{key:?}"),
                ));
            };
            let actual = self.block_properties_at(pos);
            if actual != Some(want.as_str()) {
                return Some(fail(
                    "expect.block_properties",
                    format!("{want} at {}", pos_text(pos)),
                    format!("{actual:?} at {}", pos_text(pos)),
                ));
            }
        }
        if let Some(want) = &expect.use_result {
            let wanted = match want.as_str() {
                "success" | "consume" => "handled",
                "fail" => "cancel",
                "pass" => "pass",
                other => {
                    return Some(fail(
                        "expect.use_result",
                        "success, consume, fail or pass".to_string(),
                        format!("{other:?}"),
                    ));
                }
            };
            if self.sim.last_use != wanted {
                return Some(fail(
                    "expect.use_result",
                    format!("{want} ({wanted})"),
                    format!("{:?}", self.sim.last_use),
                ));
            }
        }
        for rel in &expect.content_files {
            if !self.content_file_exists(rel) {
                return Some(fail(
                    "expect.content_files",
                    format!("{rel} in a content folder"),
                    format!("folders {:?}", self.sim.block_dirs),
                ));
            }
        }
        for rel in &expect.no_content_files {
            if self.content_file_exists(rel) {
                return Some(fail(
                    "expect.no_content_files",
                    format!("no {rel} in any content folder"),
                    "the file exists".to_string(),
                ));
            }
        }
        None
    }

    fn content_file_exists(&self, rel: &str) -> bool {
        self.sim.block_dirs.iter().any(|d| d.join(rel).is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_and_hands_have_numbers() {
        assert_eq!(face_number("down"), Some(0));
        assert_eq!(face_number("up"), Some(1));
        assert_eq!(face_number("east"), Some(5));
        assert_eq!(face_number("sideways"), None);
        assert_eq!(hand_number("main"), Some(0));
        assert_eq!(hand_number("off"), Some(1));
        assert_eq!(hand_number("third"), None);
    }

    #[test]
    fn positions_round_trip_through_their_text() {
        assert_eq!(parse_pos("1 -2 3"), Some([1, -2, 3]));
        assert_eq!(parse_pos(&pos_text([4, 5, 6])), Some([4, 5, 6]));
        assert_eq!(parse_pos("1 2"), None);
        assert_eq!(parse_pos("1 2 3 4"), None);
        assert_eq!(parse_pos("a b c"), None);
    }

    #[test]
    fn ids_order_by_mod_then_path() {
        assert!(id_parts("a:z") < id_parts("a1:a"));
        assert!(id_parts("a:b") < id_parts("a:c"));
        assert_eq!(id_parts("plain"), ("", "plain"));
    }
}
