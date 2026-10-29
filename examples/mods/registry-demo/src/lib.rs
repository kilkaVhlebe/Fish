//! Example mod: registers two entries, then tries to register after the registries froze.

use fish_api::Error;
use fish_api::event::lifecycle::server;
use fish_api::registry;

fn init() -> Result<(), Error> {
    let ruby = registry::register("fish:demo", "registry_demo:ruby")?;
    let again = registry::register("fish:demo", "registry_demo:ruby")?;
    let opal = registry::register("fish:demo", "registry_demo:opal")?;
    log::info!(
        "registered ruby={ruby} again={again} opal={opal} frozen={}",
        registry::is_frozen()?
    );
    server::STARTED.register(|_| {
        match registry::register("fish:demo", "registry_demo:late") {
            Ok(_) => log::error!("a late registration succeeded"),
            Err(e) => log::info!("late registration refused: {e}"),
        }
        log::info!(
            "raw ids: ruby={:?} late={:?} frozen={:?}",
            registry::raw_id("fish:demo", "registry_demo:ruby"),
            registry::raw_id("fish:demo", "registry_demo:late"),
            registry::is_frozen()
        );
    })?;
    Ok(())
}

fish_api::export_mod! {
    main: [init],
}
