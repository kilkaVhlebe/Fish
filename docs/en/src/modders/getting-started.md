# Getting started

A mod is a `cdylib` that calls `export_mod!` and ships a `fish.mod.toml`. The `hello` example
(`examples/mods/hello`) is the smallest useful one.

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
fish-api = { path = "../../crates/fish-api" }   # later: fish-api = "0.1"
log = "0.4"
```

```toml
# fish.mod.toml
schema = 1
id = "hello"
version = "0.1.0"

[library]
name = "hello"

[entrypoints]
main = ["init"]
client = ["init_client"]
```

```rust
use fish_api::event::{Combine, Event, Payload};

#[repr(C)]
pub struct Ping { pub size: u32, pub flag: u32 }
unsafe impl Payload for Ping {}

pub static PING: Event<Ping> = Event::new("hello:ping", Combine::Notify);

fn init() -> Result<(), fish_api::Error> {
    log::info!("Hello from main");          // the log crate is bridged to the host automatically
    PING.register(|_ping| { /* react */ })
}

fn init_client() { log::info!("Hello from client"); }

fish_api::export_mod! {
    main: [init],
    client: [init_client],
}
```

## What runs when

1. `pre_launch` of every mod, then `main` of every mod, then `client` (or `server`), each in load order
   (dependencies first, then by id).
2. An entrypoint is `fn()` or `fn() -> Result<(), E: Display>`. An `Err` or a panic marks the mod failed;
   mods that depend on it fail with `dependency <id> failed to load`. The game keeps running.

## Installing

Pack the mod as a `.fish` archive (`fish.mod.toml` at the root, the library under
`bin/<platform>/`, e.g. `bin/windows-x86_64/hello.dll`) and drop it into `<game dir>/fish-mods/`.
`cargo fish build` (phase P3) does this for you. For development set `FISH_LOADER_ADD_MODS` to a
path (or several, separated like `PATH`) of a `.fish` file or an unpacked directory with the same layout.

## Talking to other mods

`FishLoader::get()` gives `mods()`, `find_mod(id)`, `environment()`, `game_dir()`, `config_dir()`, an
**object share** (`put`/`get` of `&'static T` under a string key, the type name is checked on every
read) and `entrypoint_objects::<T>(key)` for `object` entrypoints of other mods
(`"my:api": object [api]` in `export_mod!`).
