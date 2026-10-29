# Entity events

`fish_api::event::entity`.

| Event | Kind | Fields |
| --- | --- | --- |
| `ALLOW_DAMAGE` | first non-pass, `Fail` cancels | `entity_id`, `subject` = type, `amount`, `extra` = damage kind |
| `AFTER_DAMAGE` | notify | the same; fired by the loader after an uncancelled `ALLOW_DAMAGE` |
| `ALLOW_DEATH` | first non-pass, `Fail` cancels | the same |
| `AFTER_DEATH` | notify | `entity_id`, `subject`, `pos` |
| `player::JOIN`, `LEAVE` | notify | `actor`, `actor_uuid` |
| `player::AFTER_RESPAWN`, `COPY_FROM` | notify | `actor` |
| `sleep::ALLOW` | first non-pass, `Fail` cancels | `pos` of the bed, `actor` |
| `sleep::START`, `STOP` | notify | `actor` (`pos` on start) |

```rust
use fish_api::event::entity::ALLOW_DAMAGE;
use fish_api::ActionResult;

ALLOW_DAMAGE.register(|c| {
    if c.extra() == "minecraft:fall" { ActionResult::Fail } else { ActionResult::Pass }
})?;
```

## Notes

- Horizon has no copy step on respawn (the player is mutated in place), so `COPY_FROM` fires with `AFTER_RESPAWN`.
- `AFTER_DAMAGE` reports the damage that was *allowed*, before armor and absorption.
