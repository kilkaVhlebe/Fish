//! Example mod: blocks whose use ends in every `ActionResult`, one whose handler panics, and a
//! listener that tells when the game's own use-block flow ran.
//!
//! * `block_edges:consumer` (use is consumed), `block_edges:refuser` (use fails),
//!   `block_edges:passer` (use passes) log `<name> used`,
//! * `block_edges:panicky` panics when used,
//! * `block_edges:ticker` only listens to random ticks.

use fish_api::block::{BlockBehaviour, BlockSettings, Blocks};
use fish_api::event::interaction;
use fish_api::{ActionResult, Error};

fn stone() -> BlockSettings {
    BlockSettings::copy("minecraft:stone")
}

fn init() -> Result<(), Error> {
    Blocks::register_with(
        "block_edges:consumer",
        stone(),
        BlockBehaviour::new().on_use(|_| {
            log::info!("consumer used");
            ActionResult::Consume
        }),
    )?;
    Blocks::register_with(
        "block_edges:refuser",
        stone(),
        BlockBehaviour::new().on_use(|_| {
            log::info!("refuser used");
            ActionResult::Fail
        }),
    )?;
    Blocks::register_with(
        "block_edges:passer",
        stone(),
        BlockBehaviour::new().on_use(|_| {
            log::info!("passer used");
            ActionResult::Pass
        }),
    )?;
    Blocks::register_with(
        "block_edges:panicky",
        stone(),
        BlockBehaviour::new().on_use(|_| panic!("block_edges:panicky was used")),
    )?;
    Blocks::register_with(
        "block_edges:ticker",
        stone(),
        BlockBehaviour::new().random_tick(|c| log::info!("ticker ticked at {}", c.pos())),
    )?;
    interaction::USE_BLOCK.register(|c| {
        log::info!("game use_block at {} on {}", c.pos(), c.subject());
        ActionResult::Pass
    })?;
    Ok(())
}

fish_api::export_mod! {
    main: [init],
}
