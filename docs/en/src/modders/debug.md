# F3 lines, crash reports and stopped mods

## F3

`fish_api::event::debug::F3_LINES` fires each frame the F3 screen is drawn; add lines with `ctx.add_line(text)`
(at most 32 lines of 200 characters; extra ones are dropped). Fish always adds its own first line,
`[Fish] 0.1.0 - N mod(s)`.

```rust
use fish_api::event::debug::F3_LINES;

F3_LINES.register(|c| c.add_line("my mod: 3 machines active"))?;
```

## When a mod fails

A mod fails when an entrypoint or a listener panics or returns an error. Fish then:

- logs `Fish mod <name> stopped: <reason>` and stops calling its listeners and commands for the session,
- shows the same line in the player's chat on the next client tick,
- lists the mod in **Options > Mods** as `Name 1.0 [Fish] (stopped)` with the reason as its description,
- adds `Fish`, `Fish mods` and `Fish failed mods` to the **System Details** of crash reports (`crash-reports/`), so a
  report always says which Fish and which mods were running.

Mods that depend on a failed mod fail with `dependency <id> failed to load`.
