//! The context of game events (phase P6): who did what, where.
//!
//! One [`GameContext`] type serves every event of `event::{lifecycle, interaction, entity, message}`;
//! the documentation of each event names the accessors it fills. The rest return zero or empty.

use std::fmt;

use fish_abi::client::{ROLE_DEDICATED_SERVER, ROLE_INTEGRATED_SERVER};
use fish_abi::game::{GamePayload, HAND_OFF};

use crate::client::{ClientHandle, Role};
use crate::event::Payload;

/// A hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    /// The main hand.
    Main,
    /// The off hand.
    Off,
}

/// A block face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// Bottom (-y).
    Down,
    /// Top (+y).
    Up,
    /// -z.
    North,
    /// +z.
    South,
    /// -x.
    West,
    /// +x.
    East,
}

impl Face {
    /// The lowercase name (`north`).
    pub fn name(self) -> &'static str {
        fish_abi::game::FACE_NAMES[self as usize]
    }
}

/// A block position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockPos {
    /// X.
    pub x: i32,
    /// Y.
    pub y: i32,
    /// Z.
    pub z: i32,
}

impl fmt::Display for BlockPos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.x, self.y, self.z)
    }
}

/// The payload of every P6 event.
#[repr(transparent)]
pub struct GameContext(GamePayload);

// SAFETY: `repr(transparent)` over an ABI payload that starts with `size: u32`.
unsafe impl Payload for GameContext {}

impl GameContext {
    // SAFETY (all string reads): the host passes valid UTF-8 that lives for the callback; the
    // borrow is tied to `&self`, which cannot outlive the callback.
    fn text(s: fish_abi::FishStr) -> &'static str {
        unsafe { s.as_str() }.unwrap_or("")
    }

    /// The game that raised the event. Detached events (chunks, client stopped) have an invalid
    /// handle: using it returns an error.
    pub fn client(&self) -> ClientHandle {
        ClientHandle::from_raw(self.0.view)
    }

    /// Which kind of game raised it.
    pub fn role(&self) -> Role {
        match self.0.role {
            ROLE_INTEGRATED_SERVER => Role::IntegratedServer,
            ROLE_DEDICATED_SERVER => Role::DedicatedServer,
            _ => Role::Window,
        }
    }

    /// True when this is the client's prediction of something the server decides.
    pub fn is_client(&self) -> bool {
        self.0.is_client != 0
    }

    /// The hand used.
    pub fn hand(&self) -> Hand {
        if self.0.hand == HAND_OFF {
            Hand::Off
        } else {
            Hand::Main
        }
    }

    /// The block face involved, if any.
    pub fn face(&self) -> Option<Face> {
        Some(match self.0.face {
            0 => Face::Down,
            1 => Face::Up,
            2 => Face::North,
            3 => Face::South,
            4 => Face::West,
            5 => Face::East,
            _ => return None,
        })
    }

    /// The block position, or `x`/`z` chunk coordinates for chunk events.
    pub fn pos(&self) -> BlockPos {
        let [x, y, z] = self.0.pos;
        BlockPos { x, y, z }
    }

    /// The entity's network id (0: none).
    pub fn entity_id(&self) -> i64 {
        self.0.entity_id
    }

    /// The damage amount.
    pub fn amount(&self) -> f32 {
        self.0.amount
    }

    /// The acting player's name (empty: none).
    pub fn actor(&self) -> &str {
        Self::text(self.0.actor)
    }

    /// The acting player's UUID, if known.
    pub fn actor_uuid(&self) -> Option<u128> {
        let n = u128::from_be_bytes(self.0.actor_uuid);
        (n != 0).then_some(n)
    }

    /// The dimension id (`minecraft:overworld`).
    pub fn dimension(&self) -> &str {
        Self::text(self.0.dimension)
    }

    /// The block id, entity type, command or message, depending on the event.
    pub fn subject(&self) -> &str {
        Self::text(self.0.subject)
    }

    /// The item id.
    pub fn item(&self) -> &str {
        Self::text(self.0.item)
    }

    /// The damage kind, chat source or another qualifier.
    pub fn extra(&self) -> &str {
        Self::text(self.0.extra)
    }

    /// Rewrites the text of an event that allows it (`message::client::MODIFY_SEND`); returns false
    /// when the event cannot be rewritten.
    pub fn replace_text(&mut self, text: &str) -> bool {
        match self.0.replace {
            Some(f) => {
                // SAFETY: `replace` and `replace_user` were set together by the host for this call.
                unsafe { f(self.0.replace_user, fish_abi::FishStr::new(text)) };
                true
            }
            None => false,
        }
    }
}
