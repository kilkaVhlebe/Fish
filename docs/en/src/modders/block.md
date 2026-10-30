# Blocks

`fish_api::block` registers blocks: a block copies a vanilla block (its **template**), changes some settings and may react
to events. Textures, models, loot tables and translations are ordinary files of the mod.

```rust
use fish_api::block::{BlockBehaviour, BlockSettings, Blocks};
use fish_api::ActionResult;

fn init() -> Result<(), fish_api::Error> {
    let settings = BlockSettings::copy("minecraft:iron_block")
        .hardness(4.0)
        .resistance(8.0)
        .light(7)
        .sounds("minecraft:amethyst_block")
        .requires_tool(true);
    let behaviour = BlockBehaviour::new()
        .on_use(|c| {
            log::info!("{} used {} at {:?}", c.actor(), c.block(), c.pos());
            ActionResult::Success
        })
        .on_broken(|c| log::info!("{} was broken", c.block()));

    let ruby = Blocks::register_with("content_demo:ruby_block", settings, behaviour)?;
    log::info!("states {:?}", ruby.state_range()?);
    Ok(())
}
```

- **`BlockSettings::copy(template)`** starts from a vanilla block (`minecraft:<path>`); `.hardness` (`>= 0`, or `-1`
  for unbreakable), `.resistance` (`>= 0`), `.light` (0..=15), `.sounds` (a vanilla block whose sounds are taken),
  `.requires_tool`, `.friction` (0..=2) and `.map_color` (0..=61) change one value each. What is not set keeps the
  template's value. A wrong value fails the registration with `INVALID_ARGUMENT` and the loader logs why.
- **`BlockBehaviour`** collects callbacks: `on_use` (returns an `ActionResult`), `on_place`, `on_broken`,
  `neighbor_changed`, `random_tick` and `scheduled_tick`. A callback gets a `BlockContext`: `pos`, `neighbor_pos`,
  `face`, `hand`, `actor`, `actor_uuid`, `dimension`, `block`, `state_id`, `state_index`, `item`, `role` and `client`.
- **`Blocks::register(id, settings)`** adds a block without callbacks, `register_with` one with them,
  `Blocks::state_range(id)` (or `Block::state_range`) tells the state ids of a block.

## Rules

- **Ids** are `namespace:path`; the namespace must be the id of the registering mod (`minecraft:` is refused), the path
  uses `a-z`, `0-9`, `_`, `.`, `/` and `-`. A block registered twice fails with `ALREADY_EXISTS`.
- **Registration** is allowed while `pre_launch` or `main` runs, the same freeze as [registries](registry.md).
- **A block item** with the same id always exists; its model, texture and name come from the files below.
- **Everything else comes from the template**: its collision shape, whether it ticks randomly, its state properties.
  `.random_ticks()` and `.no_collision()` do not exist because Horizon's content declarations cannot express them:
  choose a template that already has the behaviour. Custom state properties are not available yet (P8.3).

## Files

Fish turns the blocks of a mod into a Horizon content mod, written to `<game dir>/.fish/content/<mod_id>/` at every
start: a `horizon-mod.toml`, a `data/<mod_id>/horizon/block/<path>.json` per block (template and settings) and a copy of
the mod's own files. The files come from the mod's archive:

```text
assets/content_demo/lang/en_us.json                  {"block.content_demo.ruby_block": "Ruby block"}
assets/content_demo/blockstates/ruby_block.json
assets/content_demo/models/block/ruby_block.json
assets/content_demo/models/item/ruby_block.json
assets/content_demo/textures/block/ruby_block.png
data/content_demo/loot_table/blocks/ruby_block.json
data/minecraft/tags/block/mineable/pickaxe.json      {"values": ["content_demo:ruby_block"]}
```

Entries under `assets/<mod_id>/`, `data/<mod_id>/` and the tag additions `data/minecraft/tags/` and `data/c/tags/` are
copied; entries of other namespaces are skipped with a warning. Files are rewritten only when their bytes change, and
files left from earlier runs are removed.

## State ids

Horizon appends the states of content blocks after vanilla's: mods in id order, blocks of a mod in id order, a block
taking as many states as its template has. `Blocks::state_range` answers `NOT_FOUND` until the game built its states,
so ask in an event, not in `main`. A block of Fish without a state (Horizon did not add it) is logged as a warning.

## Which callbacks the game delivers

| Callback | Delivered by |
| --- | --- |
| `on_use` | Fish Edition's hook on `Game::use_item`, before the generic `use_block` event; `Success`/`Consume` end the click, `Fail` cancels it, `Pass` goes on |
| `on_place` | HEAD `Game::apply_item_block_state` (a player's or a dispenser's block item) |
| `on_broken` | HEAD `Game::break_block_as_player`; reported at the end of the tick, once the block is gone |
| `neighbor_changed`, `random_tick`, `scheduled_tick` | a small Horizon patch (`0007-block-ticks.patch`) in the block simulation; they fire for any Fish block whose template runs them. Horizon names the block that changed, not where, so `neighbor_pos` is the block's own position in the game |

`entity_inside` and `stepped_on` are not available: Horizon has no hook point for them yet.

## Saving and caches

Saves store block names, so a world keeps the blocks while the mod is installed; without the mod vanilla turns them
into air. Horizon rebuilds its content overlay when a content file changes: the first start after changing blocks takes
about 30 seconds and several GB of RAM, later ones reuse the cache.

## Testing

`cargo fish test` has the steps `place_block`, `use_block`, `break_block`, `random_tick` and `scheduled_tick` and the
expectations `blocks`, `block_states`, `use_result`, `content_files` and `no_content_files`. See
`examples/mods/content-demo`.

## Limits

- Singleplayer and the integrated server only: there is no multiplayer registry synchronisation yet.
- There is no world access API for callbacks yet: callbacks observe, and `on_use` decides.
- One state per template state, no custom properties; no custom block entities, no custom shapes.
