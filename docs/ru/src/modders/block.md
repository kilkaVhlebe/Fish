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
- **Предмет-блок** с тем же id есть всегда; его модель, текстура и название берутся из файлов ниже.
- **Всё остальное берётся у шаблона**: форма столкновения, случайные тики, свойства состояния. `.random_ticks()` и
  `.no_collision()` не существуют, потому что объявления контента Horizon не могут их выразить: выберите шаблон, у
  которого нужное поведение уже есть. Собственных свойств состояния пока нет (P8.3).

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
занимает столько состояний, сколько у его шаблона. `Blocks::state_range` отвечает `NOT_FOUND`, пока игра не построила
состояния, поэтому спрашивайте в событии, а не в `main`. Блок Fish без состояний (Horizon его не добавил) попадает в
журнал как предупреждение.

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
`blocks`, `block_states`, `use_result`, `content_files` и `no_content_files`. См. `examples/mods/content-demo`.

## Ограничения

- Только одиночная игра и встроенный сервер: синхронизации реестров по сети пока нет.
- API доступа к миру для обратных вызовов пока нет: вызовы наблюдают, а решает `on_use`.
- Одно состояние на состояние шаблона, без собственных свойств; нет своих сущностей-блоков и форм.
