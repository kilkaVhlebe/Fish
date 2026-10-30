# Items

`fish_api::item` registers items: an item copies a vanilla item (its **template**), changes a few settings and may react
to events. Models, textures and translations are ordinary files of the mod. Creative tabs are described in
[item groups](item_group.md).

```rust
use fish_api::item::{ItemBehaviour, ItemSettings, Items, Rarity};
use fish_api::ActionResult;

fn init() -> Result<(), fish_api::Error> {
    let settings = ItemSettings::copy("minecraft:diamond")
        .max_stack(16)
        .rarity(Rarity::Rare);
    let behaviour = ItemBehaviour::new()
        .on_use(|c| {
            log::info!("ruby used by {} hand {:?}", c.actor(), c.hand());
            ActionResult::Success
        })
        .on_use_on_block(|c| {
            log::info!("ruby used on {} at {:?} face {:?}", c.target(), c.pos(), c.face());
            ActionResult::Success
        })
        .append_tooltip(|_, lines| lines.add("§7A gem of content_demo"));

    let ruby = Items::register_with("content_demo:ruby", settings, behaviour)?;
    log::info!("registered {}", ruby.id());
    Ok(())
}
```

- **`ItemSettings::copy(template)`** starts from a vanilla item (`minecraft:<path>`); `.max_stack` (1..=99) and
  `.rarity` (`Common`, `Uncommon`, `Rare`, `Epic`) change one value each. What is not set keeps the template's value. A
  wrong value fails the registration with `INVALID_ARGUMENT` and the loader logs why.
- **`ItemBehaviour`** collects callbacks: `on_use`, `on_use_on_block` and `on_use_on_entity` (each returns an
  `ActionResult`), `on_finish_using`, `on_inventory_tick` and `append_tooltip`. A callback gets an `ItemContext`:
  `client`, `role`, `item`, `count`, `hand`, `face`, `slot`, `selected`, `pos`, `target`, `entity_id`, `actor`,
  `actor_uuid` and `dimension`. `slot` is an `Option<ItemSlot>` (`Inventory(n)`, `Armor(n)` or `OffHand`; `None` outside the inventory tick); `target` is the block
  or entity type that was clicked.
- **`Items::register(id, settings)`** adds an item without callbacks, `register_with` one with them; `Item::id()` gives
  the id back.

## Rules

- **Ids** are `namespace:path`; the namespace must be the id of the registering mod (`minecraft:` is refused), the path
  uses `a-z`, `0-9`, `_`, `.`, `/` and `-`. An item registered twice fails with `ALREADY_EXISTS`.
- **Registration** is allowed while `pre_launch` or `main` runs, the same freeze as [registries](registry.md).
- **An item and a block cannot share an id**: Horizon creates the item of every block itself (see
  [blocks](block.md)), so `Items::register` with a block's id is refused.
- **Everything else comes from the template**: durability, food, fire resistance, remainders, the use animation.

## Files

Items go to the same content mod as blocks, `<game dir>/.fish/content/<mod_id>/`, written at every start: a
`data/<mod_id>/horizon/item/<path>.json` per item with the template and the settings (`{"template": "minecraft:diamond",
"max_stack_size": 16, "rarity": "rare"}`), plus a copy of the mod's own files. The files come from the mod's archive:

```text
assets/content_demo/lang/en_us.json                  {"item.content_demo.ruby": "Ruby"}
assets/content_demo/items/ruby.json                  item definition
assets/content_demo/models/item/ruby.json
assets/content_demo/textures/item/ruby.png
```

The copy rules (namespaces, rewriting, cleanup) are the same as for blocks.

## Which callbacks the game delivers

| Callback | Delivered by |
| --- | --- |
| `on_use`, `on_use_on_block`, `on_use_on_entity` | Fish Edition's hook on `Game::use_item`, after the block's own use. Order: the block's use first, then the item's `on_use_on_block` or `on_use_on_entity`, then `on_use`. `Success`/`Consume` end the click, `Fail` cancels it, `Pass` goes on |
| `on_finish_using` | Horizon's `finish_consuming`, so only consumables (eating, drinking); it fires before the stack shrinks, and its result is ignored |
| `on_inventory_tick` | every tick, for each stack in the inventory, armor slots and off hand of the local player. Keep it cheap; the result is ignored |
| `append_tooltip` | the client's tooltip builder, called whenever a tooltip is drawn. Lines are added after the vanilla ones; `§` colour codes work (`§7` is gray) |

## Testing

`cargo fish test` has the steps `use_block` (also for items), `use_item`, `use_entity`, `finish_using` and
`inventory_tick` and the expectations `items`, `tooltips`, `creative_tabs`, `creative_entries` and
`no_creative_entries`; see [Testing mods](testing.md) and `examples/mods/content-demo`.

## Limits

- Horizon's item declarations support only `max_stack_size` (1..=99) and `rarity`. There is no `max_damage`, `food`,
  `fire_resistant` or `remainder`: choose a template that has them, for instance `minecraft:apple` for food,
  `minecraft:diamond_pickaxe` for durability, `minecraft:lava_bucket` for fire resistance, `minecraft:honey_bottle` or
  a stew for a remainder.
- `on_finish_using` fires only for consumables. There are no use ticks and no way to change the use animation.
- There is no world access API for callbacks yet: callbacks observe, and `on_use`, `on_use_on_block` and
  `on_use_on_entity` decide.
- Singleplayer and the integrated server only: there is no multiplayer registry synchronisation yet.
