//! Example mod: two blocks.
//!
//! * `content_demo:ruby_block` copies iron, glows a little, and logs every event the game raises
//!   for it; using it is handled (the click ends there),
//! * `content_demo:raw_ruby` copies stone and has no behaviour,
//! * the server logs the state ids of the ruby block when it starts and tries to register one more
//!   block (refused: the registries are frozen).
//!
//! The block models, textures and translations are the mod's own `assets/` and `data/`; the loader
//! hands them to Horizon together with the block declarations.

use fish_api::block::{BlockBehaviour, BlockSettings, Blocks};
use fish_api::event::lifecycle;
use fish_api::{ActionResult, Error};

fn init() -> Result<(), Error> {
    let behaviour = BlockBehaviour::new()
        .on_use(|c| {
            log::info!(
                "ruby block used at {} by {} face {:?}",
                c.pos(),
                c.actor(),
                c.face()
            );
            ActionResult::Success
        })
        .on_place(|c| log::info!("ruby block placed at {}", c.pos()))
        .on_broken(|c| log::info!("ruby block broken at {}", c.pos()))
        .neighbor_changed(|c| {
            log::info!(
                "ruby block neighbor changed at {} from {}",
                c.pos(),
                c.neighbor_pos()
            );
        })
        .random_tick(|c| log::info!("ruby block random tick at {}", c.pos()))
        .scheduled_tick(|c| log::info!("ruby block scheduled tick at {}", c.pos()));
    Blocks::register_with(
        "content_demo:ruby_block",
        BlockSettings::copy("minecraft:iron_block")
            .hardness(5.0)
            .resistance(6.0)
            .requires_tool(true)
            .light(3)
            .sounds("minecraft:amethyst_block"),
        behaviour,
    )?;
    Blocks::register(
        "content_demo:raw_ruby",
        BlockSettings::copy("minecraft:stone").hardness(1.5),
    )?;

    lifecycle::server::STARTED.register(|_| {
        match Blocks::state_range("content_demo:ruby_block") {
            Ok(r) => log::info!("ruby states {}..{}", r.start, r.end),
            Err(e) => log::error!("ruby states unknown: {e}"),
        }
        match Blocks::register("content_demo:late", BlockSettings::copy("minecraft:stone")) {
            Ok(_) => log::error!("a late block was registered"),
            Err(e) => log::info!("late block refused: {e}"),
        }
    })?;
    Ok(())
}

fish_api::export_mod! {
    main: [init],
}
