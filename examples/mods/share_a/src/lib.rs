//! Example mod: publishes `share:value` in the object share.

use std::sync::atomic::AtomicU64;

use fish_api::{Error, FishLoader};

static VALUE: AtomicU64 = AtomicU64::new(41);

fn init() -> Result<(), Error> {
    FishLoader::get().object_share().put("share:value", &VALUE)
}

fish_api::export_mod! {
    main: [init],
}
