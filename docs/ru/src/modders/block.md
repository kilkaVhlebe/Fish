# Блоки

`fish_api::block` регистрирует блоки: блок копирует ванильный (свой **шаблон**), меняет часть настроек и может
реагировать на события. Текстуры, модели, таблицы добычи и переводы — обычные файлы мода.

```rust
use fish_api::block::{BlockBehaviour, BlockSettings, Blocks};
use fish_api::ActionResult;

fn init() -> Result<(), fish_api::Error> {
    let settings = BlockSettings::copy("minecraft:iron_block")
        .hardness(4.0)
        .resistance(8.0)
        .light(7)
        .sounds("minecraft:amethyst_block")
        .requires_tool(true);
    let behaviour = BlockBehaviour::new()
        .on_use(|c| {
            log::info!("{} used {} at {:?}", c.actor(), c.block(), c.pos());
            ActionResult::Success
        })
        .on_broken(|c| log::info!("{} was broken", c.block()));

    let ruby = Blocks::register_with("content_demo:ruby_block", settings, behaviour)?;
    log::info!("states {:?}", ruby.state_range()?);
    Ok(())
}
```

- **`BlockSettings::copy(шаблон)`** берёт за основу ванильный блок (`minecraft:<путь>`); `.hardness` (`>= 0` или `-1`
  для неразрушимого), `.resistance` (`>= 0`), `.light` (0..=15), `.sounds` (ванильный блок, чьи звуки берутся),
  `.requires_tool`, `.friction` (0..=2) и `.map_color` (0..=61) меняют по одному значению. Что не задано, остаётся как у
  шаблона. Неверное значение даёт `INVALID_ARGUMENT`, загрузчик пишет причину в журнал.
- **`BlockBehaviour`** собирает обратные вызовы: `on_use` (возвращает `ActionResult`), `on_place`, `on_broken`,
  `neighbor_changed`, `random_tick` и `scheduled_tick`. Вызов получает `BlockContext`: `pos`, `neighbor_pos`, `face`,
  `hand`, `actor`, `actor_uuid`, `dimension`, `block`, `state_id`, `state_index`, `item`, `role` и `client`.
- **`Blocks::register(id, settings)`** добавляет блок без вызовов, `register_with` — с ними, `Blocks::state_range(id)`
  (или `Block::state_range`) сообщает номера состояний блока.

## Правила

- **Идентификаторы** имеют вид `namespace:path`; пространство имён должно совпадать с id регистрирующего мода
  (`minecraft:` отвергается), путь состоит из `a-z`, `0-9`, `_`, `.`, `/` и `-`. Повторная регистрация даёт
  `ALREADY_EXISTS`.
- **Регистрация** разрешена, пока выполняются `pre_launch` или `main`, — та же заморозка, что у [реестров](registry.md).
- **Предмет-блок** с тем же id есть всегда; его модель, текстура и название берутся из файлов ниже. Не регистрируйте
  предмет с таким id ([предметы](item.md)).
- **Всё остальное берётся у шаблона**: форма столкновения, случайные тики, свойства состояния. `.random_ticks()` и
  `.no_collision()` не существуют, потому что объявления контента Horizon не могут их выразить: выберите шаблон, у
  которого нужное поведение уже есть. Блок может добавить [свои свойства состояния](#свойства-состояния).

## Файлы

Fish превращает блоки мода в контент-мод Horizon и при каждом запуске записывает его в
`<папка игры>/.fish/content/<id_мода>/`: `horizon-mod.toml`, по файлу `data/<id_мода>/horizon/block/<путь>.json` на блок
(шаблон и настройки) и копию файлов самого мода. Файлы берутся из архива мода:

```text
assets/content_demo/lang/en_us.json                  {"block.content_demo.ruby_block": "Ruby block"}
assets/content_demo/blockstates/ruby_block.json
assets/content_demo/models/block/ruby_block.json
assets/content_demo/models/item/ruby_block.json
assets/content_demo/textures/block/ruby_block.png
data/content_demo/loot_table/blocks/ruby_block.json
data/minecraft/tags/block/mineable/pickaxe.json      {"values": ["content_demo:ruby_block"]}
```

Копируются записи `assets/<id_мода>/`, `data/<id_мода>/` и дополнения тегов `data/minecraft/tags/` и `data/c/tags/`;
записи других пространств имён пропускаются с предупреждением. Файл переписывается, только если изменились его байты, а
оставшиеся от прошлых запусков файлы удаляются.

## Номера состояний

Horizon дописывает состояния блоков контента после ванильных: моды по порядку id, блоки мода по порядку id, блок
занимает столько состояний, сколько у его шаблона, умноженное на число комбинаций добавленных свойств.
`Blocks::state_range` отвечает `NOT_FOUND`, пока игра не построила
состояния, поэтому спрашивайте в событии, а не в `main`. Блок Fish без состояний (Horizon его не добавил) попадает в
журнал как предупреждение.

## Свойства состояния

Блок может добавить свойства состояния к свойствам шаблона (аналог `appendProperties` в Fabric): логическое, целое
число в диапазоне или одно из нескольких слов.

```rust
use fish_api::block::{BlockBehaviour, BlockSettings, Blocks, StateProperty};
use fish_api::ActionResult;

fn init() -> Result<(), fish_api::Error> {
    let behaviour = BlockBehaviour::new()
        .on_use(|c| {
            if c.property_bool("lit") == Some(true) {
                let level = (c.property_int("level").unwrap_or(0) + 1) % 4;
                let _ = c.set_property("level", level);
            } else {
                let _ = c.set_property("lit", true);
            }
            ActionResult::Success
        })
        .neighbor_changed(|c| {
            let _ = c.set_property("lit", false); // записывается на следующем тике сервера
        });
    Blocks::register_with(
        "content_demo:ruby_lamp",
        BlockSettings::copy("minecraft:iron_block")
            .property(StateProperty::boolean("lit", false))
            .property(StateProperty::int("level", 0..=3, 0)),
        behaviour,
    )?;
    Ok(())
}
```

- **`StateProperty::boolean(name, default)`**, **`int(name, min..=max, default)`** и
  **`enumeration(name, values, default)`** описывают свойство; `BlockSettings::property` добавляет его. Имена и значения
  — `[a-z0-9_]+`; у свойства от 2 до 64 значений, у блока не больше 8 свойств и 4096 комбинаций их значений.
- **Чтение**: `BlockContext::properties()` — список `k=v` состояния (свойства шаблона и добавленные, по ключу),
  `property(name)`, `property_bool` и `property_int` читают одно.
- **Запись**: `set_property(name, value)` ставит на место блока состояние с одним изменённым свойством,
  `state_with(name, value)` только находит его id, `set_state(id)` ставит любое состояние блока. Запись идёт на сервере
  (встроенном или выделенном); окно получает `UNSUPPORTED`. У событий симуляции блоков (`neighbor_changed`,
  `random_tick`, `scheduled_tick`) нет игры под рукой: их записи ждут конца следующего тика сервера.
- **Состояния**: состояния блока — это состояния шаблона, каждое по разу на комбинацию добавленных значений, в порядке
  ванили: все свойства по имени, последнее имя меняется быстрее всего. В состоянии по умолчанию все добавленные
  свойства на своих значениях по умолчанию; его ставят `/setblock` и предмет-блок. Ищите состояния по свойству
  (`state_with`), а не арифметикой над id.
- **Модели**: файл blockstates называет добавленные свойства так же, как свойства шаблона
  (`"lit=true": {"model": "content_demo:block/ruby_lamp_on"}`); блок без своего файла blockstates во всех состояниях
  выглядит как шаблон.
- **Сохранение**: сохранения хранят значения свойств вместе с именем блока, так что мир сохраняет состояния, пока блок
  объявляет те же свойства.

Имя, которое уже есть у шаблона (`axis` у шаблона-бревна), заставляет Horizon отказаться от контента всех модов на этот
запуск, как и любое другое сломанное объявление: выбирайте имена, которых у шаблона нет. Свойства передаются в
объявлении (`state_properties` в `data/<id_мода>/horizon/block/<path>.json`); патч `0010-block-state-properties.patch`
учит оверлей контента Horizon строить состояния.

## Какие обратные вызовы доставляет игра

| Вызов | Кто доставляет |
| --- | --- |
| `on_use` | хук Fish Edition на `Game::use_item`, раньше общего события `use_block`; `Success`/`Consume` завершают клик, `Fail` отменяет его, `Pass` идёт дальше |
| `on_place` | HEAD `Game::apply_item_block_state` (блок-предмет игрока или раздатчика) |
| `on_broken` | HEAD `Game::break_block_as_player`; сообщается в конце тика, когда блока уже нет |
| `neighbor_changed`, `random_tick`, `scheduled_tick` | небольшой патч Horizon (`0007-block-ticks.patch`) в симуляции блоков; срабатывают для любого блока Fish, чей шаблон их выполняет. Horizon называет блок, который изменился, но не место, поэтому в игре `neighbor_pos` совпадает с позицией самого блока |

`entity_inside` и `stepped_on` недоступны: в Horizon для них пока нет точки хука.

## Сохранение и кэши

Сохранение хранит названия блоков, поэтому мир сохраняет блоки, пока мод установлен; без мода ванильная игра превращает
их в воздух. Horizon пересобирает оверлей контента, когда меняется файл контента: первый запуск после изменения блоков
занимает около 30 секунд и несколько ГБ памяти, следующие используют кэш.

## Тестирование

В `cargo fish test` есть шаги `place_block`, `use_block`, `break_block`, `random_tick` и `scheduled_tick` и ожидания
`blocks`, `block_states`, `block_properties`, `use_result`, `content_files` и `no_content_files`. См.
`examples/mods/content-demo`.

## Ограничения

- Только одиночная игра и встроенный сервер: синхронизации реестров по сети пока нет.
- Единственная запись в мир — собственное состояние блока (`set_property`, `set_state`); общего доступа к миру пока нет.
- Добавленные свойства меняют только состояние: модели следуют им через файл blockstates, а форма, свет и поведение
  шаблона — нет; нет своих сущностей-блоков и форм.
