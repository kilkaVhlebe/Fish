//! Example mod: two blocks, two items and a creative tab.
//!
//! * `content_demo:ruby_block` copies iron, glows a little, and logs every event the game raises
//!   for it; using it is handled (the click ends there),
//! * `content_demo:raw_ruby` copies stone and has no behaviour,
//! * `content_demo:ruby` (a diamond that stacks to 16) logs its uses, once its place in the
//!   inventory, and adds a tooltip line,
//! * `content_demo:ruby_snack` (an apple) logs who ate it,
//! * the tab `content_demo:ruby_tab` lists the items and blocks above, and the ruby is also added
//!   to vanilla's `minecraft:ingredients`,
//! * the server logs the state ids of the ruby block when it starts and tries to register one more
//!   block and one more item (refused: the registries are frozen).
//!
//! The block and item models, textures and translations are the mod's own `assets/` and `data/`;
//! the loader hands them to Horizon together with the declarations.

use std::sync::atomic::{AtomicBool, Ordering};

use fish_api::block::{BlockBehaviour, BlockSettings, Blocks};
use fish_api::event::lifecycle;
use fish_api::item::{ItemBehaviour, ItemSettings, Items, Rarity};
use fish_api::item_group::{ItemGroup, ItemGroups};
use fish_api::{ActionResult, Error};

/// Logs the inventory tick of the ruby once, so the log stays short.
static TICK_LOGGED: AtomicBool = AtomicBool::new(false);

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

    let ruby = ItemBehaviour::new()
        .on_use(|c| {
            log::info!("ruby used by {} hand {:?}", c.actor(), c.hand());
            ActionResult::Success
        })
        .on_use_on_block(|c| {
            log::info!(
                "ruby used on {} at {} face {:?}",
                c.target(),
                c.pos(),
                c.face()
            );
            ActionResult::Success
        })
        .on_inventory_tick(|c| {
            if !TICK_LOGGED.swap(true, Ordering::Relaxed) {
                log::info!(
                    "ruby is in the inventory slot {:?} selected {}",
                    c.slot(),
                    c.selected()
                );
            }
        })
        .append_tooltip(|_, lines| lines.add("§7A gem of content_demo"));
    Items::register_with(
        "content_demo:ruby",
        ItemSettings::copy("minecraft:diamond")
            .max_stack(16)
            .rarity(Rarity::Rare),
        ruby,
    )?;
    Items::register_with(
        "content_demo:ruby_snack",
        ItemSettings::copy("minecraft:apple"),
        ItemBehaviour::new().on_finish_using(|c| log::info!("ruby snack eaten by {}", c.actor())),
    )?;
    ItemGroup::builder("content_demo:ruby_tab")
        .title("Ruby")
        .icon("content_demo:ruby")
        .entries([
            "content_demo:ruby",
            "content_demo:ruby_snack",
            "content_demo:ruby_block",
            "content_demo:raw_ruby",
        ])
        .register()?;
    ItemGroups::modify_entries("minecraft:ingredients", ["content_demo:ruby"])?;

    lifecycle::server::STARTED.register(|_| {
        match Blocks::state_range("content_demo:ruby_block") {
            Ok(r) => log::info!("ruby states {}..{}", r.start, r.end),
            Err(e) => log::error!("ruby states unknown: {e}"),
        }
        match Blocks::register("content_demo:late", BlockSettings::copy("minecraft:stone")) {
            Ok(_) => log::error!("a late block was registered"),
            Err(e) => log::info!("late block refused: {e}"),
        }
        match Items::register("content_demo:late", ItemSettings::copy("minecraft:stick")) {
            Ok(_) => log::error!("a late item was registered"),
            Err(e) => log::info!("late item refused: {e}"),
        }
    })?;
    Ok(())
}

fish_api::export_mod! {
    main: [init],
}
