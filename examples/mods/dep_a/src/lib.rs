//! Example mod: depends on `dep_b` and reads its object entrypoint.

use fish_api::FishLoader;

fn init() {
    log::info!("dep_a init");
    for (id, api) in FishLoader::get().entrypoint_objects::<u32>("fish:dep_b_api") {
        log::info!("dep_a sees {id} api = {api}");
    }
}

fish_api::export_mod! {
    main: [init],
}
