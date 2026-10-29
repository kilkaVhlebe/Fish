# События сущностей

`fish_api::event::entity`.

| Событие | Вид | Поля |
| --- | --- | --- |
| `ALLOW_DAMAGE` | первый не-Pass, `Fail` отменяет | `entity_id`, `subject` = тип, `amount`, `extra` = вид урона |
| `AFTER_DAMAGE` | уведомление | то же; загрузчик шлёт после неотменённого `ALLOW_DAMAGE` |
| `ALLOW_DEATH` | первый не-Pass, `Fail` отменяет | то же |
| `AFTER_DEATH` | уведомление | `entity_id`, `subject`, `pos` |
| `player::JOIN`, `LEAVE` | уведомление | `actor`, `actor_uuid` |
| `player::AFTER_RESPAWN`, `COPY_FROM` | уведомление | `actor` |
| `sleep::ALLOW` | первый не-Pass, `Fail` отменяет | `pos` кровати, `actor` |
| `sleep::START`, `STOP` | уведомление | `actor` (`pos` при засыпании) |

```rust
use fish_api::event::entity::ALLOW_DAMAGE;
use fish_api::ActionResult;

ALLOW_DAMAGE.register(|c| {
    if c.extra() == "minecraft:fall" { ActionResult::Fail } else { ActionResult::Pass }
})?;
```

## Замечания

- У Horizon нет шага копирования при возрождении (игрок изменяется на месте), поэтому `COPY_FROM` приходит вместе с
  `AFTER_RESPAWN`.
- `AFTER_DAMAGE` сообщает *разрешённый* урон — до учёта брони и поглощения.
