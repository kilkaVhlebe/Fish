//! The `fish:game_rule` service (phase P8): game rules registered by mods.
//!
//! A rule has a name (`namespace:path`), a category, a type (boolean, integer, decimal or one of
//! named choices), bounds and a default. Rules are registered while the `pre_launch` and `main`
//! entrypoints run (the same freeze rule as the registries); every world then keeps its own
//! values, which the game saves beside its own rules and the `/gamerule` command and the rule
//! screen read and write. `get` and `set` act on the world of the calling thread's game: the one
//! whose event is running. Outside of an event there is no world: `get` answers the default and
//! `set` fails with `UNSUPPORTED`.

use core::ffi::c_void;

use crate::client::StrSink;
use crate::tables::FishServiceHeader;
use crate::types::{FishStatus, FishStr};

/// Service id.
pub const SERVICE_GAME_RULE: &str = "fish:game_rule";

/// Event id (`Notify`): a rule's value in some world changed; the payload is [`GameRuleChangedPayload`].
pub const CHANGED: &str = "fish:game_rule/changed";

/// [`RuleValueV1::kind`] and [`RuleDefV1::kind`] values.
pub mod kind {
    /// `true` or `false`.
    pub const BOOL: u8 = 0;
    /// A 32-bit integer.
    pub const INT: u8 = 1;
    /// A 64-bit float.
    pub const DOUBLE: u8 = 2;
    /// One of the rule's named choices.
    pub const ENUM: u8 = 3;
}

/// A rule value. Only the field `kind` names is meaningful.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuleValueV1 {
    /// A `kind::*` value.
    pub kind: u8,
    /// Nonzero for `true` (`kind::BOOL`).
    pub boolean: u8,
    /// Padding, zero.
    pub reserved: [u8; 2],
    /// The value of `kind::INT`.
    pub int: i32,
    /// The value of `kind::DOUBLE`.
    pub double: f64,
    /// The choice of `kind::ENUM`, borrowed for the call it is passed to.
    pub text: FishStr,
}

impl RuleValueV1 {
    /// A boolean value.
    pub const fn boolean(value: bool) -> RuleValueV1 {
        RuleValueV1 {
            kind: kind::BOOL,
            boolean: value as u8,
            reserved: [0; 2],
            int: 0,
            double: 0.0,
            text: FishStr::EMPTY,
        }
    }

    /// An integer value.
    pub const fn int(value: i32) -> RuleValueV1 {
        RuleValueV1 {
            kind: kind::INT,
            boolean: 0,
            reserved: [0; 2],
            int: value,
            double: 0.0,
            text: FishStr::EMPTY,
        }
    }

    /// A decimal value.
    pub const fn double(value: f64) -> RuleValueV1 {
        RuleValueV1 {
            kind: kind::DOUBLE,
            boolean: 0,
            reserved: [0; 2],
            int: 0,
            double: value,
            text: FishStr::EMPTY,
        }
    }

    /// A choice, borrowed from `value` for the call.
    pub const fn choice(value: &str) -> RuleValueV1 {
        RuleValueV1 {
            kind: kind::ENUM,
            boolean: 0,
            reserved: [0; 2],
            int: 0,
            double: 0.0,
            text: FishStr::new(value),
        }
    }
}

/// What a rule is: its type, bounds and default.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuleDefV1 {
    /// `size_of::<RuleDefV1>()`.
    pub size: u32,
    /// A `kind::*` value.
    pub kind: u8,
    /// Padding, zero.
    pub reserved: [u8; 3],
    /// The value of a world that never set the rule; its `kind` equals [`RuleDefV1::kind`].
    pub default_value: RuleValueV1,
    /// Smallest value of `kind::INT`.
    pub min_int: i32,
    /// Largest value of `kind::INT`.
    pub max_int: i32,
    /// Smallest value of `kind::DOUBLE`.
    pub min_double: f64,
    /// Largest value of `kind::DOUBLE`.
    pub max_double: f64,
    /// The choices of `kind::ENUM`.
    pub choices: *const FishStr,
    /// Number of choices.
    pub choice_count: usize,
}

/// Payload of [`CHANGED`]. The strings and values are borrowed for the callback.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GameRuleChangedPayload {
    /// `size_of::<GameRuleChangedPayload>()`.
    pub size: u32,
    /// Padding, zero.
    pub reserved: u32,
    /// The rule.
    pub name: FishStr,
    /// The world (its directory), empty when unknown.
    pub world: FishStr,
    /// The value before.
    pub old: RuleValueV1,
    /// The value now.
    pub new_value: RuleValueV1,
}

/// `fish:game_rule` service, version 1.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GameRuleServiceV1 {
    /// Table header.
    pub header: FishServiceHeader,
    /// Registers rule `name` in `category` for mod `token`. `INVALID_ARGUMENT` for a bad name,
    /// category, type, bound or default and after the registries froze; `ALREADY_EXISTS` when a
    /// rule of that name exists.
    pub register: unsafe extern "C" fn(
        host: *mut c_void,
        token: u64,
        name: FishStr,
        category: FishStr,
        def: *const RuleDefV1,
    ) -> FishStatus,
    /// Reads the current value of `name` in the calling thread's world (the default when there is
    /// none). The text of an enum value goes to `text`, not to `out`. `NOT_FOUND` for an unknown rule.
    pub get: unsafe extern "C" fn(
        host: *mut c_void,
        name: FishStr,
        out: *mut RuleValueV1,
        text: StrSink,
        user: *mut c_void,
    ) -> FishStatus,
    /// Sets `name` in the calling thread's world. `NOT_FOUND` for an unknown rule, `UNSUPPORTED`
    /// without a world, `TYPE_MISMATCH` for a value of another type, `INVALID_ARGUMENT` for a
    /// value out of bounds.
    pub set: unsafe extern "C" fn(
        host: *mut c_void,
        name: FishStr,
        value: *const RuleValueV1,
    ) -> FishStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn layouts_are_fixed() {
        assert_eq!(size_of::<RuleValueV1>(), 32);
        assert_eq!(offset_of!(RuleValueV1, int), 4);
        assert_eq!(offset_of!(RuleValueV1, double), 8);
        assert_eq!(offset_of!(RuleValueV1, text), 16);
        assert_eq!(size_of::<RuleDefV1>(), 80);
        assert_eq!(offset_of!(RuleDefV1, default_value), 8);
        assert_eq!(offset_of!(RuleDefV1, min_int), 40);
        assert_eq!(offset_of!(RuleDefV1, min_double), 48);
        assert_eq!(offset_of!(RuleDefV1, choices), 64);
        assert_eq!(offset_of!(RuleDefV1, choice_count), 72);
        assert_eq!(size_of::<GameRuleChangedPayload>(), 104);
        assert_eq!(offset_of!(GameRuleChangedPayload, name), 8);
        assert_eq!(offset_of!(GameRuleChangedPayload, world), 24);
        assert_eq!(offset_of!(GameRuleChangedPayload, old), 40);
        assert_eq!(offset_of!(GameRuleChangedPayload, new_value), 72);
    }

    #[test]
    fn value_constructors_set_their_kind() {
        assert_eq!(RuleValueV1::boolean(true).boolean, 1);
        assert_eq!(RuleValueV1::boolean(false).kind, kind::BOOL);
        assert_eq!(RuleValueV1::int(-3).int, -3);
        assert_eq!(RuleValueV1::double(1.5).kind, kind::DOUBLE);
        let v = RuleValueV1::choice("hard");
        assert_eq!(v.kind, kind::ENUM);
        assert_eq!(v.text.len, 4);
    }
}
