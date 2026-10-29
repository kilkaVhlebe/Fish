# Commands

`fish_api::command`: a Brigadier-style builder. You build a tree in the mod, register it once, and the host parses,
permission-checks and converts arguments before your executor runs.

```rust
use fish_api::command::{self, ArgKind, argument, literal};

command::REGISTER.register(|ctx| {
    let tree = literal("fishdemo")
        .then(literal("hello").executes(|c| {
            c.feedback(&format!("hello {}", c.source_name()));
            Ok(1)
        }))
        .then(literal("add").then(
            argument("a", ArgKind::Integer { min: -100, max: 100 }).then(
                argument("b", ArgKind::Integer { min: -100, max: 100 }).executes(|c| {
                    let sum = c.get_integer("a")? + c.get_integer("b")?;
                    c.feedback(&format!("sum = {sum}"));
                    Ok(sum)
                }),
            ),
        ))
        .then(literal("say").requires(2).then(
            argument("text", ArgKind::Greedy).executes(|c| { c.feedback(c.get_string("text")?); Ok(1) }),
        ));
    if let Err(e) = ctx.register(tree) {
        log::error!("cannot register /fishdemo: {e}");
    }
})?;
```

## Two registration events

| Event | The commands run |
| --- | --- |
| `command::REGISTER` | on the server (also in single player), for whoever types them, at their permission level |
| `command::client::REGISTER` | in the client itself, never need a server; no permission levels |

Both fire once, after every mod's entrypoints ran. `ctx.side()` tells which side Fish runs on.

## Nodes

- `literal(name)`, `argument(name, kind)`; `.then(child)`, `.executes(f)`, `.requires(level)`, `.redirect(root)`,
  `.suggests(&["a", "b"])`.
- `requires(level)` takes 0 (everyone), 2 (game master) or 4 (owner); children inherit it. A node above the source's
  level does not exist for it: the line is reported as unknown.
- `redirect("root")` continues parsing at the children of another root command.
- Executors return `Ok(value)` (the command's result) or `Err("message")` (shown in red, the command fails).
  `feedback(text)` shows a line to the source.

| `ArgKind` | Value |
| --- | --- |
| `Word`, `String` (quotes allowed), `Greedy` (rest of the line, last node only) | `get_string` |
| `Integer { min, max }` | `get_integer` |
| `Double { min, max }` | `get_double` |
| `Bool` | `get_bool` |
| `Player` (a name), `Entity` (a selector or name, **not resolved**) | `get_string` |
| `BlockPos` (three coordinates, `~` and `~n` relative to the source) | `get_block_pos` |

## Rules

- Names: literals `[A-Za-z0-9_.+-]{1,64}`; arguments `[a-z][a-z0-9_]{0,63}`. A tree has at most 4096 nodes.
- **Vanilla names win.** If a root equals a game command, Fish never runs it and logs a warning. If two mods register the
  same root, the first keeps it and the second gets a warning.
- A failed (stopped) mod's commands do not run.
- Suggestions: literals and `.suggests(...)` lists are offered by the loader; whether the game's chat shows them depends
  on the edition (see the hook notes in the architecture chapter).
