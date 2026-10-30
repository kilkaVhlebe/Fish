# Предметы

`fish_api::item` регистрирует предметы: предмет копирует ванильный (свой **шаблон**), меняет несколько настроек и может
реагировать на события. Модели, текстуры и переводы — обычные файлы мода. Вкладки творческого режима описаны в
[группах предметов](item_group.md).

```rust
use fish_api::item::{ItemBehaviour, ItemSettings, Items, Rarity};
use fish_api::ActionResult;

fn init() -> Result<(), fish_api::Error> {
    let settings = ItemSettings::copy("minecraft:diamond")
        .max_stack(16)
        .rarity(Rarity::Rare);
    let behaviour = ItemBehaviour::new()
        .on_use(|c| {
            log::info!("ruby used by {} hand {:?}", c.actor(), c.hand());
            ActionResult::Success
        })
        .on_use_on_block(|c| {
            log::info!("ruby used on {} at {:?} face {:?}", c.target(), c.pos(), c.face());
            ActionResult::Success
        })
        .append_tooltip(|_, lines| lines.add("§7A gem of content_demo"));

    let ruby = Items::register_with("content_demo:ruby", settings, behaviour)?;
    log::info!("registered {}", ruby.id());
    Ok(())
}
```

- **`ItemSettings::copy(шаблон)`** берёт за основу ванильный предмет (`minecraft:<путь>`); `.max_stack` (1..=99) и
  `.rarity` (`Common`, `Uncommon`, `Rare`, `Epic`) меняют по одному значению. Что не задано, остаётся как у шаблона.
  Неверное значение даёт `INVALID_ARGUMENT`, загрузчик пишет причину в журнал.
- **`ItemBehaviour`** собирает обратные вызовы: `on_use`, `on_use_on_block` и `on_use_on_entity` (каждый возвращает
  `ActionResult`), `on_finish_using`, `on_inventory_tick` и `append_tooltip`. Вызов получает `ItemContext`: `client`,
  `role`, `item`, `count`, `hand`, `face`, `slot`, `selected`, `pos`, `target`, `entity_id`, `actor`, `actor_uuid` и
  `dimension`. `slot` — это `Option<ItemSlot>` (`Inventory(n)`, `Armor(n)` или `OffHand`; `None` вне тика инвентаря); `target` — тип блока или сущности,
  по которому кликнули.
- **`Items::register(id, settings)`** добавляет предмет без вызовов, `register_with` — с ними; `Item::id()` возвращает
  id.

## Правила

- **Идентификаторы** имеют вид `namespace:path`; пространство имён должно совпадать с id регистрирующего мода
  (`minecraft:` отвергается), путь состоит из `a-z`, `0-9`, `_`, `.`, `/` и `-`. Повторная регистрация даёт
  `ALREADY_EXISTS`.
- **Регистрация** разрешена, пока выполняются `pre_launch` или `main`, — та же заморозка, что у [реестров](registry.md).
- **Предмет и блок не могут иметь один id**: Horizon сам создаёт предмет каждого блока (см. [блоки](block.md)), поэтому
  `Items::register` с id блока отвергается.
- **Всё остальное берётся у шаблона**: прочность, еда, огнестойкость, остаток после использования, анимация
  использования.

## Файлы

Предметы попадают в тот же контент-мод, что и блоки, `<папка игры>/.fish/content/<id_мода>/`, который записывается при
каждом запуске: `data/<id_мода>/horizon/item/<путь>.json` на предмет с шаблоном и настройками
(`{"template": "minecraft:diamond", "max_stack_size": 16, "rarity": "rare"}`) и копия файлов самого мода. Файлы берутся
из архива мода:

```text
assets/content_demo/lang/en_us.json                  {"item.content_demo.ruby": "Ruby"}
assets/content_demo/items/ruby.json                  item definition
assets/content_demo/models/item/ruby.json
assets/content_demo/textures/item/ruby.png
```

Правила копирования (пространства имён, перезапись, очистка) те же, что у блоков.

## Какие обратные вызовы доставляет игра

| Вызов | Кто доставляет |
| --- | --- |
| `on_use`, `on_use_on_block`, `on_use_on_entity` | хук Fish Edition на `Game::use_item`, после собственного использования блока. Порядок: сначала использование блока, потом `on_use_on_block` или `on_use_on_entity` предмета, потом `on_use`. `Success`/`Consume` завершают клик, `Fail` отменяет его, `Pass` идёт дальше |
| `on_finish_using` | `finish_consuming` Horizon, поэтому только съедобное и питьё; срабатывает до уменьшения стопки, результат игнорируется |
| `on_inventory_tick` | каждый тик для каждой стопки в инвентаре, слотах брони и второй руке локального игрока. Делайте его дешёвым; результат игнорируется |
| `append_tooltip` | сборщик подсказки на клиенте, вызывается при каждой отрисовке подсказки. Строки добавляются после ванильных; коды цвета `§` работают (`§7` — серый) |

## Тестирование

В `cargo fish test` есть шаги `use_block` (также для предметов), `use_item`, `use_entity`, `finish_using` и
`inventory_tick` и ожидания `items`, `tooltips`, `creative_tabs`, `creative_entries` и `no_creative_entries`; см.
[Тестирование модов](testing.md) и `examples/mods/content-demo`.

## Ограничения

- Объявления предметов Horizon поддерживают только `max_stack_size` (1..=99) и `rarity`. Нет `max_damage`, `food`,
  `fire_resistant` и `remainder`: выберите шаблон, у которого они есть, например `minecraft:apple` для еды,
  `minecraft:diamond_pickaxe` для прочности, `minecraft:lava_bucket` для огнестойкости, `minecraft:honey_bottle` или
  рагу для остатка.
- `on_finish_using` срабатывает только для съедобного. Тиков использования и смены анимации использования нет.
- API доступа к миру для обратных вызовов пока нет: вызовы наблюдают, а решают `on_use`, `on_use_on_block` и
  `on_use_on_entity`.
- Только одиночная игра и встроенный сервер: синхронизации реестров по сети пока нет.
