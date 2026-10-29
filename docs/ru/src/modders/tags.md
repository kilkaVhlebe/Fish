# Теги

`fish_api::tag::TagKey` называет тег в реестре (`item`, `block`, `entity_type`, `fluid` или реестр Fish).
`TagKey::data_path()` даёт путь, где участники объявляются в паке: `data/<ns>/tags/<registry>/<path>.json`.

```rust
use fish_api::tag::{TagKey, c};

const RUBIES: TagKey = TagKey::new("item", "mymod:rubies");
let json_at = c::ITEM_INGOTS_IRON.data_path(); // data/c/tags/item/ingots/iron.json
```

`fish_api::tag::c` содержит соглашения Fabric (пространство `c`) как константы `<РЕЕСТР>_<ПУТЬ>`
(`ITEM_INGOTS_IRON`, `BLOCK_ORES_COAL`); `c::ALL` перечисляет их. Список — `crates/fish-api/src/tag/c_tags.txt`.

## Состояние

Только имена и пути. Чтение состава тегов в игре и рецепты с тегами требуют хуков контента, которых ещё нет
(дорожная карта P8.2+/P9).
