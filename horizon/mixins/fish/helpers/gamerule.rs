fn fish_gamerule(game: &mut Game<'_>, ctx: &Context) -> std::ops::ControlFlow<R<i32>> {
    crate::game::fish_bridge::rules::command(game, &ctx.literals, ctx.get("value"))
}
