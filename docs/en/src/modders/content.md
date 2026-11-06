# Content tables

`fish_api::content::Content` adds entries to the tables the game keeps about items, blocks and game events: furnace
fuel, composting, fire, what an axe, hoe or shovel does to a block, how mobs path over a block and how a sculk sensor
hears a game event (Fabric's `FuelRegistry`, `CompostingChanceRegistry`, `FlammableBlockRegistry`,
`StrippableBlockRegistry`, `TillableBlockRegistry`, `FlattenableBlockRegistry`, `OxidizableBlocksRegistry` and
`SculkSensorFrequencyRegistry`).

```rust
use fish_api::content::{Content, PathNodeType};

fn init() -> Result<(), fish_api::Error> {
    Content::fuel("content_demo:ruby", 2400)?;                  // burns 2400 ticks (12 items)
    Content::compostable("content_demo:ruby_snack", 0.5)?;      // 50 % to add a composter layer
    Content::flammable("content_demo:ruby_log", 30, 60)?;       // ignite odds, burn odds
    Content::strippable("content_demo:ruby_log", "content_demo:stripped_ruby_log")?;
    Content::waxable("content_demo:ruby_block", "content_demo:waxed_ruby_block")?;
    Content::path_node_type("content_demo:raw_ruby", PathNodeType::Damaging)?;
    Content::sculk_frequency("minecraft:eat", 12)?;
    Ok(())
}
```

| Call | What the game does |
| --- | --- |
| `fuel(item, ticks)` / `remove_fuel(item)` | a furnace burns the item `ticks` ticks (`1..=1_000_000`; coal is 1600); removing makes a vanilla item stop burning |
| `compostable(item, chance)` / `remove_compostable(item)` | a composter takes the item and adds a layer with probability `chance` in `(0, 1]` (whole percent); the first item into an empty composter always adds one, as in vanilla |
| `flammable(block, ignite, burn)` / `fireproof(block)` | fire spreads onto the block with `ignite` odds and consumes it with `burn` odds (`0..=255`; planks are 5 and 20); `fireproof` sets both to zero |
| `strippable(block, stripped)` | an axe turns `block` into `stripped`, keeping the properties both have (the axis of a log) |
| `tillable(block, tilled)` | a hoe turns `block` into `tilled` when the block above is air, not from below |
| `flattenable(block, flattened)` | a shovel does the same as a hoe, for paths |
| `waxable(unwaxed, waxed)` | honeycomb turns `unwaxed` into `waxed`; an axe scrapes the wax off again |
| `oxidizable(less, more)` | an axe scrapes `more` back into `less` |
| `path_node_type(block, kind)` | mobs see every state of the block as `kind` (`Damaging`, `Walkable`, `Blocked`, …) when they plan a path |
| `sculk_frequency(event, frequency)` | a sculk sensor hears the game event at `frequency` (`0..=15`; zero: no vibration) |

## Rules

- **Ids** are any item, block or game event, vanilla or from any mod: the namespace is *not* limited to the calling mod,
  because changing `minecraft:stick` is how a mod changes the vanilla game.
- **One entry per key**: the first registration of an item, block, tool action or game event wins; a second one fails
  with `ALREADY_EXISTS` and the loader logs who registered it first.
- **Registration** is allowed while `pre_launch` or `main` runs, the same freeze as [registries](registry.md).
- A mod that stops loses its entries: the tables the game builds afterwards leave them out.

## When it is applied

Once, while the game loads its resources, in the game's own tables (patch `0009-content-tables.patch`, see
[Fish Edition](../architecture/edition.md)): the fuel and compostable components of the item table, the axe, hoe and
shovel transformer files (the mods' actions come after the vanilla ones, so a vanilla action wins where both match), the
fire odds of blocks, the pathfinding types of the mob data and the vibration frequencies. When the entrypoints are over
the loader logs one line:

```text
Fish content tables: 1 fuel(s), 1 compostable item(s), 2 flammable block(s), 2 tool action(s), 1 path type(s), 1 sculk frequency(ies), 1 sound event(s), 1 particle type(s)
```

and the game logs what it applied, for example `Fish content tables: 2 axe action(s) added`. An item or block the game
does not know is skipped with a warning.

## Limits

- Horizon ages only vanilla copper: `oxidizable` adds the axe's scraping, but a block of a mod never oxidizes by itself.
- Horizon knows only the vanilla game events, so `sculk_frequency` changes the frequency of those.
- Fire odds change the fire spread of the block simulation; the client's fire visuals next to a block (the flames
  drawn on its sides) still follow the vanilla table.
- Everything is fixed for the run: there is no reload of the tables.
- Testing: the step `use_tool` and the expectations `fuels`, `compostables`, `flammables`, `path_types`,
  `sculk_frequencies` and `tool_result` of [Testing mods](testing.md).
