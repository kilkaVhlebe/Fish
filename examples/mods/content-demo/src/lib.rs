//! Example mod: blocks, items, a creative tab, content tables, a sound event and a particle type.
//!
//! * `content_demo:ruby_block` copies iron, glows a little, and logs every event the game raises
//!   for it; using it is handled (the click ends there) unless the hand holds honeycomb or an axe,
//! * `content_demo:raw_ruby` copies stone and has no behaviour; mobs avoid walking on it
//!   (pathfinding type `DAMAGING`),
//! * `content_demo:ruby_log` and `content_demo:stripped_ruby_log` copy the oak logs: an axe strips
//!   the first into the second, and both burn easily,
//! * honeycomb waxes the ruby block into `content_demo:waxed_ruby_block` (an axe scrapes it back),
//! * `content_demo:ruby_lamp` copies iron and adds the state properties `lit` and `level`: using
//!   it lights it, then raises its level; a neighbour change puts it out,
//! * `content_demo:ruby` (a diamond that stacks to 16) logs its uses, once its place in the
//!   inventory, and adds a tooltip line; it burns in a furnace for 2400 ticks, and using it in the
//!   air plays the mod's sound event `content_demo:ruby_chime`,
//! * `content_demo:ruby_snack` (an apple) logs who ate it and composts with a 50 % chance,
//! * eating makes a vibration of frequency 12 for sculk sensors (vanilla: 8),
//! * zombies sometimes drop a ruby (a pool added to their loot table) and a dead bush drops a ruby
//!   (its loot table replaced),
//! * the particle type `content_demo:ruby_sparkle` is registered (it has no client factory yet:
//!   nothing draws it),
//! * the tab `content_demo:ruby_tab` lists the items and blocks above, and the ruby is also added
//!   to vanilla's `minecraft:ingredients`,
//! * the mod reads its own `assets/` and, at start and after every resource reload, the game's copy
//!   of its `sounds.json`; the ruby block drops a ruby (`data/content_demo/loot_table`) and is
//!   crafted from nine rubies; a recipe that needs the mod `gems` is left out by its load conditions,
//! * the server logs the state ids of the ruby block and the raw id of the particle type when it
//!   starts and tries to register one more block and one more item (refused: the registries are
//!   frozen).
//!
//! The block and item models, textures, translations, `sounds.json` with its `.ogg` file and the
//! particle description are the mod's own `assets/` and `data/`; the loader hands them to Horizon
//! together with the declarations.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use fish_api::block::{BlockBehaviour, BlockSettings, Blocks, StateProperty};
use fish_api::content::{Content, PathNodeType};
use fish_api::event::lifecycle;
use fish_api::item::{ItemBehaviour, ItemSettings, Items, Rarity};
use fish_api::item_group::{ItemGroup, ItemGroups};
use fish_api::loot::{LootPool, LootTables};
use fish_api::particle::{ParticleType, Particles};
use fish_api::resource::Resources;
use fish_api::sound::{SoundEvent, Sounds};
use fish_api::{ActionResult, Error};

/// Logs the inventory tick of the ruby once, so the log stays short.
static TICK_LOGGED: AtomicBool = AtomicBool::new(false);
/// The sound event the ruby plays.
static CHIME: OnceLock<SoundEvent> = OnceLock::new();
/// Set when the ruby was used: the window plays the chime on its next tick.
static CHIME_PENDING: AtomicBool = AtomicBool::new(false);
/// The particle type, for its raw id.
static SPARKLE: OnceLock<ParticleType> = OnceLock::new();

/// The mod's own resource files, and the game's copy of its `sounds.json` at start and after every
/// resource reload (the file reaches the game through the content folder).
fn resources() -> Result<(), Error> {
    let files = Resources::list_own("assets/content_demo/")?;
    let sounds = Resources::own_text("assets/content_demo/sounds.json")?;
    log::info!(
        "content_demo has {} asset files; its sounds.json names ruby_chime: {}",
        files.len(),
        sounds.contains("ruby_chime")
    );
    let game_sounds =
        |client: &fish_api::client::ClientHandle, when: &str| match Resources::game_text(
            client,
            "assets/content_demo/sounds.json",
        ) {
            Ok(text) => log::info!(
                "game resources {when}: sounds.json names ruby_chime: {}",
                text.contains("ruby_chime")
            ),
            Err(e) => log::warn!("game resources {when}: {e}"),
        };
    lifecycle::client::STARTED.register(move |c| game_sounds(&c.client(), "at start"))?;
    lifecycle::resources::RELOADED.register(move |c| game_sounds(&c.client(), "after a reload"))?;
    Ok(())
}

/// The ruby lamp: state properties `lit` (off by default) and `level` (0 to 3). Using it lights it,
/// then raises its level; a neighbour change puts it out (an event without a game: the write
/// waits for the server's next tick).
fn lamp() -> Result<(), Error> {
    let behaviour = BlockBehaviour::new()
        .on_use(|c| {
            let result = if c.property_bool("lit") == Some(true) {
                let level = (c.property_int("level").unwrap_or(0) + 1) % 4;
                c.set_property("level", level)
                    .map(|()| log::info!("ruby lamp level {level} at {}", c.pos()))
            } else {
                c.set_property("lit", true)
                    .map(|()| log::info!("ruby lamp lit at {}", c.pos()))
            };
            if let Err(e) = result {
                log::warn!("ruby lamp not changed: {e}");
            }
            ActionResult::Success
        })
        .on_place(|c| log::info!("ruby lamp placed: {}", c.properties()))
        .neighbor_changed(|c| {
            if c.property_bool("lit") == Some(true) {
                match c.set_property("lit", false) {
                    Ok(()) => log::info!("ruby lamp goes out at {}", c.pos()),
                    Err(e) => log::warn!("ruby lamp did not go out: {e}"),
                }
            }
        });
    Blocks::register_with(
        "content_demo:ruby_lamp",
        BlockSettings::copy("minecraft:iron_block")
            .hardness(2.0)
            .property(StateProperty::boolean("lit", false))
            .property(StateProperty::int("level", 0..=3, 0)),
        behaviour,
    )?;
    Ok(())
}

/// The content tables, the sound event and the particle type.
fn content() -> Result<(), Error> {
    Blocks::register(
        "content_demo:ruby_log",
        BlockSettings::copy("minecraft:oak_log"),
    )?;
    Blocks::register(
        "content_demo:stripped_ruby_log",
        BlockSettings::copy("minecraft:stripped_oak_log"),
    )?;
    Blocks::register(
        "content_demo:waxed_ruby_block",
        BlockSettings::copy("minecraft:iron_block")
            .hardness(5.0)
            .resistance(6.0)
            .requires_tool(true)
            .light(3)
            .sounds("minecraft:amethyst_block"),
    )?;
    Content::fuel("content_demo:ruby", 2400)?;
    Content::compostable("content_demo:ruby_snack", 0.5)?;
    Content::flammable("content_demo:ruby_log", 30, 60)?;
    Content::flammable("content_demo:stripped_ruby_log", 30, 60)?;
    Content::strippable("content_demo:ruby_log", "content_demo:stripped_ruby_log")?;
    Content::waxable("content_demo:ruby_block", "content_demo:waxed_ruby_block")?;
    Content::path_node_type("content_demo:raw_ruby", PathNodeType::Damaging)?;
    Content::sculk_frequency("minecraft:eat", 12)?;
    match Content::fuel("content_demo:ruby", 100) {
        Ok(()) => log::error!("a second fuel entry for the ruby was accepted"),
        Err(e) => log::info!("second ruby fuel refused: {e}"),
    }
    // Loot: zombies sometimes drop a ruby, a dead bush drops a ruby instead of sticks.
    LootTables::add_pool(
        "minecraft:entities/zombie",
        &LootPool::item("content_demo:ruby").chance(0.05).to_json(),
    )?;
    LootTables::replace(
        "minecraft:blocks/dead_bush",
        &format!(
            r#"{{"type":"minecraft:block","pools":[{}]}}"#,
            LootPool::item("content_demo:ruby").to_json()
        ),
    )?;
    let _ = CHIME.set(Sounds::register("content_demo:ruby_chime")?);
    let _ = SPARKLE.set(Particles::register("content_demo:ruby_sparkle")?);
    lifecycle::client::END_TICK.register(|c| {
        if !CHIME_PENDING.swap(false, Ordering::Relaxed) {
            return;
        }
        let Some(chime) = CHIME.get() else { return };
        let client = c.client();
        let at = client.player().ok().flatten().map(|p| [p.x, p.y, p.z]);
        let played = match at {
            Some(pos) => chime.play_at(&client, pos, 1.0, 1.0),
            None => chime.play(&client, 1.0, 1.0),
        };
        match played {
            Ok(()) => log::info!("ruby chime played"),
            Err(e) => log::warn!("ruby chime not played: {e}"),
        }
    })?;
    Ok(())
}

fn init() -> Result<(), Error> {
    let behaviour = BlockBehaviour::new()
        .on_use(|c| {
            // Honeycomb and axes act on the block themselves (waxing, scraping).
            if c.item() == "minecraft:honeycomb" || c.item().ends_with("_axe") {
                return ActionResult::Pass;
            }
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
            // The use runs on the server, which has no audio: the window plays the chime on its
            // next tick.
            CHIME_PENDING.store(true, Ordering::Relaxed);
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
            "content_demo:ruby_log",
            "content_demo:stripped_ruby_log",
            "content_demo:waxed_ruby_block",
            "content_demo:ruby_lamp",
        ])
        .register()?;
    content()?;
    lamp()?;
    resources()?;
    ItemGroups::modify_entries("minecraft:ingredients", ["content_demo:ruby"])?;

    lifecycle::server::STARTED.register(|_| {
        match Blocks::state_range("content_demo:ruby_block") {
            Ok(r) => log::info!("ruby states {}..{}", r.start, r.end),
            Err(e) => log::error!("ruby states unknown: {e}"),
        }
        if let Some(sparkle) = SPARKLE.get() {
            match sparkle.raw_id() {
                Ok(id) => log::info!("ruby sparkle raw id {id}"),
                Err(e) => log::error!("ruby sparkle raw id unknown: {e}"),
            }
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
