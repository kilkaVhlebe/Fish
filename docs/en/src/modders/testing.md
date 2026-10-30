# Testing mods

`cargo fish test` builds the mod and runs every `tests/*.toml` script in the in-process test host
(`fish-test-host`): a real loader in a temporary game directory with the simulated client. Each script gets
a fresh host. A failure prints the first failed step with *expected* and *actual*:

```text
FAILED  hello.toml
        step 1: expect.logs failed
          expected: a message containing "this is never logged"
          actual:   ["Fish 0.1.0 (ABI 0.1) found 1 mod candidate(s)", …]
```

## Script schema v1

```toml
name = "optional display name"

[[step]]
key = { name = "H", pressed = true }   # key event
command = "fish hello"                 # client command (no leading slash)
chat = "hi"                            # chat the player sends
ticks = 20                             # advance client ticks
frames = 2                             # render frames

[step.expect]
logs = ["Hello from main"]             # substrings among all logged messages
chat = []                              # substrings among chat shown to the player
hud = []                               # substrings among text drawn on the HUD
command_feedback = []                  # substrings among command feedback
stopped = ["panicky"]                  # mods that must be in the failed state
```

Unknown keys are errors. Within a step the actions run in the order `key`, `command`, `chat`, `ticks`,
`frames`; then `expect` is checked. `logs` and `stopped` observe the real loader; the other
observations (`chat`, `hud`, `command_feedback`) are fed by the client hooks of later phases, so until a mod
produces such output they report `nothing`. Later phases append keys (for example world interaction).

## From Rust tests

`fish_test_host::TestHost` is the same host as a library: `build_example`-style packing, `boot`, `fire` for
events, `share` to read the object share, `mod_log(id)`, `state(id)`, `reboot()`. The Fish repository's own
loader tests are written with it.

## Game events in scripts

Steps can raise the same game events the edition's hooks do. Within a step they run in this order: `key`, `command`,
`chat`, then `game`, `join`, `leave`, `use_block`, `attack_block`, `attack_entity`, `break_block`, `damage`, `death`,
`chat_in`, `chat_out`, `chat_received`, `server_command`, `reload`, `event`, then `ticks` (the window's game),
`server_ticks` (the integrated server) and `frames`.

```toml
[[step]]
break_block = { pos = [1, 64, 2], block = "minecraft:bedrock" }   # block_break_before
[step.expect]
outcome = "cancel"                       # outcome of the step's last explicit event
events = ["interaction/block_break_before cancel"]   # every event fired, as "group/name outcome"
no_events = ["block_break_after"]
logs = ["refusing to break bedrock"]
```

| Key | Does |
| --- | --- |
| `game = true` / `false` | starts (server starting, first server tick, first frame) / stops the game |
| `join = "Alex"`, `leave = "Alex"` | player joins or leaves |
| `use_block = { pos, block, item }` | client prediction first, then the server's decision; also decides item `use_on_block` |
| `use_item = { item, hand }`, `use_entity = { id, type, item }` | an item is used in the air / on an entity (Fish item callbacks) |
| `finish_using = { item }` | a consumable is finished (`on_finish_using`) |
| `inventory_tick = { item, slot, selected }` | one inventory tick of a stack (`on_inventory_tick`) |
| `attack_block = { pos, block }`, `attack_entity = { id, type }` | attacks |
| `break_block = { pos, block }` | `block_break_before`; `after` fires on the next `server_ticks` |
| `damage = { id, type, amount, source }`, `death = { id, type }` | entity events |
| `chat_in = { actor, text }` | a chat message reaches the server (`expect.broadcast`) |
| `chat_out = "text"` | the player types a line (`expect.sent_chat`, may be rewritten) |
| `chat_received = { text, source }` | a line arrives at the client (`expect.received`) |
| `server_command = { line, level, actor }` | a command on the server (`expect.command_feedback`, `command_ok`) |
| `command = "name"` | a client command (Fish client trees first, then raw command listeners) |
| `reload = true` | `/reload`; `reload_end` fires on the next `server_ticks` |
| `event = { kind = "interaction/pick_item", ... }` | any game event, every field settable |

`expect` additionally knows `outcome`, `events`, `no_events`, `broadcast`, `sent_chat`, `received`, `broken`
and `command_ok`. In Rust, `TestHost` has the same flows (`use_block`, `break_block`, `chat_in`, `server_command`, …) and
`game_event(&GameEvent)` for anything else; `sim.events` is the event log.

Item expectations: `items` (registered Fish items), `tooltips` (lines `append_tooltip` adds for an item),
`creative_tabs` (Fish tabs present), `creative_entries` (items present in a tab) and `no_creative_entries` (items that
must not be in a tab). See [items](item.md) and [item groups](item_group.md).
