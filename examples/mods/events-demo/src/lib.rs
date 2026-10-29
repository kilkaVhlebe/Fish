//! Example mod: listens to the P6 events.
//!
//! * a block-break veto (bedrock), a use veto (obsidian with a stick),
//! * a chat filter (drops messages with "badword") and a chat rewrite (`:fish:` becomes `><>`),
//! * a welcome log on join, fall damage immunity, a data reload notice,
//! * the `/fishdemo` client command.

use fish_api::client::command::RAW_COMMAND;
use fish_api::command::{self, ArgKind, argument, literal};
use fish_api::event::{debug, entity, interaction, lifecycle, message};
use fish_api::{ActionResult, Error};

fn report(result: Result<(), Error>) {
    if let Err(e) = result {
        log::error!("cannot register a command: {e}");
    }
}

fn init() -> Result<(), Error> {
    lifecycle::client::STARTED.register(|_| log::info!("client started"))?;
    lifecycle::client::STOPPING.register(|_| log::info!("client stopping"))?;
    lifecycle::client::STOPPED.register(|_| log::info!("client stopped"))?;
    lifecycle::server::STARTING.register(|c| log::info!("server starting: {}", c.subject()))?;
    lifecycle::server::STARTED.register(|_| log::info!("server started"))?;
    lifecycle::server::STOPPING.register(|_| log::info!("server stopping"))?;
    lifecycle::server::STOPPED.register(|_| log::info!("server stopped"))?;
    lifecycle::world::LOAD.register(|c| log::info!("world loaded: {}", c.dimension()))?;
    lifecycle::world::UNLOAD.register(|c| log::info!("world unloading: {}", c.dimension()))?;
    lifecycle::chunk::LOAD
        .register(|c| log::debug!("chunk loaded: {} {}", c.pos().x, c.pos().z))?;
    lifecycle::reload::START.register(|_| log::info!("reload started"))?;
    lifecycle::reload::END.register(|_| log::info!("reload finished"))?;

    interaction::BLOCK_BREAK_BEFORE.register(|c| {
        if c.subject() == "minecraft:bedrock" {
            log::info!("refusing to break bedrock at {}", c.pos());
            ActionResult::Fail
        } else {
            ActionResult::Pass
        }
    })?;
    interaction::BLOCK_BREAK_CANCELED.register(|c| log::info!("break canceled at {}", c.pos()))?;
    interaction::BLOCK_BREAK_AFTER
        .register(|c| log::info!("{} broke at {}", c.subject(), c.pos()))?;
    interaction::USE_BLOCK.register(|c| {
        if c.subject() == "minecraft:obsidian" && c.item() == "minecraft:stick" {
            log::info!(
                "a stick does nothing to obsidian ({})",
                if c.is_client() { "client" } else { "server" }
            );
            ActionResult::Fail
        } else {
            ActionResult::Pass
        }
    })?;
    interaction::ATTACK_ENTITY.register(|c| {
        log::info!("{} attacks {} #{}", c.actor(), c.subject(), c.entity_id());
        ActionResult::Pass
    })?;

    entity::ALLOW_DAMAGE.register(|c| {
        if c.extra() == "minecraft:fall" {
            ActionResult::Fail
        } else {
            ActionResult::Pass
        }
    })?;
    entity::AFTER_DAMAGE
        .register(|c| log::info!("{} took {} damage ({})", c.subject(), c.amount(), c.extra()))?;
    entity::AFTER_DEATH.register(|c| log::info!("{} #{} died", c.subject(), c.entity_id()))?;
    entity::player::JOIN.register(|c| log::info!("welcome {}", c.actor()))?;
    entity::player::LEAVE.register(|c| log::info!("goodbye {}", c.actor()))?;

    message::server::ALLOW_CHAT.register(|c| {
        if c.subject().contains("badword") {
            log::info!("dropped a message from {}", c.actor());
            ActionResult::Fail
        } else {
            ActionResult::Pass
        }
    })?;
    message::client::MODIFY_SEND.register(|c| {
        if c.subject().contains(":fish:") {
            let text = c.subject().replace(":fish:", "><>");
            c.replace_text(&text);
        }
    })?;
    message::client::ALLOW_RECEIVE.register(|c| {
        if c.subject().contains("spam") {
            ActionResult::Fail
        } else {
            ActionResult::Pass
        }
    })?;

    RAW_COMMAND.register(|c| {
        if c.command() == "fishraw" {
            let _ = c.feedback("events-demo is listening");
            ActionResult::Success
        } else {
            ActionResult::Pass
        }
    })?;
    debug::F3_LINES.register(|c| c.add_line("events-demo is listening"))?;
    command::REGISTER.register(|ctx| {
        report(
            ctx.register(
                literal("fishdemo")
                    .then(literal("hello").executes(|c| {
                        c.feedback(&format!("hello {}", c.source_name()));
                        Ok(1)
                    }))
                    .then(
                        literal("add").then(
                            argument(
                                "a",
                                ArgKind::Integer {
                                    min: -100,
                                    max: 100,
                                },
                            )
                            .then(
                                argument(
                                    "b",
                                    ArgKind::Integer {
                                        min: -100,
                                        max: 100,
                                    },
                                )
                                .executes(|c| {
                                    let sum = c.get_integer("a")? + c.get_integer("b")?;
                                    c.feedback(&format!("sum = {sum}"));
                                    Ok(sum)
                                }),
                            ),
                        ),
                    )
                    .then(literal("say").requires(2).then(
                        argument("text", ArgKind::Greedy).executes(|c| {
                            c.feedback(&format!(
                                "{} says {}",
                                c.source_name(),
                                c.get_string("text")?
                            ));
                            Ok(1)
                        }),
                    ))
                    .then(
                        literal("mode").then(
                            argument("mode", ArgKind::Word)
                                .suggests(&["fast", "slow"])
                                .executes(|c| {
                                    c.feedback(&format!("mode {}", c.get_string("mode")?));
                                    Ok(0)
                                }),
                        ),
                    )
                    .then(
                        literal("at").then(argument("pos", ArgKind::BlockPos).executes(|c| {
                            c.feedback(&format!("at {}", c.get_block_pos("pos")?));
                            Ok(1)
                        })),
                    )
                    .then(literal("fail").executes(|_| Err("on purpose".into()))),
            ),
        );
    })?;
    command::client::REGISTER.register(|ctx| {
        report(ctx.register(literal("fishclient").executes(|c| {
            c.feedback("client command ran");
            Ok(1)
        })));
    })?;
    Ok(())
}

fish_api::export_mod! {
    main: [init],
}
