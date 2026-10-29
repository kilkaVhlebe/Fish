//! Game event payloads (phase P6): lifecycle, interaction, entity and message events.
//!
//! Every P6 event carries one [`GamePayload`]; the documentation of each id says which fields
//! it fills. Unused fields are zero or empty. Strings are borrowed for the callback.
//! `FirstNonPass` events return an action byte; `Fail` cancels, `Success` or `Consume` mark the
//! event handled (for allow events: allowed).

use core::ffi::c_void;

use crate::types::{FishHandle, FishStr};
use crate::{COMBINE_FIRST_NON_PASS, COMBINE_NOTIFY};

/// Event ids.
pub mod ids {
    /// The window's game ran its first frame.
    pub const CLIENT_STARTED: &str = "fish:lifecycle/client_started";
    /// The window is closing; the world is not saved yet.
    pub const CLIENT_STOPPING: &str = "fish:lifecycle/client_stopping";
    /// The window closed and its world is saved. Detached: no game view.
    pub const CLIENT_STOPPED: &str = "fish:lifecycle/client_stopped";
    /// A server (integrated, dedicated or in-frame) starts loading its world. `subject` = world name.
    pub const SERVER_STARTING: &str = "fish:lifecycle/server_starting";
    /// A server ran its first tick.
    pub const SERVER_STARTED: &str = "fish:lifecycle/server_started";
    /// A server begins to stop; players are not saved yet.
    pub const SERVER_STOPPING: &str = "fish:lifecycle/server_stopping";
    /// A server finished saving and closed.
    pub const SERVER_STOPPED: &str = "fish:lifecycle/server_stopped";
    /// A level (dimension) became loaded. `dimension` = its id.
    pub const WORLD_LOAD: &str = "fish:lifecycle/world_load";
    /// A level is about to be unloaded. `dimension` = its id.
    pub const WORLD_UNLOAD: &str = "fish:lifecycle/world_unload";
    /// A chunk entered the loaded set. `pos` = [chunk x, 0, chunk z]; detached.
    pub const CHUNK_LOAD: &str = "fish:lifecycle/chunk_load";
    /// A chunk is about to leave the loaded set. `pos` = [chunk x, 0, chunk z]; detached.
    pub const CHUNK_UNLOAD: &str = "fish:lifecycle/chunk_unload";
    /// Data (datapacks/functions) reload starts.
    pub const RELOAD_START: &str = "fish:lifecycle/reload_start";
    /// Data reload finished (observed on the next server tick).
    pub const RELOAD_END: &str = "fish:lifecycle/reload_end";
    /// A player uses a block with a hand. `pos`, `face`, `hand`, `subject` = block id, `item` = held item id. `Fail` cancels.
    pub const USE_BLOCK: &str = "fish:interaction/use_block";
    /// A player uses the held item in the air. `hand`, `item`. `Fail` cancels.
    pub const USE_ITEM: &str = "fish:interaction/use_item";
    /// A player uses an entity. `entity_id`, `subject` = entity type, `hand`, `item`. `Fail` cancels.
    pub const USE_ENTITY: &str = "fish:interaction/use_entity";
    /// A player starts attacking (mining) a block. `pos`, `face`, `subject` = block id. `Fail` cancels.
    pub const ATTACK_BLOCK: &str = "fish:interaction/attack_block";
    /// A player attacks an entity. `entity_id`, `subject` = entity type. `Fail` cancels.
    pub const ATTACK_ENTITY: &str = "fish:interaction/attack_entity";
    /// A player is about to break a block. `pos`, `subject` = block id. `Fail` cancels and fires block_break_canceled.
    pub const BLOCK_BREAK_BEFORE: &str = "fish:interaction/block_break_before";
    /// A player broke a block (observed on the next tick). `pos`, `subject` = the block that was there.
    pub const BLOCK_BREAK_AFTER: &str = "fish:interaction/block_break_after";
    /// A block break was cancelled by a listener. `pos`, `subject`.
    pub const BLOCK_BREAK_CANCELED: &str = "fish:interaction/block_break_canceled";
    /// A player picks an item (middle click). `item`. `Fail` cancels.
    pub const PICK_ITEM: &str = "fish:interaction/pick_item";
    /// A living entity is about to take damage. `entity_id`, `subject` = entity type, `amount`, `extra` = damage kind. `Fail` cancels.
    pub const ALLOW_DAMAGE: &str = "fish:entity/allow_damage";
    /// A living entity took damage. `entity_id`, `amount`, `extra`.
    pub const AFTER_DAMAGE: &str = "fish:entity/after_damage";
    /// A living entity is about to die from damage. `Fail` cancels (approximated, see docs).
    pub const ALLOW_DEATH: &str = "fish:entity/allow_death";
    /// A living entity died. `entity_id`, `subject`, `pos`.
    pub const AFTER_DEATH: &str = "fish:entity/after_death";
    /// A player joined a server. `actor`, `actor_uuid`.
    pub const PLAYER_JOIN: &str = "fish:entity/player_join";
    /// A player left a server. `actor`, `actor_uuid`.
    pub const PLAYER_LEAVE: &str = "fish:entity/player_leave";
    /// A player respawned. `actor`.
    pub const AFTER_RESPAWN: &str = "fish:entity/after_respawn";
    /// A player's data carried over a respawn. `actor`.
    pub const COPY_FROM: &str = "fish:entity/copy_from";
    /// A player tries to sleep in a bed. `pos`, `actor`. `Fail` cancels.
    pub const ALLOW_SLEEPING: &str = "fish:entity/allow_sleeping";
    /// A player fell asleep. `pos`, `actor`.
    pub const SLEEP_START: &str = "fish:entity/sleep_start";
    /// A player woke up. `actor`.
    pub const SLEEP_STOP: &str = "fish:entity/sleep_stop";
    /// A player's chat message reached a server. `actor`, `subject` = message. `Fail` drops it.
    pub const ALLOW_CHAT: &str = "fish:message/allow_chat";
    /// A player's chat message was broadcast. `actor`, `subject`.
    pub const CHAT: &str = "fish:message/chat";
    /// A server sends a game (system) message. `subject` = text. `Fail` drops it.
    pub const ALLOW_GAME: &str = "fish:message/allow_game";
    /// A server sent a game message. `subject`.
    pub const GAME: &str = "fish:message/game";
    /// A server is about to run a player's command. `actor`, `subject` = command. `Fail` blocks it.
    pub const ALLOW_COMMAND: &str = "fish:message/allow_command";
    /// A server ran a player's command. `actor`, `subject`.
    pub const COMMAND: &str = "fish:message/command";
    /// The client received a chat line. `subject` = plain text, `extra` = source (player/server/client). `Fail` hides it.
    pub const ALLOW_RECEIVE: &str = "fish:message/allow_receive";
    /// The client showed a received chat line. `subject`, `extra`.
    pub const RECEIVE: &str = "fish:message/receive";
    /// The player typed a chat line. `subject`. `Fail` cancels.
    pub const ALLOW_SEND: &str = "fish:message/allow_send";
    /// The player's chat line may be rewritten with `replace_text`. `subject`.
    pub const MODIFY_SEND: &str = "fish:message/modify_send";
    /// The player's chat line was sent. `subject`.
    pub const SEND: &str = "fish:message/send";
}

/// Every game event with its combine mode, in the order of `fish_hooks::Kind`.
pub const EVENTS: [(&str, u8); 44] = [
    (ids::CLIENT_STARTED, COMBINE_NOTIFY),
    (ids::CLIENT_STOPPING, COMBINE_NOTIFY),
    (ids::CLIENT_STOPPED, COMBINE_NOTIFY),
    (ids::SERVER_STARTING, COMBINE_NOTIFY),
    (ids::SERVER_STARTED, COMBINE_NOTIFY),
    (ids::SERVER_STOPPING, COMBINE_NOTIFY),
    (ids::SERVER_STOPPED, COMBINE_NOTIFY),
    (ids::WORLD_LOAD, COMBINE_NOTIFY),
    (ids::WORLD_UNLOAD, COMBINE_NOTIFY),
    (ids::CHUNK_LOAD, COMBINE_NOTIFY),
    (ids::CHUNK_UNLOAD, COMBINE_NOTIFY),
    (ids::RELOAD_START, COMBINE_NOTIFY),
    (ids::RELOAD_END, COMBINE_NOTIFY),
    (ids::USE_BLOCK, COMBINE_FIRST_NON_PASS),
    (ids::USE_ITEM, COMBINE_FIRST_NON_PASS),
    (ids::USE_ENTITY, COMBINE_FIRST_NON_PASS),
    (ids::ATTACK_BLOCK, COMBINE_FIRST_NON_PASS),
    (ids::ATTACK_ENTITY, COMBINE_FIRST_NON_PASS),
    (ids::BLOCK_BREAK_BEFORE, COMBINE_FIRST_NON_PASS),
    (ids::BLOCK_BREAK_AFTER, COMBINE_NOTIFY),
    (ids::BLOCK_BREAK_CANCELED, COMBINE_NOTIFY),
    (ids::PICK_ITEM, COMBINE_FIRST_NON_PASS),
    (ids::ALLOW_DAMAGE, COMBINE_FIRST_NON_PASS),
    (ids::AFTER_DAMAGE, COMBINE_NOTIFY),
    (ids::ALLOW_DEATH, COMBINE_FIRST_NON_PASS),
    (ids::AFTER_DEATH, COMBINE_NOTIFY),
    (ids::PLAYER_JOIN, COMBINE_NOTIFY),
    (ids::PLAYER_LEAVE, COMBINE_NOTIFY),
    (ids::AFTER_RESPAWN, COMBINE_NOTIFY),
    (ids::COPY_FROM, COMBINE_NOTIFY),
    (ids::ALLOW_SLEEPING, COMBINE_FIRST_NON_PASS),
    (ids::SLEEP_START, COMBINE_NOTIFY),
    (ids::SLEEP_STOP, COMBINE_NOTIFY),
    (ids::ALLOW_CHAT, COMBINE_FIRST_NON_PASS),
    (ids::CHAT, COMBINE_NOTIFY),
    (ids::ALLOW_GAME, COMBINE_FIRST_NON_PASS),
    (ids::GAME, COMBINE_NOTIFY),
    (ids::ALLOW_COMMAND, COMBINE_FIRST_NON_PASS),
    (ids::COMMAND, COMBINE_NOTIFY),
    (ids::ALLOW_RECEIVE, COMBINE_FIRST_NON_PASS),
    (ids::RECEIVE, COMBINE_NOTIFY),
    (ids::ALLOW_SEND, COMBINE_FIRST_NON_PASS),
    (ids::MODIFY_SEND, COMBINE_NOTIFY),
    (ids::SEND, COMBINE_NOTIFY),
];

/// [`GamePayload::hand`]: main hand.
pub const HAND_MAIN: u8 = 0;
/// [`GamePayload::hand`]: off hand.
pub const HAND_OFF: u8 = 1;
/// [`GamePayload::face`]: no face.
pub const FACE_NONE: u8 = 255;
/// [`GamePayload::face`] values 0..=5: down, up, north, south, west, east.
pub const FACE_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// Receives replacement text (see [`GamePayload::replace`]).
pub type ReplaceFn = unsafe extern "C" fn(user: *mut c_void, text: FishStr);

/// The one payload of all P6 events.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GamePayload {
    /// `size_of::<GamePayload>()`.
    pub size: u32,
    /// One of the `ROLE_*` values of [`crate::client`].
    pub role: u8,
    /// Nonzero for client-side predictions of an interaction.
    pub is_client: u8,
    /// `HAND_*`.
    pub hand: u8,
    /// A face index or `FACE_NONE`.
    pub face: u8,
    /// The game that raised the event; an invalid handle for detached events.
    pub view: FishHandle,
    /// Block position, or chunk coordinates for chunk events.
    pub pos: [i32; 3],
    /// Entity network id (0: none).
    pub entity_id: i64,
    /// Damage amount, or 0.
    pub amount: f32,
    /// Padding, zero.
    pub reserved: u32,
    /// UUID bytes of `actor` (big endian), zero when unknown.
    pub actor_uuid: [u8; 16],
    /// Name of the acting player.
    pub actor: FishStr,
    /// Dimension id.
    pub dimension: FishStr,
    /// Block id, entity type, command or message, depending on the event.
    pub subject: FishStr,
    /// Item id.
    pub item: FishStr,
    /// Damage kind, chat source or other qualifier.
    pub extra: FishStr,
    /// Where `replace_text` writes; null when the event cannot be rewritten.
    pub replace: Option<ReplaceFn>,
    /// Passed to `replace`.
    pub replace_user: *mut c_void,
}

impl GamePayload {
    /// An empty payload for `view` and `role`.
    pub fn new(role: u8, view: FishHandle) -> GamePayload {
        let none = FishStr::new("");
        GamePayload {
            size: size_of::<GamePayload>() as u32,
            role,
            is_client: 0,
            hand: HAND_MAIN,
            face: FACE_NONE,
            view,
            pos: [0; 3],
            entity_id: 0,
            amount: 0.0,
            reserved: 0,
            actor_uuid: [0; 16],
            actor: none,
            dimension: none,
            subject: none,
            item: none,
            extra: none,
            replace: None,
            replace_user: core::ptr::null_mut(),
        }
    }
}
