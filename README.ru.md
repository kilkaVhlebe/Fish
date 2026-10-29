# Fish

Fish — загрузчик модов в стиле Fabric для [Horizon](https://github.com/horizon-rust/horizon),
Minecraft: Java Edition, переписанного на Rust.

- Игрок запускает один установщик, нажимает «Играть» на экземпляре «Fish» в лаунчере Horizon
  и кладёт моды `.fish` в `fish-mods/`.
- Моддер собирает нативные моды через `cargo fish` под документированный версионированный API.
- Fish не трогает файлы лаунчера: он подключается как обёртка запуска экземпляра.

> Fish не связан с Horizon, Mojang и Microsoft.

**Статус:** ранняя разработка. См. [дорожную карту](ROADMAP.ru.md).
English version: [README.md](README.md).

## Документация

- English: `docs/en` (mdBook)
- Русский: `docs/ru` (mdBook)

## Лицензия

MIT, см. [LICENSE](LICENSE).
