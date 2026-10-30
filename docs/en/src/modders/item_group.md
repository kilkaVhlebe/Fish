# Item groups

`fish_api::item_group` adds creative tabs and puts items into tabs. Content items are **not** in any tab by themselves:
only a tab's `entries`, a `modify_entries` call and the search tab list them.

```rust
use fish_api::item_group::{ItemGroup, ItemGroups};

fn init() -> Result<(), fish_api::Error> {
    ItemGroup::builder("content_demo:ruby_tab")
        .title("Ruby")
        .icon("content_demo:ruby")
        .entry("content_demo:ruby")
        .entries(["content_demo:ruby_snack", "content_demo:ruby_block", "content_demo:raw_ruby"])
        .register()?;
    ItemGroups::modify_entries("minecraft:ingredients", ["content_demo:ruby"])?;
    Ok(())
}
```

- **`ItemGroup::builder(id)`** starts a tab: `.title` (a literal string, shown as is), `.icon` (an item id), `.entry`
  and `.entries` (item ids, in order). `.register()` adds it.
- **`ItemGroups::modify_entries(group_id, entries)`** appends items to an existing group: a vanilla one such as
  `minecraft:ingredients`, or a tab of a mod.

## Rules

- **Ids** follow the same rules as [items](item.md); the namespace is the registering mod's id.
- **Registration** is allowed while `pre_launch` or `main` runs, the same freeze as [registries](registry.md). A tab
  registered twice fails with `ALREADY_EXISTS`.
- **Order**: tabs are sorted by id, and entries keep the order they were given in; `modify_entries` calls apply in
  mod load order.

## When it is applied

Once, while the game loads its resources: the loader rewrites `creative-inventory.json` in memory (the file on disk is
untouched), adds the Fish tabs and appends the entries. A `modify_entries` call for an unknown group is logged and
skipped. When something was patched the loader logs

```text
Fish creative inventory: 1 tab(s), 1 addition(s)
```

(`tab(s)` are the Fish tabs, `addition(s)` the `modify_entries` calls that applied).

## Limits

- Horizon's creative screen has exactly **two** free tab slots, top column 5 and bottom column 5 (vanilla's Saved
  Hotbars and Operator tabs are not implemented). The first two Fish tabs by id get them; further tabs are logged and
  their entries stay reachable through the search tab only.
- Tab titles are literal strings: Horizon does not translate the titles of non-vanilla tabs, so there is no translation
  key.
- A tab only lists items; it does not change them.
- Testing: the expectations `creative_tabs`, `creative_entries` and `no_creative_entries` of [Testing mods](testing.md).
