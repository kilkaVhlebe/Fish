# Настройка окружения

## Windows

1. Установите Rust 1.99.1 (MSVC) через `rustup`; версия закреплена в `rust-toolchain.toml`.
2. Установите Visual Studio 2022 с нагрузкой C++ и Windows SDK.
3. `cargo install mdbook cargo-llvm-cov cargo-deny`

## Linux

`sudo apt-get install libasound2-dev libudev-dev`, затем те же `cargo install`.

## Рабочий процесс

```text
cargo xtask ci         # fmt, clippy -D warnings, тесты, проверки конфига и документации
cargo xtask coverage   # пороги покрытия по крейтам из fish.toml
```

Один коммит на пункт дорожной карты, сообщение `P<n>.<m>: <описание>`. Отмечайте пункт в обоих
`docs/en/src/roadmap.md` и `docs/ru/src/roadmap.md`; каждая страница существует на двух языках.
