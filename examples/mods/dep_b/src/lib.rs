//! Example mod: provides an object entrypoint and logs when it initialises.

static API: u32 = 7;

fn early() {
    log::info!("dep_b early");
}

fn init() {
    log::info!("dep_b init");
}

fn api() -> &'static u32 {
    &API
}

fish_api::export_mod! {
    pre_launch: [early],
    main: [init],
    "fish:dep_b_api": object [api],
}
