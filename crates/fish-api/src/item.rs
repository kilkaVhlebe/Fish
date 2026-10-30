//! Items (Fabric's `Item` registration): a mod adds items that copy a vanilla *template* item and
//! differ from it by a stack size and a rarity; their models, textures and translations come from
//! the `assets/` of the mod's `.fish` archive. Creative tabs are in
//! [`item_group`](crate::item_group).
//!
//! Register while `pre_launch` or `main` runs (the same freeze as [`registry`](crate::registry)):
//!
//! ```no_run
//! use fish_api::ActionResult;
//! use fish_api::item::{ItemBehaviour, ItemSettings, Items, Rarity};
//!
//! # fn init() -> Result<(), fish_api::Error> {
//! let ruby = Items::register_with(
//!     "mymod:ruby",
//!     ItemSettings::copy("minecraft:diamond")
//!         .max_stack(16)
//!         .rarity(Rarity::Rare),
//!     ItemBehaviour::new()
//!         .on_use(|c| {
//!             log::info!("{} used the ruby", c.actor());
//!             ActionResult::Success
//!         })
//!         .append_tooltip(|_, lines| lines.add("§7A gem")),
//! )?;
//! log::info!("registered {}", ruby.id());
//! # Ok(()) }
//! ```
//!
//! The namespace of the identifier must be the id of the registering mod. A behaviour is called
//! with the [`ItemContext`] of the stack that raised the event. A panic in a callback stops the
//! mod, like a panic in any other listener.

use std::ffi::c_void;

use fish_abi::client::{ROLE_DEDICATED_SERVER, ROLE_INTEGRATED_SERVER};
use fish_abi::game::HAND_OFF;
use fish_abi::items::{
    HANDLER_PANICKED, ItemHandlerFn, ItemPayloadV1, ItemServiceV1, ItemSettingsV1, SERVICE_ITEM,
    event, rarity, set, slot,
};
use fish_abi::{ActionResult, FishStatus, FishStr};

use crate::client::{ClientHandle, Role, service};
use crate::error::{Error, check};
use crate::game::{BlockPos, Face, Hand};
use crate::host;

/// How an item's name is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rarity {
    /// White.
    Common,
    /// Yellow.
    Uncommon,
    /// Aqua.
    Rare,
    /// Light purple.
    Epic,
}

impl Rarity {
    fn as_u8(self) -> u8 {
        match self {
            Rarity::Common => rarity::COMMON,
            Rarity::Uncommon => rarity::UNCOMMON,
            Rarity::Rare => rarity::RARE,
            Rarity::Epic => rarity::EPIC,
        }
    }
}

/// The settings of an item that differ from its template's.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemSettings {
    template: String,
    max_stack: Option<u8>,
    rarity: Option<Rarity>,
}

impl ItemSettings {
    /// Settings that copy the vanilla item `template` (`minecraft:<path>`): its components
    /// (durability, food, remainder, use animation, ...) and every setting not changed below.
    pub fn copy(template: &str) -> ItemSettings {
        ItemSettings {
            template: template.to_string(),
            max_stack: None,
            rarity: None,
        }
    }

    /// The largest stack, 1 to 99.
    pub fn max_stack(mut self, max_stack: u8) -> Self {
        self.max_stack = Some(max_stack);
        self
    }

    /// The colour of the item's name.
    pub fn rarity(mut self, rarity: Rarity) -> Self {
        self.rarity = Some(rarity);
        self
    }

    /// The ABI form; borrows the strings of `self`.
    fn abi(&self) -> ItemSettingsV1 {
        let mut bits = 0;
        if self.max_stack.is_some() {
            bits |= set::MAX_STACK;
        }
        if self.rarity.is_some() {
            bits |= set::RARITY;
        }
        ItemSettingsV1 {
            size: size_of::<ItemSettingsV1>() as u32,
            set: bits,
            max_stack: self.max_stack.unwrap_or(0),
            rarity: self.rarity.map_or(0, Rarity::as_u8),
            reserved: [0; 2],
            template: FishStr::new(&self.template),
        }
    }
}

/// Where a stack sits in the player's inventory ([`ItemBehaviour::on_inventory_tick`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemSlot {
    /// Slot `0..36`: `0..9` is the hotbar, then the main inventory.
    Inventory(u8),
    /// Armor slot `0..4`: head, chest, legs, feet.
    Armor(u8),
    /// The off hand.
    OffHand,
}

/// Collects the lines [`ItemBehaviour::append_tooltip`] adds.
pub struct TooltipLines {
    add: fish_abi::items::AddLineFn,
    lines: *mut c_void,
}

impl TooltipLines {
    /// Adds a line below the vanilla ones; `§` colour codes work (`§7` is gray). The host keeps at
    /// most 16 lines per call.
    pub fn add(&mut self, line: &str) {
        // SAFETY: `add` and `lines` come from the host together and are valid for the callback;
        // the string outlives the call.
        unsafe { (self.add)(self.lines, FishStr::new(line)) };
    }
}

/// What an item callback is called with.
#[repr(transparent)]
pub struct ItemContext(ItemPayloadV1);

impl ItemContext {
    // SAFETY (string reads): the host passes valid UTF-8 that lives for the callback; the borrow is
    // tied to `&self`, which cannot outlive the callback.
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

    /// The item's id (`mymod:ruby`).
    pub fn item(&self) -> &str {
        Self::text(self.0.item)
    }

    /// How many items the stack holds.
    pub fn count(&self) -> u32 {
        self.0.count
    }

    /// The hand used (use, finish).
    pub fn hand(&self) -> Hand {
        if self.0.hand == HAND_OFF {
            Hand::Off
        } else {
            Hand::Main
        }
    }

    /// The face of the block that was clicked ([`ItemBehaviour::on_use_on_block`]), if any.
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

    /// The slot of the stack ([`ItemBehaviour::on_inventory_tick`]), else `None`.
    pub fn slot(&self) -> Option<ItemSlot> {
        let s = self.0.slot;
        if (0..slot::INVENTORY_COUNT).contains(&s) {
            Some(ItemSlot::Inventory(s as u8))
        } else if (slot::ARMOR_FIRST..slot::OFFHAND).contains(&s) {
            Some(ItemSlot::Armor((s - slot::ARMOR_FIRST) as u8))
        } else if s == slot::OFFHAND {
            Some(ItemSlot::OffHand)
        } else {
            None
        }
    }

    /// True when the stack is in the selected hotbar slot or the off hand
    /// ([`ItemBehaviour::on_inventory_tick`]).
    pub fn selected(&self) -> bool {
        self.0.selected != 0
    }

    /// The block clicked ([`ItemBehaviour::on_use_on_block`]), else the origin.
    pub fn pos(&self) -> BlockPos {
        let [x, y, z] = self.0.pos;
        BlockPos { x, y, z }
    }

    /// The block's id ([`ItemBehaviour::on_use_on_block`]) or the entity's type
    /// ([`ItemBehaviour::on_use_on_entity`]), else empty.
    pub fn target(&self) -> &str {
        Self::text(self.0.target)
    }

    /// The entity's network id ([`ItemBehaviour::on_use_on_entity`]), else zero.
    pub fn entity_id(&self) -> i64 {
        self.0.entity_id
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
}

type Action = Box<dyn Fn(&ItemContext) -> ActionResult + Send + Sync>;
type Notify = Box<dyn Fn(&ItemContext) + Send + Sync>;
type Tooltip = Box<dyn Fn(&ItemContext, &mut TooltipLines) + Send + Sync>;

/// What an item does: one optional callback per event.
#[derive(Default)]
pub struct ItemBehaviour {
    on_use: Option<Action>,
    on_use_on_block: Option<Action>,
    on_use_on_entity: Option<Action>,
    on_finish_using: Option<Notify>,
    on_inventory_tick: Option<Notify>,
    append_tooltip: Option<Tooltip>,
}

impl ItemBehaviour {
    /// A behaviour with no callbacks.
    pub fn new() -> Self {
        ItemBehaviour::default()
    }

    /// Called when a player uses the item in the air (or where the block and the entity left the
    /// click to the item). `Pass` leaves the click to the game, `Fail` cancels it, anything else
    /// ends it as handled.
    pub fn on_use<F>(mut self, f: F) -> Self
    where
        F: Fn(&ItemContext) -> ActionResult + Send + Sync + 'static,
    {
        self.on_use = Some(Box::new(f));
        self
    }

    /// Called when a player uses the item on a block, after the block's own use. Decides like
    /// [`on_use`](Self::on_use).
    pub fn on_use_on_block<F>(mut self, f: F) -> Self
    where
        F: Fn(&ItemContext) -> ActionResult + Send + Sync + 'static,
    {
        self.on_use_on_block = Some(Box::new(f));
        self
    }

    /// Called when a player uses the item on an entity. Decides like [`on_use`](Self::on_use).
    pub fn on_use_on_entity<F>(mut self, f: F) -> Self
    where
        F: Fn(&ItemContext) -> ActionResult + Send + Sync + 'static,
    {
        self.on_use_on_entity = Some(Box::new(f));
        self
    }

    /// Called when a player finished eating or drinking the item, before the stack shrinks.
    pub fn on_finish_using<F>(mut self, f: F) -> Self
    where
        F: Fn(&ItemContext) + Send + Sync + 'static,
    {
        self.on_finish_using = Some(Box::new(f));
        self
    }

    /// Called every tick for each stack of the item in the player's inventory. Keep it cheap.
    pub fn on_inventory_tick<F>(mut self, f: F) -> Self
    where
        F: Fn(&ItemContext) + Send + Sync + 'static,
    {
        self.on_inventory_tick = Some(Box::new(f));
        self
    }

    /// Called whenever the tooltip of a stack of the item is built (client only; keep it cheap).
    pub fn append_tooltip<F>(mut self, f: F) -> Self
    where
        F: Fn(&ItemContext, &mut TooltipLines) + Send + Sync + 'static,
    {
        self.append_tooltip = Some(Box::new(f));
        self
    }

    /// The `1 << event::*` bits of the callbacks that are set.
    fn mask(&self) -> u32 {
        let bits = [
            (self.on_use.is_some(), event::USE),
            (self.on_use_on_block.is_some(), event::USE_ON_BLOCK),
            (self.on_use_on_entity.is_some(), event::USE_ON_ENTITY),
            (self.on_finish_using.is_some(), event::FINISH_USING),
            (self.on_inventory_tick.is_some(), event::INVENTORY_TICK),
            (self.append_tooltip.is_some(), event::APPEND_TOOLTIP),
        ];
        bits.iter()
            .filter(|(on, _)| *on)
            .fold(0, |mask, (_, e)| mask | 1 << e)
    }

    /// Runs the callback of `event`; only the `use` events answer.
    fn run(&self, event: u8, ctx: &ItemContext) -> u8 {
        let pass = ActionResult::Pass.as_u8();
        let decide = |f: &Option<Action>| f.as_ref().map_or(pass, |f| f(ctx).as_u8());
        let notify = |f: &Option<Notify>| {
            if let Some(f) = f {
                f(ctx);
            }
            pass
        };
        match event {
            event::USE => decide(&self.on_use),
            event::USE_ON_BLOCK => decide(&self.on_use_on_block),
            event::USE_ON_ENTITY => decide(&self.on_use_on_entity),
            event::FINISH_USING => notify(&self.on_finish_using),
            event::INVENTORY_TICK => notify(&self.on_inventory_tick),
            event::APPEND_TOOLTIP => {
                if let (Some(f), Some(add)) = (&self.append_tooltip, ctx.0.add_line) {
                    f(
                        ctx,
                        &mut TooltipLines {
                            add,
                            lines: ctx.0.lines,
                        },
                    );
                }
                pass
            }
            _ => pass,
        }
    }
}

unsafe extern "C" fn handle(user: *mut c_void, event: u8, payload: *const ItemPayloadV1) -> u8 {
    // SAFETY: `user` is the `ItemBehaviour` `register_with` boxed; `ItemContext` is
    // `repr(transparent)` over the payload, which the host keeps valid for the call.
    let (behaviour, ctx) = unsafe {
        (
            &*user.cast::<ItemBehaviour>(),
            &*payload.cast::<ItemContext>(),
        )
    };
    crate::event::__catch("an item callback", || behaviour.run(event, ctx))
        .unwrap_or(HANDLER_PANICKED)
}

unsafe extern "C" fn drop_behaviour(user: *mut c_void) {
    // SAFETY: `user` was created by `Box::into_raw(Box::new(ItemBehaviour))` in `register_with`.
    drop(unsafe { Box::from_raw(user.cast::<ItemBehaviour>()) });
}

/// A registered item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item(String);

impl Item {
    /// The item's id (`mymod:ruby`).
    pub fn id(&self) -> &str {
        &self.0
    }
}

/// Registers items.
pub struct Items;

impl Items {
    /// Registers item `id` (`namespace:path`, the namespace is the mod's id) that only exists.
    pub fn register(id: &str, settings: ItemSettings) -> Result<Item, Error> {
        Items::register_with(id, settings, ItemBehaviour::new())
    }

    /// Registers item `id` whose events call `behaviour`.
    pub fn register_with(
        id: &str,
        settings: ItemSettings,
        behaviour: ItemBehaviour,
    ) -> Result<Item, Error> {
        let (svc, host_ptr) = service::<ItemServiceV1>(SERVICE_ITEM)?;
        let table = host::get()?;
        let abi = settings.abi();
        let mask = behaviour.mask();
        let (handler, user, drop_user) = if mask == 0 {
            (None, std::ptr::null_mut(), None)
        } else {
            (
                Some(handle as ItemHandlerFn),
                Box::into_raw(Box::new(behaviour)).cast::<c_void>(),
                Some(drop_behaviour as fish_abi::DropUserFn),
            )
        };
        // SAFETY: valid service table; the id and the settings outlive the call; ownership of
        // `user` moves to the host on every outcome: it calls `drop_behaviour` exactly once, on a
        // refusal at once and on success when the loader goes.
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
        if status == FishStatus::INVALID_ARGUMENT && crate::registry::is_frozen()? {
            return Err(Error::InvalidArgument("registries are frozen".to_string()));
        }
        check("registering an item", status)?;
        Ok(Item(id.to_string()))
    }
}
