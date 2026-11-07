# Тестирование модов

`cargo fish test` собирает мод и запускает каждый скрипт `tests/*.toml` во встроенном тестовом хосте
(`fish-test-host`): настоящий загрузчик во временном игровом каталоге и имитируемый клиент. Для каждого
скрипта создаётся свежий хост. При ошибке выводится первый упавший шаг с *ожидаемым* и *фактическим*:

```text
FAILED  hello.toml
        step 1: expect.logs failed
          expected: a message containing "this is never logged"
          actual:   ["Fish 0.1.0 (ABI 0.1) found 1 mod candidate(s)", …]
```

## Схема скрипта v1

```toml
name = "необязательное имя"

[[step]]
key = { name = "H", pressed = true }   # событие клавиши
command = "fish hello"                 # клиентская команда (без слеша)
chat = "hi"                            # сообщение игрока в чат
ticks = 20                             # тики клиента
frames = 2                             # кадры рендера

[step.expect]
logs = ["Hello from main"]             # подстроки среди всех записанных сообщений
chat = []                              # подстроки среди чата, показанного игроку
hud = []                               # подстроки среди текста, нарисованного на HUD
command_feedback = []                  # подстроки среди отклика команд
stopped = ["panicky"]                  # моды, которые должны быть в состоянии failed
```

Неизвестные ключи — ошибка. В шаге действия выполняются в порядке `key`, `command`, `chat`, `ticks`,
`frames`; затем проверяется `expect`. `logs` и `stopped` наблюдают настоящий загрузчик; остальные
наблюдения (`chat`, `hud`, `command_feedback`) питаются клиентскими хуками следующих фаз, поэтому пока мод
ничего не выдаёт, они сообщают `nothing`. Следующие фазы добавляют ключи (например, взаимодействие с миром).

## Из тестов на Rust

`fish_test_host::TestHost` — тот же хост как библиотека: упаковка в стиле `build_example`, `boot`, `fire`
для событий, `share` для чтения object share, `mod_log(id)`, `state(id)`, `reboot()`. Собственные тесты
загрузчика Fish написаны на нём.

## Игровые события в скриптах

Шаги могут поднимать те же игровые события, что и хуки издания. Внутри шага они идут в таком порядке: `key`,
`command`, `chat`, затем `game`, `join`, `leave`, `use_block`, `attack_block`, `attack_entity`, `break_block`,
`damage`, `death`, `chat_in`, `chat_out`, `chat_received`, `server_command`, `reload`, `event`, потом `ticks` (игра
окна), `server_ticks` (встроенный сервер) и `frames`.

```toml
[[step]]
break_block = { pos = [1, 64, 2], block = "minecraft:bedrock" }   # block_break_before
[step.expect]
outcome = "cancel"                       # исход последнего явного события шага
events = ["interaction/block_break_before cancel"]   # все события, как "группа/имя исход"
no_events = ["block_break_after"]
logs = ["refusing to break bedrock"]
```

| Ключ | Что делает |
| --- | --- |
| `game = true` / `false` | запускает (запуск сервера, первый тик сервера, первый кадр) / останавливает игру |
| `join = "Alex"`, `leave = "Alex"` | игрок входит или выходит |
| `use_block = { pos, block, item }` | сначала предсказание клиента, потом решение сервера; также решает `use_on_block` предмета |
| `use_item = { item, hand }`, `use_entity = { id, type, item }` | предмет используется в воздухе / на сущности (вызовы предметов Fish) |
| `finish_using = { item }` | съедобное доедено (`on_finish_using`) |
| `inventory_tick = { item, slot, selected }` | один тик инвентаря для стопки (`on_inventory_tick`) |
| `use_tool = { tool, block }` | `axe`, `hoe`, `shovel` или `honeycomb` на блоке с воздухом сверху, по симулированным таблицам преобразователей (`expect.tool_result`) |
| `attack_block = { pos, block }`, `attack_entity = { id, type }` | атаки |
| `break_block = { pos, block }` | `block_break_before`; `after` приходит на следующем `server_ticks` |
| `damage = { id, type, amount, source }`, `death = { id, type }` | события сущностей |
| `chat_in = { actor, text }` | сообщение чата доходит до сервера (`expect.broadcast`) |
| `chat_out = "text"` | игрок набирает строку (`expect.sent_chat`, может быть переписана) |
| `chat_received = { text, source }` | строка приходит клиенту (`expect.received`) |
| `server_command = { line, level, actor }` | команда на сервере (`expect.command_feedback`, `command_ok`) |
| `command = "name"` | клиентская команда (сначала деревья Fish, затем слушатели сырых команд) |
| `reload = true` | `/reload`; `reload_end` приходит на следующем `server_ticks` |
| `event = { kind = "interaction/pick_item", ... }` | любое игровое событие, все поля задаются |

`expect` дополнительно знает `outcome`, `events`, `no_events`, `broadcast`, `sent_chat`, `received`, `broken` и
`command_ok`. В Rust у `TestHost` те же сценарии (`use_block`, `break_block`, `chat_in`, `server_command`, …) и
`game_event(&GameEvent)` для всего остального; `sim.events` — журнал событий.

Ожидания для предметов: `items` (зарегистрированные предметы Fish), `tooltips` (строки, которые `append_tooltip`
добавляет предмету), `creative_tabs` (имеющиеся вкладки Fish), `creative_entries` (предметы, присутствующие во вкладке)
и `no_creative_entries` (предметы, которых во вкладке быть не должно). См. [предметы](item.md) и
[группы предметов](item_group.md).

Ожидания для контента: `fuels` (`"item" = тики`, 0 — не топливо), `compostables` (`"item" = 0.5`), `flammables`
(`"block" = [воспламенение, сгорание]`), `path_types` (`"block" = "DAMAGING"`), `sculk_frequencies`
(`"minecraft:eat" = 12`), `sounds` и `particles` (зарегистрированные id), `played_sounds` (подстроки
`id at x y z volume v pitch p`) и `tool_result` (что оставил `use_tool` шага, `none`, если ничего). Симулированная игра
начинает с небольших ванильных заготовок (уголь и палки горят, яблоки компостируются, топор обтёсывает дубовые брёвна,
доски горят) и применяет записи модов теми же функциями, что и edition. См. [таблицы контента](content.md),
[звуковые события](sound.md) и [типы частиц](particle.md).

Свойства состояния блоков: `block_properties = { "x y z" = "level=1,lit=true" }` проверяет состояние блока Fish в точке
(пары `k=v` по ключу). Поставленный блок начинает в состоянии по умолчанию; симулированный сервер ставит состояния,
которые задают моды, и поднимает `neighbor_changed`, `random_tick` и `scheduled_tick` без игры, как edition, поэтому их
записи появляются на следующем `server_ticks`. См. [блоки](block.md#свойства-состояния).

Ресурсы: шаг `resource_reload = true` пересобирает симулированный корень ресурсов (ванильные заготовки с папками контента
поверх) и поднимает `lifecycle/resources_reloaded` в окне; `Resources::game` читает этот корень. См.
[ресурсы](resource.md).

Добыча: `loot = { "minecraft:entities/zombie" = ["minecraft:rotten_flesh", "content_demo:ruby"] }` перечисляет по порядку
предметы записей симулированной таблицы добычи после изменений модов (заготовки: `minecraft:blocks/stone`,
`minecraft:entities/zombie`, `minecraft:blocks/dead_bush`). См. [таблицы добычи](loot.md).
