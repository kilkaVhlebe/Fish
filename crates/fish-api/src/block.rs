//! Blocks (Fabric's `Block` registration): a mod adds blocks that copy a vanilla *template* block
//! and differ from it by a few settings; their models, textures and translations come from the
//! `assets/` of the mod's `.fish` archive.
//!
//! Register while `pre_launch` or `main` runs (the same freeze as [`registry`](crate::registry)):
//!
//! ```no_run
//! use fish_api::ActionResult;
//! use fish_api::block::{BlockBehaviour, BlockSettings, Blocks};
//!
//! # fn init() -> Result<(), fish_api::Error> {
//! let ruby = Blocks::register_with(
//!     "mymod:ruby_block",
//!     BlockSettings::copy("minecraft:iron_block")
//!         .hardness(5.0)
//!         .requires_tool(true),
//!     BlockBehaviour::new().on_use(|c| {
//!         log::info!("{} used the ruby block at {}", c.actor(), c.pos());
//!         ActionResult::Success
//!     }),
//! )?;
//! // once the game built its block states (inside any event listener):
//! let states = ruby.state_range()?;
//! log::info!("ruby block states {}..{}", states.start, states.end);
//! # Ok(()) }
//! ```
//!
//! The namespace of the identifier must be the id of the registering mod. The block takes as many
//! states as its template has; a behaviour is called with the [`BlockContext`] of the block that
//! raised the event. A panic in a callback stops the mod, like a panic in any other listener.

use std::ffi::c_void;
use std::ops::Range;

use fish_abi::blocks::{
    BlockHandlerFn, BlockPayloadV1, BlockServiceV1, BlockSettingsV1, HANDLER_PANICKED,
    SERVICE_BLOCK, event, set,
};
use fish_abi::client::{ROLE_DEDICATED_SERVER, ROLE_INTEGRATED_SERVER};
use fish_abi::game::HAND_OFF;
use fish_abi::{ActionResult, FishStatus, FishStr};

use crate::client::{ClientHandle, Role, service};
use crate::error::{Error, check};
use crate::game::{BlockPos, Face, Hand};
use crate::host;

/// The settings of a block that differ from its template's.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockSettings {
    template: String,
    hardness: Option<f32>,
    resistance: Option<f32>,
    light: Option<u8>,
    requires_tool: Option<bool>,
    friction: Option<f32>,
    map_color: Option<u8>,
    sounds: Option<String>,
}

impl BlockSettings {
    /// Settings that copy the vanilla block `template` (`minecraft:<path>`): its behaviour class,
    /// state properties, shapes and every setting not changed below.
    pub fn copy(template: &str) -> BlockSettings {
        BlockSettings {
            template: template.to_string(),
            hardness: None,
            resistance: None,
            light: None,
            requires_tool: None,
            friction: None,
            map_color: None,
            sounds: None,
        }
    }

    /// Seconds of hand mining (`-1`: unbreakable).
    pub fn hardness(mut self, hardness: f32) -> Self {
        self.hardness = Some(hardness);
        self
    }

    /// Explosion resistance, 0 or more.
    pub fn resistance(mut self, resistance: f32) -> Self {
        self.resistance = Some(resistance);
        self
    }

    /// Light emitted, 0 to 15.
    pub fn light(mut self, light: u8) -> Self {
        self.light = Some(light);
        self
    }

    /// Whether the right tool is needed for drops.
    pub fn requires_tool(mut self, requires_tool: bool) -> Self {
        self.requires_tool = Some(requires_tool);
        self
    }

    /// Friction, 0 to 2.
    pub fn friction(mut self, friction: f32) -> Self {
        self.friction = Some(friction);
        self
    }

    /// A map colour id, 0 to 61.
    pub fn map_color(mut self, map_color: u8) -> Self {
        self.map_color = Some(map_color);
        self
    }

    /// The vanilla block (`minecraft:<path>`) whose sounds the block uses.
    pub fn sounds(mut self, sounds: &str) -> Self {
        self.sounds = Some(sounds.to_string());
        self
    }

    /// The ABI form; borrows the strings of `self`.
    fn abi(&self) -> BlockSettingsV1 {
        let mut bits = 0;
        let mut flag = |on: bool, bit: u32| {
            if on {
                bits |= bit;
            }
        };
        flag(self.hardness.is_some(), set::HARDNESS);
        flag(self.resistance.is_some(), set::RESISTANCE);
        flag(self.light.is_some(), set::LIGHT);
        flag(self.requires_tool.is_some(), set::REQUIRES_TOOL);
        flag(self.friction.is_some(), set::FRICTION);
        flag(self.map_color.is_some(), set::MAP_COLOR);
        flag(self.sounds.is_some(), set::SOUNDS);
        BlockSettingsV1 {
            size: size_of::<BlockSettingsV1>() as u32,
            set: bits,
            hardness: self.hardness.unwrap_or(0.0),
            resistance: self.resistance.unwrap_or(0.0),
            friction: self.friction.unwrap_or(0.0),
            light: self.light.unwrap_or(0),
            requires_tool: u8::from(self.requires_tool.unwrap_or(false)),
            map_color: self.map_color.unwrap_or(0),
            reserved: 0,
            template: FishStr::new(&self.template),
            sounds: FishStr::new(self.sounds.as_deref().unwrap_or("")),
        }
    }
}

/// What a block callback is called with.
#[repr(transparent)]
pub struct BlockContext(BlockPayloadV1);

impl BlockContext {
    // SAFETY (all string reads): the host passes valid UTF-8 that lives for the callback; the
    // borrow is tied to `&self`, which cannot outlive the callback.
    fn text(s: FishStr) -> &'static str {
        unsafe { s.as_str() }.unwrap_or("")
    }

    /// The game that raised the event; an invalid handle when none is running.
    pub fn client(&self) -> ClientHandle {
        ClientHandle::from_raw(self.0.view)
    }

    /// Which kind of game raised the event.
    pub fn role(&self) -> Role {
        match self.0.role {
            ROLE_INTEGRATED_SERVER => Role::IntegratedServer,
            ROLE_DEDICATED_SERVER => Role::DedicatedServer,
            _ => Role::Window,
        }
    }

    /// The block's position.
    pub fn pos(&self) -> BlockPos {
        let [x, y, z] = self.0.pos;
        BlockPos { x, y, z }
    }

    /// The neighbour that changed ([`BlockBehaviour::neighbor_changed`]), else the origin.
    pub fn neighbor_pos(&self) -> BlockPos {
        let [x, y, z] = self.0.other_pos;
        BlockPos { x, y, z }
    }

    /// The face that was clicked ([`BlockBehaviour::on_use`]), if any.
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

    /// The hand used.
    pub fn hand(&self) -> Hand {
        if self.0.hand == HAND_OFF {
            Hand::Off
        } else {
            Hand::Main
        }
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

    /// The block's id (`mymod:ruby_block`).
    pub fn block(&self) -> &str {
        Self::text(self.0.block)
    }

    /// The block state's id in the game.
    pub fn state_id(&self) -> u32 {
        self.0.state
    }

    /// The state's index among the block's own states.
    pub fn state_index(&self) -> u32 {
        self.0.state_index
    }

    /// The item in the acting hand (use, place), else empty.
    pub fn item(&self) -> &str {
        Self::text(self.0.item)
    }
}

type Action = Box<dyn Fn(&BlockContext) -> ActionResult + Send + Sync>;
type Notify = Box<dyn Fn(&BlockContext) + Send + Sync>;

/// What a block does: one optional callback per event.
#[derive(Default)]
pub struct BlockBehaviour {
    on_use: Option<Action>,
    on_place: Option<Notify>,
    on_broken: Option<Notify>,
    neighbor_changed: Option<Notify>,
    random_tick: Option<Notify>,
    scheduled_tick: Option<Notify>,
}

impl BlockBehaviour {
    /// A behaviour with no callbacks.
    pub fn new() -> Self {
        BlockBehaviour::default()
    }

    /// Called when a player uses the block. `Pass` leaves the click to the game, `Fail` cancels
    /// it, anything else ends it as handled.
    pub fn on_use<F>(mut self, f: F) -> Self
    where
        F: Fn(&BlockContext) -> ActionResult + Send + Sync + 'static,
    {
        self.on_use = Some(Box::new(f));
        self
    }

    /// Called after a player placed the block.
    pub fn on_place<F>(mut self, f: F) -> Self
    where
        F: Fn(&BlockContext) + Send + Sync + 'static,
    {
        self.on_place = Some(Box::new(f));
        self
    }

    /// Called after a player broke the block.
    pub fn on_broken<F>(mut self, f: F) -> Self
    where
        F: Fn(&BlockContext) + Send + Sync + 'static,
    {
        self.on_broken = Some(Box::new(f));
        self
    }

    /// Called when a neighbour of the block changed ([`BlockContext::neighbor_pos`]).
    pub fn neighbor_changed<F>(mut self, f: F) -> Self
    where
        F: Fn(&BlockContext) + Send + Sync + 'static,
    {
        self.neighbor_changed = Some(Box::new(f));
        self
    }

    /// Called when a random tick picks the block.
    pub fn random_tick<F>(mut self, f: F) -> Self
    where
        F: Fn(&BlockContext) + Send + Sync + 'static,
    {
        self.random_tick = Some(Box::new(f));
        self
    }

    /// Called when a scheduled tick of the block runs.
    pub fn scheduled_tick<F>(mut self, f: F) -> Self
    where
        F: Fn(&BlockContext) + Send + Sync + 'static,
    {
        self.scheduled_tick = Some(Box::new(f));
        self
    }

    /// The `1 << event::*` bits of the callbacks that are set.
    fn mask(&self) -> u32 {
        let bits = [
            (self.on_use.is_some(), event::USE),
            (self.on_place.is_some(), event::PLACE),
            (self.on_broken.is_some(), event::BROKEN),
            (self.neighbor_changed.is_some(), event::NEIGHBOR_CHANGED),
            (self.random_tick.is_some(), event::RANDOM_TICK),
            (self.scheduled_tick.is_some(), event::SCHEDULED_TICK),
        ];
        bits.iter()
            .filter(|(on, _)| *on)
            .fold(0, |mask, (_, e)| mask | 1 << e)
    }

    /// Runs the callback of `event`; only `use` answers.
    fn run(&self, event: u8, ctx: &BlockContext) -> u8 {
        let notify = |f: &Option<Notify>| {
            if let Some(f) = f {
                f(ctx);
            }
            ActionResult::Pass.as_u8()
        };
        match event {
            event::USE => self
                .on_use
                .as_ref()
                .map_or(ActionResult::Pass.as_u8(), |f| f(ctx).as_u8()),
            event::PLACE => notify(&self.on_place),
            event::BROKEN => notify(&self.on_broken),
            event::NEIGHBOR_CHANGED => notify(&self.neighbor_changed),
            event::RANDOM_TICK => notify(&self.random_tick),
            event::SCHEDULED_TICK => notify(&self.scheduled_tick),
            _ => ActionResult::Pass.as_u8(),
        }
    }
}

unsafe extern "C" fn handle(user: *mut c_void, event: u8, payload: *const BlockPayloadV1) -> u8 {
    // SAFETY: `user` is the `BlockBehaviour` `register_with` boxed; `BlockContext` is
    // `repr(transparent)` over the payload, which the host keeps valid for the call.
    let (behaviour, ctx) = unsafe {
        (
            &*user.cast::<BlockBehaviour>(),
            &*payload.cast::<BlockContext>(),
        )
    };
    crate::event::__catch("a block callback", || behaviour.run(event, ctx))
        .unwrap_or(HANDLER_PANICKED)
}

unsafe extern "C" fn drop_behaviour(user: *mut c_void) {
    // SAFETY: `user` was created by `Box::into_raw(Box::new(BlockBehaviour))` in `register_with`.
    drop(unsafe { Box::from_raw(user.cast::<BlockBehaviour>()) });
}

fn is_frozen() -> Result<bool, Error> {
    crate::registry::is_frozen()
}

/// A registered block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block(String);

impl Block {
    /// The block's id (`mymod:ruby_block`).
    pub fn id(&self) -> &str {
        &self.0
    }

    /// The state ids the game gave the block; `NOT_FOUND` until the game built its block states.
    pub fn state_range(&self) -> Result<Range<u32>, Error> {
        Blocks::state_range(&self.0)
    }
}

/// Registers blocks and looks up their states.
pub struct Blocks;

impl Blocks {
    /// Registers block `id` (`namespace:path`, the namespace is the mod's id) that only exists.
    pub fn register(id: &str, settings: BlockSettings) -> Result<Block, Error> {
        Blocks::register_with(id, settings, BlockBehaviour::new())
    }

    /// Registers block `id` whose events call `behaviour`.
    pub fn register_with(
        id: &str,
        settings: BlockSettings,
        behaviour: BlockBehaviour,
    ) -> Result<Block, Error> {
        let (svc, host_ptr) = service::<BlockServiceV1>(SERVICE_BLOCK)?;
        let table = host::get()?;
        let abi = settings.abi();
        let mask = behaviour.mask();
        let (handler, user, drop_user) = if mask == 0 {
            (None, std::ptr::null_mut(), None)
        } else {
            (
                Some(handle as BlockHandlerFn),
                Box::into_raw(Box::new(behaviour)).cast::<c_void>(),
                Some(drop_behaviour as fish_abi::DropUserFn),
            )
        };
        // SAFETY: valid service table; the id and the settings outlive the call; ownership of
        // `user` moves to the host, which calls `drop_behaviour` exactly once on success.
        let status = unsafe {
            (svc.register)(
                host_ptr,
                table.mod_token,
                FishStr::new(id),
                &abi,
                mask,
                handler,
                user,
                drop_user,
            )
        };
        if !status.is_ok() && !user.is_null() {
            // SAFETY: the host did not take ownership.
            unsafe { drop_behaviour(user) };
        }
        if status == FishStatus::INVALID_ARGUMENT && is_frozen()? {
            return Err(Error::InvalidArgument("registries are frozen".to_string()));
        }
        check("registering a block", status)?;
        Ok(Block(id.to_string()))
    }

    /// The state ids the game gave block `id`; `NOT_FOUND` for an unknown block and until the
    /// game built its block states.
    pub fn state_range(id: &str) -> Result<Range<u32>, Error> {
        let (svc, host_ptr) = service::<BlockServiceV1>(SERVICE_BLOCK)?;
        let (mut first, mut count) = (0u32, 0u32);
        // SAFETY: valid service table; the id and both outputs outlive the call.
        let status =
            unsafe { (svc.state_range)(host_ptr, FishStr::new(id), &mut first, &mut count) };
        check("reading block states", status)?;
        Ok(first..first.saturating_add(count))
    }
}
