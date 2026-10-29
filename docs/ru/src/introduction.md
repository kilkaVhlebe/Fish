# Fish

Fish — нативный загрузчик модов в стиле Fabric для [Horizon](https://github.com/horizon-rust/horizon),
Minecraft: Java Edition, переписанного на Rust.

- **Игроки** запускают один установщик, нажимают «Играть» на экземпляре «Fish» в лаунчере Horizon
  и кладут моды `.fish` в `fish-mods/`.
- **Моддеры** собирают моды через `cargo fish` под стабильный версионированный C ABI и слоистый API на Rust.
- Fish никогда не изменяет файлы, принадлежащие лаунчеру Horizon.

Fish не связан с Horizon, Mojang и Microsoft. [Дорожная карта](roadmap.md) показывает, что уже готово.
