# Быстрый старт

Мод — это `cdylib`, вызывающий `export_mod!`, и файл `fish.mod.toml`. Пример `hello`
(`examples/mods/hello`) — самый маленький полезный мод.

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
fish-api = { path = "../../crates/fish-api" }   # позже: fish-api = "0.1"
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
    log::info!("Hello from main");          // крейт log автоматически подключён к хосту
    PING.register(|_ping| { /* реакция */ })
}

fn init_client() { log::info!("Hello from client"); }

fish_api::export_mod! {
    main: [init],
    client: [init_client],
}
```

## Что и когда выполняется

1. `pre_launch` всех модов, затем `main` всех модов, затем `client` (или `server`), каждый раз в порядке
   загрузки (сначала зависимости, затем по id).
2. Точка входа — `fn()` или `fn() -> Result<(), E: Display>`. `Err` или паника помечают мод как failed;
   зависящие от него моды падают с `dependency <id> failed to load`. Игра продолжает работать.

## Установка

Упакуйте мод в архив `.fish` (`fish.mod.toml` в корне, библиотека в `bin/<platform>/`, например
`bin/windows-x86_64/hello.dll`) и положите в `<game dir>/fish-mods/`. `cargo fish build` (фаза P3)
делает это за вас. Для разработки задайте `FISH_LOADER_ADD_MODS` — путь (или несколько, как в `PATH`)
к `.fish`-файлу или распакованному каталогу с той же структурой.

## Общение с другими модами

`FishLoader::get()` даёт `mods()`, `find_mod(id)`, `environment()`, `game_dir()`, `config_dir()`,
**object share** (`put`/`get` значений `&'static T` по строковому ключу, имя типа проверяется при каждом
чтении) и `entrypoint_objects::<T>(key)` для `object`-точек входа других модов
(`"my:api": object [api]` в `export_mod!`).
