//! Behaviour when no host is attached (this test binary never attaches one).

use fish_api::event::{Combine, Event, Payload};
use fish_api::{Error, FishLoader};

#[repr(C)]
struct P {
    size: u32,
}
// SAFETY: repr(C) with a leading `size: u32`.
unsafe impl Payload for P {}

#[test]
fn calls_fail_cleanly_without_a_host() {
    static EV: Event<P> = Event::new("none:event", Combine::Notify);
    assert_eq!(EV.define(), Err(Error::NoHost));
    assert_eq!(EV.register(|_| {}), Err(Error::NoHost));
    assert!(matches!(FishLoader::try_get(), Err(Error::NoHost)));
    assert!(Error::NoHost.to_string().contains("no Fish host"));
    assert_eq!(Error::NoHost.status(), None);
    assert!(
        Error::Unsupported("x")
            .to_string()
            .contains("does not support x")
    );
}

#[test]
#[should_panic(expected = "FishLoader::get()")]
fn get_panics_without_a_host() {
    let _ = FishLoader::get();
}
