# Interaction events

`fish_api::event::interaction`. All but `BLOCK_BREAK_AFTER` and `BLOCK_BREAK_CANCELED` are *first non-pass*:
return `ActionResult::Fail` to cancel, `Success`/`Consume` to mark the action handled (for the game: allowed and done),
`Pass` to let the next listener and the game decide.

| Event | Fields |
| --- | --- |
| `USE_BLOCK` | `pos`, `face`, `hand`, `subject` = block, `item` = held item |
| `USE_ITEM` | `hand`, `item` |
| `USE_ENTITY` | `entity_id`, `subject` = entity type, `hand`, `item` |
| `ATTACK_BLOCK` | `pos`, `face`, `subject` |
| `ATTACK_ENTITY` | `entity_id`, `subject` |
| `BLOCK_BREAK_BEFORE` | `pos`, `subject`; `Fail` stops the break |
| `BLOCK_BREAK_CANCELED` | the same, after a listener cancelled |
| `BLOCK_BREAK_AFTER` | the same, on the next server tick after an uncancelled break |
| `PICK_ITEM` | `item` (middle click) |

`actor()` is the acting player (a guest on a LAN or integrated server, the local player otherwise); `is_client()` is
true for the window's *prediction* of an action the server decides. Cancel both, or the server will correct the client:

```rust
use fish_api::event::interaction::BLOCK_BREAK_BEFORE;
use fish_api::ActionResult;

BLOCK_BREAK_BEFORE.register(|c| {
    if c.subject() == "minecraft:bedrock" { ActionResult::Fail } else { ActionResult::Pass }
})?;
```

## Notes

- `BLOCK_BREAK_CANCELED` and `BLOCK_BREAK_AFTER` are derived by the loader from the outcome of
  `BLOCK_BREAK_BEFORE`. `AFTER` means *the break was not vetoed*; Horizon does not report a later failure.
- `hand` is `Hand::Main` or `Hand::Off`; `face` is `None` when the action has no face.
