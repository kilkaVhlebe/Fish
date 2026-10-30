//! Example mod: items whose use ends in every `ActionResult`, one whose handler panics, items that
//! tick, show tooltips, act on entities and on eating, and a creative tab with entries.
//!
//! * `item_edges:consumer` (use is consumed), `item_edges:refuser` (use fails),
//!   `item_edges:passer` (use passes) log `<name> used`,
//! * `item_edges:panicky` panics when used,
//! * `item_edges:ticker` logs the slot it sits in on every inventory tick,
//! * `item_edges:tipper` adds two tooltip lines,
//! * `item_edges:entity_user` logs the entity it is used on and succeeds,
//! * `item_edges:finisher` logs when it was eaten,
//! * `item_edges:stacked` only changes its settings: a stack of one, epic rarity.
//!
//! The tab `item_edges:tab` lists them, and `minecraft:building_blocks` gets `consumer`.

use fish_api::item::{ItemBehaviour, ItemSettings, Items, Rarity};
use fish_api::item_group::{ItemGroup, ItemGroups};
use fish_api::{ActionResult, Error};

fn stick() -> ItemSettings {
    ItemSettings::copy("minecraft:stick")
}

fn init() -> Result<(), Error> {
    Items::register_with(
        "item_edges:consumer",
        stick(),
        ItemBehaviour::new().on_use(|_| {
            log::info!("consumer used");
            ActionResult::Consume
        }),
    )?;
    Items::register_with(
        "item_edges:refuser",
        stick(),
        ItemBehaviour::new().on_use(|_| {
            log::info!("refuser used");
            ActionResult::Fail
        }),
    )?;
    Items::register_with(
        "item_edges:passer",
        stick(),
        ItemBehaviour::new().on_use(|_| {
            log::info!("passer used");
            ActionResult::Pass
        }),
    )?;
    Items::register_with(
        "item_edges:panicky",
        stick(),
        ItemBehaviour::new().on_use(|_| panic!("item_edges:panicky was used")),
    )?;
    Items::register_with(
        "item_edges:ticker",
        stick(),
        ItemBehaviour::new().on_inventory_tick(|c| {
            log::info!(
                "ticker in slot {:?} selected {} count {}",
                c.slot(),
                c.selected(),
                c.count()
            );
        }),
    )?;
    Items::register_with(
        "item_edges:tipper",
        stick(),
        ItemBehaviour::new().append_tooltip(|_, lines| {
            lines.add("§6first line");
            lines.add("second line");
        }),
    )?;
    Items::register_with(
        "item_edges:entity_user",
        stick(),
        ItemBehaviour::new().on_use_on_entity(|c| {
            log::info!(
                "entity_user used on {} ({}) by {}",
                c.entity_id(),
                c.target(),
                c.actor()
            );
            ActionResult::Success
        }),
    )?;
    Items::register_with(
        "item_edges:finisher",
        ItemSettings::copy("minecraft:apple"),
        ItemBehaviour::new().on_finish_using(|c| {
            log::info!("finisher eaten by {}", c.actor());
        }),
    )?;
    Items::register(
        "item_edges:stacked",
        stick().max_stack(1).rarity(Rarity::Epic),
    )?;

    ItemGroup::builder("item_edges:tab")
        .title("Item Edges")
        .icon("item_edges:consumer")
        .entries([
            "item_edges:consumer",
            "item_edges:refuser",
            "item_edges:passer",
            "item_edges:stacked",
        ])
        .register()?;
    ItemGroups::modify_entries("minecraft:building_blocks", ["item_edges:consumer"])?;
    Ok(())
}

fish_api::export_mod! {
    main: [init],
}
