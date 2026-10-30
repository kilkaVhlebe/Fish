# Game rules

`fish_api::game_rule` adds rules to `/gamerule`: a boolean, an integer, a decimal or one of named choices, each with a
category and a default. Every world keeps its own values and the game saves them with the world.

```rust
use fish_api::game_rule::{Category, GameRules};

fn init() -> Result<(), fish_api::Error> {
    let sparkles = GameRules::register_bool("mymod:sparkles", Category::Misc, true)?;
    let limit = GameRules::register_int("mymod:limit", Category::Mobs, 8, 0, 64)?;
    let scale = GameRules::register_double("mymod:scale", Category::Drops, 1.0, 0.0, 10.0)?;
    let mode = GameRules::register_enum("mymod:mode", Category::Misc, "normal", &["easy", "normal", "hard"])?;

    GameRules::on_change("mymod:limit", |c| log::info!("limit {:?} -> {:?} in {}", c.old, c.new, c.world))?;

    // later, inside any event listener of a game that has a world:
    if sparkles.get()? && limit.get()? > 4 {
        limit.set(4)?;
    }
    Ok(())
}
```

- **Names** are `namespace:path`. The `minecraft` namespace belongs to the game and is refused.
- **Categories** are `Player`, `Mobs`, `Spawning`, `Drops`, `Updates`, `Chat` and `Misc`: the game's own groups.
- **Registration** is allowed while `pre_launch` or `main` runs, the same freeze as [registries](registry.md); a late call
  fails with `registries are frozen`. A name registered twice fails with `ALREADY_EXISTS` (the first mod keeps it); an
  invalid definition (reversed bounds, a default outside them, an empty or duplicated choice) fails with
  `INVALID_ARGUMENT` and the loader logs why. Choices are made of letters, digits, `_`, `-` and `.` because players type
  them.
- **`get` and `set`** act on the world of the game whose event is running (any event that has a game: ticks, interaction,
  commands, keys, HUD). Outside of an event there is no world: `get` answers the default and `set` fails with
  `UNSUPPORTED`. `GameRules::get(name)` / `GameRules::set(name, Value)` reach any mod's rule by name; the typed handles
  (`BoolRule`, `IntRule`, `DoubleRule`, `EnumRule`) are what `register_*` returns. Setting a value of another type fails
  with `TYPE_MISMATCH`, one out of bounds or not among the choices with `INVALID_ARGUMENT`.
- **Change callbacks** (`GameRules::on_change`, or `game_rule::CHANGED_EVENT` for every rule) run after a value changed,
  whether by `/gamerule`, the rule screen or a mod. Setting the value a rule already has raises nothing.

## In the game

`/gamerule <namespace:path> [<value>]` queries or sets a rule, with the game's own messages and the permission level of
`/gamerule` (2). The command tree is built with the rules, so chat completion and colouring know them: a boolean rule
takes `true`/`false`, integers and decimals take a number within the bounds (a value out of bounds is refused by the
parser), enum rules complete their choices. The result of the command (`/execute store`) is 0 or 1 for a boolean, the
value for an integer, the truncated value for a decimal and the index of the choice for an enum. A line like
`/gamerule mymod:limit 99` (out of bounds) fails with the bounds in the message.

In World Options > Game Rules the rules of mods are listed in their categories, after the game's own of the category,
under the mod's name for the rule: a switch for a boolean, a box for an integer or decimal, a button that cycles the
choices of an enum. Done writes the changed ones (silently: the screen does not announce Fish rules in chat; `/gamerule`
does).

## Saving

The game's `data/minecraft/game_rules.dat` is left alone: vanilla refuses a file with keys it does not know. Fish keeps
its rules beside it, under its own namespace, in `<world>/data/fish/game_rules.json`:

```json
{ "schema": 1, "rules": { "mymod:limit": 4, "mymod:mode": "hard" } }
```

Only values that differ from the default are written (and the file is removed when nothing differs). Values of rules no
mod registers any more are kept as they were, so taking a mod out for a while loses nothing; a value that does not fit its
rule (wrong type, out of bounds) is ignored with a warning. The rules are read when a world opens and written when it is
saved. A world made without Fish, or a vanilla world opened with Fish, has no such file and all defaults. Removing Fish
leaves a save that vanilla reads as before.

## Testing

`cargo fish test` scripts have two keys, and `expect.gamerules` reads values back:

```toml
[[step]]
gamerule = { name = "mymod:limit", value = 12 }     # /gamerule in the current world
[step.expect]
command_feedback = ["Gamerule mymod:limit is now set to: 12"]
gamerules = { "mymod:limit" = 12, "mymod:sparkles" = true }

[[step]]
world = "other"            # saves this world's rules to disk, opens another and reads its rules
[step.expect]
gamerules = { "mymod:limit" = 8 }
```

`gamerule` without `value` queries the rule. A step with a value out of bounds or of the wrong type reports the reason in
`command_feedback` and leaves the value unchanged. In Rust, `TestHost::gamerule`, `game_rule`, `game_rule_defs`,
`switch_world` and `close_world` do the same; `sim.world = None` is the main menu. See `examples/mods/gamerule-demo`.

## Limits

- A client connected to a **remote server** has no world of its own: its `get` answers the defaults and `set` fails. The
  server's Fish (if it has the mod) owns the values; Fish does not synchronise them to the client yet, and the Game
  Rules screen of such a client shows only the game's rules.
- **Create New World**'s rule screen lists only the game's rules: a new world starts with the defaults and `/gamerule`
  changes them. The in-world screen of a world this game runs lists Fish's rules.
- The command tree is built once, on first use, which happens after Fish booted: rules must be registered while the
  entrypoints run (they cannot be registered later anyway).
- The categories are the game's seven; there are no custom ones.
- Values are saved when the game saves the world and read when it opens one; a world that is open in two games of one
  process (the window and its integrated server) shares one set of values.
