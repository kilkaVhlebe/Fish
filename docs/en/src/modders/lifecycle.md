# Lifecycle events

`fish_api::event::lifecycle`. Every event here carries a [`GameContext`](events.md#game-events) unless noted; the
tick events carry a `TickContext`.

| Event | When | Fields |
| --- | --- | --- |
| `client::START_TICK`, `END_TICK` | every tick of the window's game (in a world) | `TickContext` |
| `client::STARTED` | the window's first frame (title screen included) | role |
| `client::STOPPING` | the window is closing, the world is not saved yet | role |
| `client::STOPPED` | the window closed and saved; **no game view** (`client()` fails) | role |
| `server::START_TICK`, `END_TICK` | every tick of an integrated or dedicated server | `TickContext` |
| `server::STARTING` | the server begins loading its world | `subject` = world name |
| `server::STARTED` | the server's first tick | role |
| `server::STOPPING` | the server begins to stop, players are not saved yet | role |
| `server::STOPPED` | the server saved and closed | role |
| `world::LOAD` | a level (dimension) became loaded | `dimension` |
| `world::UNLOAD` | a level is about to be unloaded; all fire when the server stops | `dimension` |
| `chunk::LOAD`, `UNLOAD` | a chunk enters or leaves the loaded set; **no game view** | `pos()` = chunk `x`, `z` |
| `reload::START` | `/reload` runs | role |
| `reload::END` | the first server tick after `START` (the data is ready) | role |

```rust
use fish_api::event::lifecycle::{server, world};

server::STARTED.register(|c| log::info!("server is up ({:?})", c.role()))?;
world::LOAD.register(|c| log::info!("level {} loaded", c.dimension()))?;
```

## Notes

- Single player runs *two* games: the window (`Role::Window`) and an integrated server (`Role::IntegratedServer`).
  `client::*` fire in the window, `server::*` in the server. A dedicated server only has `server::*`.
- `client::STARTED`, `server::STARTED` and the first `world::LOAD` are derived by the loader from the first
  frame and the first server tick; they fire exactly once per process.
- `world::LOAD` for the first level fires at the server's first tick, before `server::STARTED`; further levels fire
  as the game opens them. `world::UNLOAD` is reported for every loaded level when the server stops (levels the game
  closes while idle are not reported).
- `chunk::*` fire from a hot path: Fish skips them entirely when no mod listens. Keep listeners cheap. The dimension is
  not known there; use `pos()` only.
- `reload::END` is observed on the next server tick, not synchronously.
