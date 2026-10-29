//! Example mod: a listener on `hello:ping` that panics when the payload flag is set.

use fish_api::Error;
use fish_api::event::{Combine, Event, Payload};

#[repr(C)]
struct Ping {
    size: u32,
    flag: u32,
}

// SAFETY: repr(C) with a leading `size: u32`.
unsafe impl Payload for Ping {}

static PING: Event<Ping> = Event::new("hello:ping", Combine::Notify);

fn init() -> Result<(), Error> {
    PING.register(|p| {
        if p.flag != 0 {
            panic!("panicky was asked to panic");
        }
    })
}

fish_api::export_mod! {
    main: [init],
}
