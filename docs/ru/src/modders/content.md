# Таблицы контента

`fish_api::content::Content` добавляет записи в таблицы, которые игра ведёт о предметах, блоках и игровых событиях:
топливо печи, компостирование, огонь, что топор, мотыга или лопата делают с блоком, как мобы прокладывают путь по блоку
и как датчик скалка слышит игровое событие (аналоги `FuelRegistry`, `CompostingChanceRegistry`,
`FlammableBlockRegistry`, `StrippableBlockRegistry`, `TillableBlockRegistry`, `FlattenableBlockRegistry`,
`OxidizableBlocksRegistry` и `SculkSensorFrequencyRegistry` из Fabric).

```rust
use fish_api::content::{Content, PathNodeType};

fn init() -> Result<(), fish_api::Error> {
    Content::fuel("content_demo:ruby", 2400)?;                  // горит 2400 тиков (12 предметов)
    Content::compostable("content_demo:ruby_snack", 0.5)?;      // 50 % добавить слой в компостер
    Content::flammable("content_demo:ruby_log", 30, 60)?;       // шанс воспламенения, шанс сгорания
    Content::strippable("content_demo:ruby_log", "content_demo:stripped_ruby_log")?;
    Content::waxable("content_demo:ruby_block", "content_demo:waxed_ruby_block")?;
    Content::path_node_type("content_demo:raw_ruby", PathNodeType::Damaging)?;
    Content::sculk_frequency("minecraft:eat", 12)?;
    Ok(())
}
```

| Вызов | Что делает игра |
| --- | --- |
| `fuel(item, ticks)` / `remove_fuel(item)` | печь жжёт предмет `ticks` тиков (`1..=1_000_000`; уголь — 1600); удаление лишает ванильный предмет свойства топлива |
| `compostable(item, chance)` / `remove_compostable(item)` | компостер принимает предмет и добавляет слой с вероятностью `chance` из `(0, 1]` (в целых процентах); первый предмет в пустой компостер всегда добавляет слой, как в ванили |
| `flammable(block, ignite, burn)` / `fireproof(block)` | огонь перекидывается на блок с шансом `ignite` и сжигает его с шансом `burn` (`0..=255`; у досок 5 и 20); `fireproof` ставит оба в ноль |
| `strippable(block, stripped)` | топор превращает `block` в `stripped`, сохраняя общие свойства (ось бревна) |
| `tillable(block, tilled)` | мотыга превращает `block` в `tilled`, если сверху воздух, и не снизу |
| `flattenable(block, flattened)` | лопата делает то же, что мотыга, для тропинок |
| `waxable(unwaxed, waxed)` | соты превращают `unwaxed` в `waxed`; топор снова соскабливает воск |
| `oxidizable(less, more)` | топор соскабливает `more` обратно в `less` |
| `path_node_type(block, kind)` | мобы видят каждое состояние блока как `kind` (`Damaging`, `Walkable`, `Blocked`, …), когда строят путь |
| `sculk_frequency(event, frequency)` | датчик скалка слышит игровое событие на частоте `frequency` (`0..=15`; ноль — нет вибрации) |

## Правила

- **Идентификаторы** — любой предмет, блок или игровое событие, ванильное или любого мода: пространство имён *не*
  ограничено вызывающим модом, ведь изменить `minecraft:stick` — это и есть способ изменить ванильную игру.
- **Одна запись на ключ**: побеждает первая регистрация предмета, блока, действия инструмента или игрового события;
  повторная даёт `ALREADY_EXISTS`, а загрузчик пишет в журнал, кто зарегистрировал первым.
- **Регистрация** разрешена, пока выполняются `pre_launch` или `main`, — та же заморозка, что у [реестров](registry.md).
- Остановившийся мод теряет свои записи: таблицы, которые игра строит после этого, их не содержат.

## Когда применяется

Один раз, пока игра загружает ресурсы, в собственных таблицах игры (патч `0009-content-tables.patch`, см.
[Fish Edition](../architecture/edition.md)): компоненты топлива и компоста в таблице предметов, файлы преобразователей
топора, мотыги и лопаты (действия модов идут после ванильных, так что ванильное действие побеждает, где совпадают оба),
шансы огня у блоков, типы путей в данных мобов и частоты вибраций. Когда точки входа закончились, загрузчик пишет одну
строку:

```text
Fish content tables: 1 fuel(s), 1 compostable item(s), 2 flammable block(s), 2 tool action(s), 1 path type(s), 1 sculk frequency(ies), 1 sound event(s), 1 particle type(s)
```

а игра пишет, что применила, например `Fish content tables: 2 axe action(s) added`. Предмет или блок, которого игра не
знает, пропускается с предупреждением.

## Ограничения

- Horizon старит только ванильную медь: `oxidizable` добавляет соскабливание топором, но блок мода сам не окисляется.
- Horizon знает только ванильные игровые события, поэтому `sculk_frequency` меняет частоту именно их.
- Шансы огня меняют распространение огня в симуляции блоков; визуальный огонь клиента рядом с блоком (языки пламени на
  его сторонах) по-прежнему следует ванильной таблице.
- Всё фиксируется на время запуска: перезагрузки таблиц нет.
- Тестирование: шаг `use_tool` и ожидания `fuels`, `compostables`, `flammables`, `path_types`, `sculk_frequencies` и
  `tool_result` из [Тестирования модов](testing.md).
