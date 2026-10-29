//! Example mod: logs from its entrypoints and counts `hello:ping` events.

use std::sync::atomic::{AtomicU64, Ordering};

use fish_api::event::{Combine, Event, Payload};
use fish_api::{Error, FishLoader};

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

fn init() -> Result<(), Error> {
    log::info!("Hello from main");
    PING.register(|_| {
        PINGS.fetch_add(1, Ordering::SeqCst);
    })?;
    FishLoader::get().object_share().put("hello:pings", &PINGS)
}

fn init_client() {
    log::info!("Hello from client");
}

fish_api::export_mod! {
    main: [init],
    client: [init_client],
}
