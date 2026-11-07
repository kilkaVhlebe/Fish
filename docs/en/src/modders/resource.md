# Resources

`fish_api::resource` is Fish's resource loader (Fabric's resource loader API): a mod's own `assets/` and `data/` reach
the game, a mod reads them, and it reads the game's active resources.

## A mod's files in the game

Every file under `assets/` and `data/` in a mod's archive reaches the game, whether or not the mod registers blocks or
items: the loader puts them into the mod's content folder (`<game dir>/.fish/content/<mod_id>/`), which Horizon reads
like a content mod's. Textures, models, blockstates, sounds, translations, loot tables, recipes and tags all work this
way:

```text
assets/content_demo/lang/en_us.json
assets/content_demo/sounds.json
data/content_demo/loot_table/blocks/ruby_block.json     the ruby block drops a ruby
data/minecraft/tags/block/mineable/pickaxe.json         additions to vanilla and c: tags
```

Files of other namespaces are skipped with a warning (see [blocks](block.md#files)).

## Reading files

```rust
use fish_api::event::lifecycle;
use fish_api::resource::Resources;

fn init() -> Result<(), fish_api::Error> {
    let sounds = Resources::own_text("assets/content_demo/sounds.json")?;
    let files = Resources::list_own("assets/content_demo/")?;
    log::info!("{} asset files, ruby_chime: {}", files.len(), sounds.contains("ruby_chime"));
    lifecycle::resources::RELOADED.register(|c| {
        if let Ok(text) = Resources::game_text(&c.client(), "assets/minecraft/lang/en_us.json") {
            log::info!("the game's en_us.json is {} bytes now", text.len());
        }
    })?;
    Ok(())
}
```

- **`Resources::own(path)`** / **`own_text`** read a file of the mod's own archive, from `pre_launch` on.
  **`list_own(prefix)`** names the files under a prefix, in archive order.
- **`Resources::game(client, path)`** / **`game_text`** read a file of the game's active resource root: the vanilla
  resources with the selected resource packs and the content of every mod over them, as the game uses them. It needs the
  game of an event (`c.client()`); a dedicated server has no resource root (`UNSUPPORTED`).
- Paths are `assets/<namespace>/...` or `data/<namespace>/...` without `.` or `..` components; files are at most 64 MiB.
  A missing file is `NOT_FOUND`.

## Reloads

`lifecycle::resources::RELOADED` fires in the window after it reloaded its resources (the resource packs changed in the
options, F3+T): the game's files may differ now. Data reloads of the server (`/reload`) are
`lifecycle::reload::START` and `END` ([lifecycle events](lifecycle.md)). The event comes from a small Horizon patch
(`0011-resources-reloaded.patch`) after the reload swapped the resources in.

## Limits

- A mod's files are fixed for the run: they are copied when the game starts, and a resource reload re-reads the same
  copies. Built-in optional resource packs (Fabric's `registerBuiltinResourcePack`) do not exist yet.
- There is no reload listener with stages: mods get one event after the reload, not a part in it.
- Testing: the step `resource_reload` of [Testing mods](testing.md); the simulated resource root holds a few vanilla
  stand-ins with the content folders over them.
