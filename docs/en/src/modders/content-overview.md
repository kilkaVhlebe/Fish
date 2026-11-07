# Content

Phase P8 lets a mod add game content: things the game knows by identifier and keeps in its own tables. Everything is
registered while `pre_launch` or `main` runs; after the entrypoints the registries are frozen.

| What | API | Page |
| --- | --- | --- |
| Fish registries (raw ids synchronised with a server) | `fish_api::registry` | [Registries](registry.md) |
| Tags and the convention tags (`c:`) | `fish_api::tag` | [Tags](tags.md) |
| Game rules | `fish_api::game_rule` | [Game rules](game_rule.md) |
| Blocks, with state properties of their own | `fish_api::block` | [Blocks](block.md) |
| Items | `fish_api::item` | [Items](item.md) |
| Creative tabs | `fish_api::item_group` | [Item groups](item_group.md) |
| Fuel, composting, fire, tool actions, path types, vibrations | `fish_api::content` | [Content tables](content.md) |
| Sound events | `fish_api::sound` | [Sound events](sound.md) |
| Particle types | `fish_api::particle` | [Particle types](particle.md) |

## How content reaches the game

Horizon has no code registries: its blocks and items are tables exported from vanilla. A block or item of a mod copies a
vanilla *template* and differs from it by data, so the loader writes a Horizon content mod for every Fish mod with content
(`<game dir>/.fish/content/<mod_id>/`: declarations, the mod's `assets/` and `data/`), and Horizon builds its content
overlay from them (patch `0006`). The tables Horizon builds while it loads its resources take the rest: the creative
inventory (patch `0008`), the content tables (patch `0009`) and the states of blocks with state properties (patch
`0010`). Events of blocks come from hooks and from the block simulation (patch `0007`). See
[Fish Edition](../architecture/edition.md) for the full hook map.

The loader logs what it registered once the entrypoints are over, for example

```text
Fish blocks: 6 block(s), states 35723..35740
Fish creative inventory: 1 tab(s), 1 addition(s)
Fish content tables: 1 fuel(s), 1 compostable item(s), 2 flammable block(s), 2 tool action(s), 1 path type(s), 1 sculk frequency(ies), 1 sound event(s), 1 particle type(s)
```

## The example

`examples/mods/content-demo` uses every part of the phase: two plain blocks, logs an axe strips, a block honeycomb
waxes, a lamp with the state properties `lit` and `level`, two items, a creative tab, fuel, composting, fire odds, a
path type, a vibration frequency, a sound event with its `.ogg` and a particle type. Its scripts in `tests/` run in the
simulation (`cargo fish test`, `cargo test -p fish-test-host`), the edition's self test checks its registrations in the
real binary (`cargo xtask edition selftest`), and the in-game test (`cargo xtask e2e`) places, uses, strips, waxes,
lights and saves its blocks in a running game.

## Testing

The script keys of the phase are listed in [Testing mods](testing.md): `items`, `tooltips`, `creative_*`, `blocks`,
`block_states`, `block_properties`, `use_result`, `content_files`, the content tables (`fuels`, `compostables`,
`flammables`, `path_types`, `sculk_frequencies`, `tool_result`), `sounds`, `played_sounds`, `particles` and
`gamerules`.

## Limits of the phase

- Content is fixed for a run: there is no reload of registries or tables.
- Registry synchronisation over the network is implemented but has not been tried between two real game instances.
- Particle types have no client factory (P10), so nothing draws them; sounds play on the player's own window only.
- Blocks copy a vanilla template: shapes, block entities and behaviour classes come from it; state properties change
  the state and the model, not the behaviour.
