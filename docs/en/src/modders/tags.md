# Tags

`fish_api::tag::TagKey` names a tag in a registry (`item`, `block`, `entity_type`, `fluid`, or a Fish registry).
`TagKey::data_path()` gives where its members are declared in a pack: `data/<ns>/tags/<registry>/<path>.json`.

```rust
use fish_api::tag::{TagKey, c};

const RUBIES: TagKey = TagKey::new("item", "mymod:rubies");
let json_at = c::ITEM_INGOTS_IRON.data_path(); // data/c/tags/item/ingots/iron.json
```

`fish_api::tag::c` has Fabric's convention tags (namespace `c`) as constants named `<REGISTRY>_<PATH>`
(`ITEM_INGOTS_IRON`, `BLOCK_ORES_COAL`); `c::ALL` lists them. The list is `crates/fish-api/src/tag/c_tags.txt`.

## Status

Names and paths only. Reading tag membership in game and tag-aware recipes need content hooks that are not built yet
(roadmap P8.2+/P9).
