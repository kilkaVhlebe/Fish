//! Example mod: four game rules (boolean, integer, decimal, enum) with a change callback.
//!
//! * `/gamerule gamerule_demo:allow_stone_break false` makes the server refuse to break stone,
//! * the client command `rules show` logs every rule of the current world, `rules cap` sets the
//!   integer rule to 4 from the mod,
//! * the server logs the rules when it starts and tries to register one more (refused: frozen).

use fish_api::client::command::RAW_COMMAND;
use fish_api::event::{interaction, lifecycle};
use fish_api::game_rule::{BoolRule, Category, DoubleRule, EnumRule, GameRules, IntRule};
use fish_api::{ActionResult, Error};

struct Rules {
    allow_stone_break: BoolRule,
    sparkle_limit: IntRule,
    damage_scale: DoubleRule,
    difficulty_mode: EnumRule,
}

fn describe(rules: &Rules) -> String {
    format!(
        "allow_stone_break={:?} sparkle_limit={:?} damage_scale={:?} difficulty_mode={:?}",
        rules.allow_stone_break.get(),
        rules.sparkle_limit.get(),
        rules.damage_scale.get(),
        rules.difficulty_mode.get()
    )
}

fn init() -> Result<(), Error> {
    let rules = std::sync::Arc::new(Rules {
        allow_stone_break: GameRules::register_bool(
            "gamerule_demo:allow_stone_break",
            Category::Player,
            true,
        )?,
        sparkle_limit: GameRules::register_int(
            "gamerule_demo:sparkle_limit",
            Category::Mobs,
            8,
            0,
            64,
        )?,
        damage_scale: GameRules::register_double(
            "gamerule_demo:damage_scale",
            Category::Drops,
            1.0,
            0.0,
            10.0,
        )?,
        difficulty_mode: GameRules::register_enum(
            "gamerule_demo:difficulty_mode",
            Category::Misc,
            "normal",
            &["easy", "normal", "hard"],
        )?,
    });
    GameRules::on_change("gamerule_demo:sparkle_limit", |c| {
        log::info!("sparkle_limit changed {:?} -> {:?}", c.old, c.new);
    })?;

    let r = rules.clone();
    interaction::BLOCK_BREAK_BEFORE.register(move |c| {
        if c.subject() == "minecraft:stone" && r.allow_stone_break.get() == Ok(false) {
            log::info!("stone breaking is switched off at {}", c.pos());
            ActionResult::Fail
        } else {
            ActionResult::Pass
        }
    })?;

    let r = rules.clone();
    lifecycle::server::STARTED.register(move |_| {
        log::info!("rules at start: {}", describe(&r));
        match GameRules::register_bool("gamerule_demo:late", Category::Misc, true) {
            Ok(_) => log::error!("a late game rule was registered"),
            Err(e) => log::info!("late game rule refused: {e}"),
        }
    })?;

    let r = rules;
    RAW_COMMAND.register(move |ctx| match ctx.command() {
        "rules show" => {
            log::info!("rules: {}", describe(&r));
            ActionResult::Success
        }
        "rules cap" => {
            match r.sparkle_limit.set(4) {
                Ok(()) => log::info!("capped the sparkle limit"),
                Err(e) => log::info!("cannot cap the sparkle limit: {e}"),
            }
            ActionResult::Success
        }
        _ => ActionResult::Pass,
    })?;
    Ok(())
}

fish_api::export_mod! {
    main: [init],
}
