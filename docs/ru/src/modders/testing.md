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
| `use_block = { pos, block, item }` | сначала предсказание клиента, потом решение сервера |
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
