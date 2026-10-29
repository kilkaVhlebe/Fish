# Architecture overview

## Decisions

- **Injection model: Fish Edition + runtime mods.** Fish CI builds the Horizon client from source at
  a pinned commit and injects hooks at build time with *horizon-mixins*, Horizon's compile-time
  source-mixin tool. Mods are native dynamic libraries loaded at game start through a stable C ABI.
  Players compile nothing. Runtime binary patching of the official executable is rejected
  (thin-LTO, single codegen unit, private symbols, frequent nightlies).
- **Launcher integration** only through the public `horizon-launcher` CLI. The installer creates an
  instance and sets its *wrapper* to `fish-launch`. Fish never writes launcher-owned files.
- **Platforms:** Windows first; Linux in CI from P0; macOS in P16.
- **Docs:** rustdoc/API reference in English; guides and roadmap in English and Russian.
- **Toolchain:** Rust 1.99.1, edition 2024. License MIT.
- **ABI versioning:** major 0 until the first public release; then host minor >= mod minor.
- **Naming:** data dir `fish-loader`, env vars `FISH_LOADER_*` (no clash with the fish shell).

## Core contracts

- `fish.toml` is the single source of truth for versions, pins, coverage gates, release slug.
- Mods are `.fish` zip archives (or directories) with a `fish.mod.toml` manifest and
  `bin/<platform>/` libraries.
- The ABI is `#[repr(C)]` tables, versioned by `size` and appended functions only; panics never
  cross the boundary; memory is freed by its allocator.
- Events are keyed by `namespace:path` identifiers with phases and combine modes (Fabric `Event<T>` parity).

Details land in the pages added by later phases.
