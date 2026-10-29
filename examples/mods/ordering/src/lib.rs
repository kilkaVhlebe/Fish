//! Example mod: phase ordering, a phase cycle, and first-non-pass short-circuiting.
//!
//! Listeners append tags to a shared trace, published as `ordering:trace`.

use std::sync::Mutex;

use fish_api::event::{Combine, Event, Payload, Phase};
use fish_api::{ActionResult, Error, FishLoader};

#[repr(C)]
struct Tick {
    size: u32,
}

// SAFETY: repr(C) with a leading `size: u32`.
unsafe impl Payload for Tick {}

static TRACE: Mutex<String> = Mutex::new(String::new());

static ORDER: Event<Tick> = Event::new("ordering:phases", Combine::Notify);
static CYCLE: Event<Tick> = Event::new("ordering:cycle", Combine::Notify);
static FIRST: Event<Tick, ActionResult> = Event::new("ordering:first", Combine::FirstNonPass);

fn push(tag: &str) {
    let mut t = TRACE.lock().unwrap();
    t.push_str(tag);
    t.push(' ');
}

fn early() {
    log::info!("ordering early");
}

fn init() -> Result<(), Error> {
    ORDER.add_phase_ordering(Phase("ordering:b"), Phase::DEFAULT)?;
    ORDER.add_phase_ordering(Phase("ordering:a"), Phase("ordering:b"))?;
    ORDER.register_phase(Phase("ordering:b"), |_| push("b1"))?;
    ORDER.register(|_| push("d1"))?;
    ORDER.register_phase(Phase("ordering:a"), |_| push("a1"))?;
    ORDER.register_phase(Phase("ordering:b"), |_| push("b2"))?;

    CYCLE.add_phase_ordering(Phase("ordering:x"), Phase("ordering:y"))?;
    CYCLE.add_phase_ordering(Phase("ordering:y"), Phase("ordering:x"))?;
    CYCLE.register_phase(Phase("ordering:y"), |_| push("y1"))?;
    CYCLE.register_phase(Phase("ordering:x"), |_| push("x1"))?;
    CYCLE.register(|_| push("d1"))?;

    FIRST.register(|_| {
        push("p");
        ActionResult::Pass
    })?;
    FIRST.register(|_| {
        push("c");
        ActionResult::Consume
    })?;
    FIRST.register(|_| {
        push("f");
        ActionResult::Fail
    })?;

    FishLoader::get()
        .object_share()
        .put("ordering:trace", &TRACE)
}

fish_api::export_mod! {
    pre_launch: [early],
    main: [init],
}
