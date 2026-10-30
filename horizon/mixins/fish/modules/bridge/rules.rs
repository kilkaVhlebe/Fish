// Fish game rules in the game: `/gamerule` for rules mods registered, and their saving and loading
// beside the game's own (the world's `data/fish/game_rules.json`).

use std::ops::ControlFlow;

use fish_hooks::GameRuleReply;
use horizon_client::command::{Arg, CommandError};
use horizon_client::text::Text;

use crate::game::Game;
use crate::game::commands::{R, err};

/// The id Fish keeps a world's rules under: its folder. `None` where the game has no world of its
/// own (a remote server, the menu).
pub(in crate::game) fn world_of(game: &Game<'_>) -> Option<String> {
    (!game.world_root.as_os_str().is_empty())
        .then(|| game.world_root.to_string_lossy().into_owned())
}

/// `cmd_gamerule`: runs `/gamerule <rule> [<value>]` when the rule is one a mod registered.
/// `literals` are the words the line matched (`gamerule`, the rule, a choice), `value` the
/// argument after the rule.
pub(in crate::game) fn command(
    game: &mut Game<'_>,
    literals: &[String],
    value: Option<&Arg>,
) -> ControlFlow<R<i32>> {
    let (Some(name), Some(d)) = (literals.get(1), fish_hooks::get()) else {
        return ControlFlow::Continue(());
    };
    if horizon_client::gamerules::rule(name).is_some() {
        return ControlFlow::Continue(());
    }
    let Some(world) = world_of(game) else {
        return ControlFlow::Continue(());
    };
    let given = match (value, literals.get(2)) {
        (Some(Arg::Bool(b)), _) => Some(b.to_string()),
        (Some(Arg::Int(i)), _) => Some(i.to_string()),
        (Some(Arg::Double(f)), _) => Some(f.to_string()),
        (_, Some(choice)) => Some(choice.clone()),
        _ => None,
    };
    let Some(reply) = fish_hooks::game_rule_command(d, &world, name, given.as_deref()) else {
        return ControlFlow::Continue(());
    };
    ControlFlow::Break(match reply {
        GameRuleReply::Query { name, value, result } => {
            game.system_message(Text::tr("commands.gamerule.query", &[&name, &value]));
            Ok(result)
        }
        GameRuleReply::Set { name, value, result } => {
            game.feedback(Text::tr("commands.gamerule.set", &[&name, &value]));
            Ok(result)
        }
        GameRuleReply::Unchanged { name, value } => {
            Err(err("commands.gamerule.not_set", &[&name, &value]))
        }
        GameRuleReply::Failed(why) => Err(CommandError::new(Text::literal(why.as_str()))),
    })
}

/// `load_world_state`: a world opened, so its Fish rules are read.
pub(in crate::game) fn load(game: &Game<'_>) {
    if let (Some(d), Some(world)) = (fish_hooks::get(), world_of(game)) {
        d.game_rules_load(&world);
    }
}

/// `save_game_metadata`: the world is saved, and the Fish rules with it.
pub(in crate::game) fn save(game: &Game<'_>) {
    if let (Some(d), Some(world)) = (fish_hooks::get(), world_of(game)) {
        d.game_rules_save(&world);
    }
}
