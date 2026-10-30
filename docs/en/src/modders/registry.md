# Registries

`fish_api::registry` keeps Fish registries: identifiers (`namespace:path`) mapped to raw ids.

```rust
let raw = fish_api::registry::register("fish:blocks", "mymod:ruby_block")?;
```

- Register while `pre_launch` or `main` runs. After every mod's entrypoints ran the registries are **frozen**: a late
  call fails with `registries are frozen` and the loader logs a warning.
- Registering the same id twice returns the same raw id.
- Raw ids follow registration order (mod load order, then call order). On joining a server they are renumbered to the
  server's, so never store a raw id; look it up with `raw_id` when needed.
- Entries of mods with `environment = "*"` are synchronised with the server (see [Networking](networking.md)); entries of
  client-only or server-only mods stay local.

## Status

This is the registry core. Items, item groups, sounds and particles (the rest of P8; game rules and blocks have their own pages, [game rules](game_rule.md) and [blocks](block.md)) are
not implemented: they need content overlay hooks in Horizon.
