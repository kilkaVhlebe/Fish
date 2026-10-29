//! Example mod: logs from its entrypoints and counts `hello:ping` events.

use std::sync::atomic::{AtomicU64, Ordering};

use fish_api::client::command::RAW_COMMAND;
use fish_api::client::hud::RENDER;
use fish_api::event::lifecycle::client::END_TICK;
use fish_api::event::{Combine, Event, Payload};
use fish_api::{ActionResult, Error, FishLoader};

/// Payload of `hello:ping`.
#[repr(C)]
pub struct Ping {
    pub size: u32,
    /// Nonzero makes the `panicky` example panic.
    pub flag: u32,
}

// SAFETY: repr(C) with a leading `size: u32`.
unsafe impl Payload for Ping {}

/// The custom event this mod defines.
pub static PING: Event<Ping> = Event::new("hello:ping", Combine::Notify);

static PINGS: AtomicU64 = AtomicU64::new(0);
static TICKS: AtomicU64 = AtomicU64::new(0);

fn init() -> Result<(), Error> {
    log::info!("Hello from main");
    PING.register(|_| {
        PINGS.fetch_add(1, Ordering::SeqCst);
    })?;
    END_TICK.register(|_| {
        if TICKS.fetch_add(1, Ordering::SeqCst) + 1 == 100 {
            log::info!("tick 100");
        }
    })?;
    RENDER.register(|ctx| {
        // Drawing fails only when the HUD handle is stale, which cannot happen in the callback.
        let _ = ctx
            .draw()
            .text("Hello from Fish", 4.0, 4.0, 0x00FF_FFFF, true);
    })?;
    RAW_COMMAND.register(|ctx| {
        if ctx.command() == "fish hello" {
            log::info!("Fish says hello");
            let _ = ctx.feedback("Fish says hello");
            ActionResult::Success
        } else {
            ActionResult::Pass
        }
    })?;
    let share = FishLoader::get().object_share();
    share.put("hello:ticks", &TICKS)?;
    share.put("hello:pings", &PINGS)
}

fn init_client() {
    log::info!("Hello from client");
}

fish_api::export_mod! {
    main: [init],
    client: [init_client],
}
