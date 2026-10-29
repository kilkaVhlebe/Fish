# Инструменты: `cargo fish`

`cargo-fish` — аналог Fabric Loom для Fish. Установите: `cargo install --path crates/cargo-fish`
(из репозитория Fish) и запускайте как `cargo fish <команда>`. Ремаппинг не нужен: API стабилен.

## `cargo fish new <name>`

```text
cargo fish new my-mod [--template basic|client|content] [--fish-path <каталог Fish>]
```

Создаёт `Cargo.toml` (`crate-type = ["cdylib"]`, `[profile.release] panic = "unwind"`), `fish.mod.toml`,
`src/lib.rs` с `export_mod!`, `tests/hello.toml` и `assets/<id>/lang/en_us.json`. Имя крейта — в нижнем
регистре; id мода — имя с заменой `-` на `_`, он должен совпадать с именем библиотеки.

- `basic`: точка входа `main`. `client`: `main` и `client`.
- `content`: только ассеты и данные, **без Cargo-проекта и без библиотеки**.
- С `--fish-path` зависимость `fish-api` — путь; без него — версия с crates.io, совпадающая с вашим `cargo-fish`.

## `cargo fish build [--release] [--target-platform <key>]`

1. `cargo build` (сообщения JSON) для проектов с `[library]`.
2. Отказывает при `panic = "abort"` (`[profile.*]` корня workspace или `CARGO_PROFILE_*_PANIC`).
3. Проверяет `fish.mod.toml` и что имя собранной библиотеки соответствует `[library] name`.
4. Пишет `target/fish/<id>-<version>.fish` с `bin/<platform>/`, `assets/`, `data/`, иконкой и вложенными архивами.

`--target-platform` принимает ключ платформы (`windows-x86_64`, `linux-x86_64`, `macos-aarch64`, …) и передаёт
cargo соответствующий `--target`.

## `cargo fish check <file.fish|dir>`

Запускает временный хост только с этим модом **в дочернем процессе**, поэтому падающая библиотека не
уронит инструмент. Печатает загруженные моды, предупреждения и ошибки; при любой ошибке код выхода не нулевой.

## `cargo fish test [filter]`

Собирает мод и запускает скрипты `tests/*.toml`; см. [Тестирование](testing.md).

## Запуск в игре

Положите `.fish` в `<game dir>/fish-mods/` или укажите путь в `FISH_LOADER_ADD_MODS`. Переменные окружения:

| Переменная | Значение |
| --- | --- |
| `FISH_LOADER_ADD_MODS` | дополнительные пути модов (как `PATH`), загружаются как dev-моды |
| `FISH_LOADER_DEV` | `1`: режим разработки |
| `FISH_LOADER_STRICT` | `1`: паника мода завершает процесс вместо отключения мода |
