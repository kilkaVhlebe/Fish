//! Example mod: reads and bumps `share:value`, and checks the type tag is enforced.

use std::sync::atomic::{AtomicU64, Ordering};

use fish_api::{Error, FishLoader};

fn init() -> Result<(), Error> {
    let share = FishLoader::get().object_share();
    let value = share.get::<AtomicU64>("share:value")?;
    value.fetch_add(1, Ordering::SeqCst);
    match share.get::<String>("share:value") {
        Err(e) if e.status() == Some(fish_api::abi::FishStatus::TYPE_MISMATCH) => {
            log::info!("share_b: type mismatch detected");
        }
        other => log::error!("share_b: expected a type mismatch, got {other:?}"),
    }
    Ok(())
}

fish_api::export_mod! {
    main: [init],
}
