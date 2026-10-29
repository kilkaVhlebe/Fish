# События взаимодействия

`fish_api::event::interaction`. Все, кроме `BLOCK_BREAK_AFTER` и `BLOCK_BREAK_CANCELED`, — *первый не-Pass*:
верните `ActionResult::Fail`, чтобы отменить, `Success`/`Consume` — отметить действие обработанным (для игры:
разрешено и выполнено), `Pass` — оставить решение следующим слушателям и игре.

| Событие | Поля |
| --- | --- |
| `USE_BLOCK` | `pos`, `face`, `hand`, `subject` = блок, `item` = предмет в руке |
| `USE_ITEM` | `hand`, `item` |
| `USE_ENTITY` | `entity_id`, `subject` = тип сущности, `hand`, `item` |
| `ATTACK_BLOCK` | `pos`, `face`, `subject` |
| `ATTACK_ENTITY` | `entity_id`, `subject` |
| `BLOCK_BREAK_BEFORE` | `pos`, `subject`; `Fail` останавливает разрушение |
| `BLOCK_BREAK_CANCELED` | то же, после отмены слушателем |
| `BLOCK_BREAK_AFTER` | то же, на следующем тике сервера после неотменённого разрушения |
| `PICK_ITEM` | `item` (средняя кнопка) |

`actor()` — действующий игрок (гость LAN или встроенного сервера, иначе локальный игрок); `is_client()` истинно для
*предсказания* окна о действии, которое решает сервер. Отменяйте оба, иначе сервер поправит клиента:

```rust
use fish_api::event::interaction::BLOCK_BREAK_BEFORE;
use fish_api::ActionResult;

BLOCK_BREAK_BEFORE.register(|c| {
    if c.subject() == "minecraft:bedrock" { ActionResult::Fail } else { ActionResult::Pass }
})?;
```

## Замечания

- `BLOCK_BREAK_CANCELED` и `BLOCK_BREAK_AFTER` выводит загрузчик из исхода `BLOCK_BREAK_BEFORE`. `AFTER` значит
  «разрушение не запрещено»; Horizon не сообщает о более поздней неудаче.
- `hand` — `Hand::Main` или `Hand::Off`; `face` равен `None`, если у действия нет грани.
