fn fish_gamerule_tree(mut d: Dispatcher) -> Dispatcher {
    let Some(fish) = fish_hooks::get() else {
        return d;
    };
    let mut rules = Vec::new();
    fish.game_rules(&mut rules);
    let Some(parent) = d.find("gamerule") else {
        return d;
    };
    for rule in rules {
        let node = d.literal(parent, &rule.name);
        d.executes(node, "gamerule");
        match &rule.kind {
            fish_hooks::RuleKind::Bool => {
                let v = d.argument(node, "value", ArgType::Bool);
                d.executes(v, "gamerule");
            }
            fish_hooks::RuleKind::Int { min, max } => {
                let v = d.argument(node, "value", ArgType::Integer { min: *min, max: *max });
                d.executes(v, "gamerule");
            }
            fish_hooks::RuleKind::Double { min, max } => {
                let v = d.argument(node, "value", ArgType::Double { min: *min, max: *max });
                d.executes(v, "gamerule");
            }
            fish_hooks::RuleKind::Enum(choices) => {
                for choice in choices {
                    let c = d.literal(node, choice);
                    d.executes(c, "gamerule");
                }
            }
        }
    }
    d
}
