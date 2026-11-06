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
- **A block item** with the same id always exists; its model, texture and name come from the files below. Do not
  register an item with that id ([items](item.md)).
- **Everything else comes from the template**: its collision shape, whether it ticks randomly, its state properties.
  `.random_ticks()` and `.no_collision()` do not exist because Horizon's content declarations cannot express them:
  choose a template that already has the behaviour. A block may add [state properties](#state-properties) of its own.

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
taking as many states as its template has, times the combinations of its added state properties. `Blocks::state_range`
answers `NOT_FOUND` until the game built its states,
so ask in an event, not in `main`. A block of Fish without a state (Horizon did not add it) is logged as a warning.

## State properties

A block may add state properties to the ones of its template (Fabric's `appendProperties`): a boolean, a whole number in
a range or one of a few words.

```rust
use fish_api::block::{BlockBehaviour, BlockSettings, Blocks, StateProperty};
use fish_api::ActionResult;

fn init() -> Result<(), fish_api::Error> {
    let behaviour = BlockBehaviour::new()
        .on_use(|c| {
            if c.property_bool("lit") == Some(true) {
                let level = (c.property_int("level").unwrap_or(0) + 1) % 4;
                let _ = c.set_property("level", level);
            } else {
                let _ = c.set_property("lit", true);
            }
            ActionResult::Success
        })
        .neighbor_changed(|c| {
            let _ = c.set_property("lit", false); // written on the server's next tick
        });
    Blocks::register_with(
        "content_demo:ruby_lamp",
        BlockSettings::copy("minecraft:iron_block")
            .property(StateProperty::boolean("lit", false))
            .property(StateProperty::int("level", 0..=3, 0)),
        behaviour,
    )?;
    Ok(())
}
```

- **`StateProperty::boolean(name, default)`**, **`int(name, min..=max, default)`** and
  **`enumeration(name, values, default)`** describe a property; `BlockSettings::property` adds it. Names and values are
  `[a-z0-9_]+`; a property has 2 to 64 values, a block at most 8 properties and 4096 combinations of their values.
- **Reading**: `BlockContext::properties()` is the state's `k=v` list (the template's properties and the added ones,
  sorted by key), `property(name)`, `property_bool` and `property_int` read one.
- **Writing**: `set_property(name, value)` puts the state with one property changed where the block is,
  `state_with(name, value)` only finds its id, `set_state(id)` puts any state of the block. Writes happen on the server
  (the integrated or the dedicated one); a window gets `UNSUPPORTED`. The events of the block simulation
  (`neighbor_changed`, `random_tick`, `scheduled_tick`) have no game at hand: their writes wait for the end of the
  server's next tick.
- **States**: the block's states are the template's, each once per combination of the added values, in vanilla's
  order: every property sorted by name, the last name changing fastest. The default state has every added property at
  its default; `/setblock` and placing the block item put it. Find states by property (`state_with`), not by
  arithmetic on ids.
- **Models**: the blockstates file names the added properties like the template's
  (`"lit=true": {"model": "content_demo:block/ruby_lamp_on"}`); a block without its own blockstates file looks like its
  template in every state.
- **Saving**: saves store the property values with the block name, so a world keeps the states as long as the block
  declares the same properties.

A name the template already uses (`axis` for a log template) makes Horizon refuse the content of every mod for the run,
like any other broken declaration: pick names the template does not have. The declaration carries the properties
(`state_properties` in `data/<mod_id>/horizon/block/<path>.json`); patch `0010-block-state-properties.patch` makes
Horizon's content overlay build the states.

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
expectations `blocks`, `block_states`, `block_properties`, `use_result`, `content_files` and `no_content_files`. See
`examples/mods/content-demo`.

## Limits

- Singleplayer and the integrated server only: there is no multiplayer registry synchronisation yet.
- The only world write is the block's own state (`set_property`, `set_state`); there is no general world access yet.
- Added state properties change the state only: models follow them through the blockstates file, but shapes, light and
  the template's behaviour do not; no custom block entities, no custom shapes.
