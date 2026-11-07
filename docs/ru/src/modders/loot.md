# Таблицы добычи

`fish_api::loot` меняет таблицы добычи, ванильные или любого мода (аналог `LootTableEvents.MODIFY` и `REPLACE` из
Fabric): мод добавляет в таблицу пулы или заменяет её, в JSON-формате датапака.

```rust
use fish_api::loot::{LootPool, LootTables};

fn init() -> Result<(), fish_api::Error> {
    // зомби иногда роняют рубин
    LootTables::add_pool(
        "minecraft:entities/zombie",
        &LootPool::item("content_demo:ruby").chance(0.05).to_json(),
    )?;
    // мёртвый куст роняет рубин вместо палок
    LootTables::replace(
        "minecraft:blocks/dead_bush",
        &format!(
            r#"{{"type":"minecraft:block","pools":[{}]}}"#,
            LootPool::item("content_demo:ruby").to_json()
        ),
    )?;
    Ok(())
}
```

- **`LootTables::add_pool(table, json)`** дописывает один объект пула или список пулов в `pools` таблицы.
- **`LootTables::replace(table, json)`** заменяет таблицу целиком.
- **`LootPool::item(id)`** пишет JSON обычного пула: один предмет, `.rolls(n)`, `.count(min, max)`
  (`minecraft:set_count`) и `.chance(p)` (`minecraft:random_chance`). С `add_pool` работает любой JSON пула.

## Правила

- **Идентификаторы** — id таблиц добычи: `minecraft:blocks/stone`, `minecraft:entities/zombie`,
  `minecraft:chests/simple_dungeon`. Пространство имён не ограничено вызывающим модом.
- **Регистрация** разрешена, пока выполняются `pre_launch` или `main`, — та же заморозка, что у [реестров](registry.md).
  JSON проверяется при регистрации: пулы должны быть объектами, таблица — объектом.
- **Одна замена на таблицу**: вторая даёт `ALREADY_EXISTS`; добавленные пулы не ограничены.
- Собственные новые таблицы мода — это файлы, а не изменения: `data/<id_мода>/loot_table/...` ([ресурсы](resource.md)).

## Когда применяется

Один раз, когда игра загружает таблицы добычи (вместе с таблицей предметов): сначала все замены, затем добавленные пулы в
порядке регистрации. Загрузчик пишет `Fish loot tables: 2 change(s)` после точек входа, игра —
`Fish loot tables: 2 change(s) applied`; изменение таблицы, которой в игре нет, пропускается с предупреждением. Horizon
хранит таблицы в `horizon-items`; патч `0012-loot-tables.patch` даёт мосту к ним доступ.

## Ограничения

- JSON применяется как есть: Horizon вычисляет известные ему ванильные функции, условия и записи; неизвестная запись
  ничего не даёт.
- Ни один обратный вызов не видит таблицу: изменения объявляются, а не вычисляются по содержимому таблицы (`MODIFY` в
  Fabric получает построитель таблицы; мод читает таблицу через [`Resources::game`](resource.md), если нужно её
  содержимое).
- Тестирование: ожидание `loot = { "table" = ["item", ...] }` из [Тестирования модов](testing.md) перечисляет предметы,
  которые может дать симулированная таблица.
