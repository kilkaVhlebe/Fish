# Loot tables

`fish_api::loot` changes loot tables, vanilla's or any mod's (Fabric's `LootTableEvents.MODIFY` and `REPLACE`): a mod
adds pools to a table or replaces it, in the data pack's JSON format.

```rust
use fish_api::loot::{LootPool, LootTables};

fn init() -> Result<(), fish_api::Error> {
    // zombies sometimes drop a ruby
    LootTables::add_pool(
        "minecraft:entities/zombie",
        &LootPool::item("content_demo:ruby").chance(0.05).to_json(),
    )?;
    // a dead bush drops a ruby instead of sticks
    LootTables::replace(
        "minecraft:blocks/dead_bush",
        &format!(
            r#"{{"type":"minecraft:block","pools":[{}]}}"#,
            LootPool::item("content_demo:ruby").to_json()
        ),
    )?;
    Ok(())
}
```

- **`LootTables::add_pool(table, json)`** appends one pool object, or a list of pools, to the table's `pools`.
- **`LootTables::replace(table, json)`** replaces the whole table.
- **`LootPool::item(id)`** writes the JSON of a common pool: one item, `.rolls(n)`, `.count(min, max)`
  (`minecraft:set_count`) and `.chance(p)` (`minecraft:random_chance`). Any pool JSON works with `add_pool`.

## Rules

- **Ids** are loot table ids: `minecraft:blocks/stone`, `minecraft:entities/zombie`, `minecraft:chests/simple_dungeon`.
  The namespace is not limited to the calling mod.
- **Registration** is allowed while `pre_launch` or `main` runs, the same freeze as [registries](registry.md). The JSON
  is checked when it is registered: pools must be objects, a table an object.
- **One replacement per table**: a second one fails with `ALREADY_EXISTS`; added pools are not limited.
- A mod's own new tables are files, not changes: `data/<mod_id>/loot_table/...` ([resources](resource.md)).

## When it is applied

Once, when the game loads its loot tables (with the item table): every replacement first, then the added pools in
registration order. The loader logs `Fish loot tables: 2 change(s)` after the entrypoints, the game
`Fish loot tables: 2 change(s) applied`; a change of a table the game does not have is skipped with a warning. Horizon
keeps the tables in `horizon-items`; patch `0012-loot-tables.patch` gives the bridge access to them.

## Limits

- The JSON is applied as it is: Horizon evaluates the vanilla functions, conditions and entries it knows; an entry it
  does not know draws nothing.
- No callback sees a table: changes are declared, not computed from the table's contents (Fabric's `MODIFY` gets a
  builder of the table; a mod reads a table with [`Resources::game`](resource.md) when it needs its contents).
- Testing: the expectation `loot = { "table" = ["item", ...] }` of [Testing mods](testing.md) lists the items a
  simulated table can drop.
