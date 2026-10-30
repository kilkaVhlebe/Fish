# Fish roadmap

Fish is a Fabric-style mod loader for [Horizon](https://github.com/horizon-rust/horizon). This page is the short version;
the detailed per-item checklist is in [docs/en/src/roadmap.md](docs/en/src/roadmap.md).

| Phase | What | Status |
| --- | --- | --- |
| P0 | **Foundation**: Repository, CI, docs skeleton, tooling | ✅ done |
| P1 | **Mod manifest and dependencies**: `fish.mod.toml`, versions, dependency resolver | ✅ done |
| P2 | **Loader core**: Stable C ABI, `.fish` archives, mod API, event bus, test host | ✅ done |
| P3 | **Developer tool**: `cargo fish new / build / check / test` | ✅ done |
| P4 | **Fish Edition**: Horizon built with Fish hooks, first events (ticks, HUD, commands, keys) | ✅ done |
| P5 | **Installer**: One-click installer (window + CLI), launcher integration, `cargo fish run` | ✅ done |
| P6 | **Events I**: Lifecycle, interaction, entities, chat, commands, F3, crash info | ✅ done |
| P7 | **Networking**: Custom payloads, handshake, registry sync | ✅ done |
| P8 | **Content**: Registries, blocks, tags and game rules done; items, sounds, particles not yet | 🚧 in progress |
| P9 | **Resources and data**: Resource packs, loot, recipes, biomes, dimensions, data generation | ⬜ planned |
| P10 | **Rendering**: World/HUD rendering, shaders, models, particles, sound | ⬜ planned |
| P11 | **UI and input**: Screens, config screens, key bindings, menus | ⬜ planned |
| P12 | **Gameplay**: Block entities, entities, storage API, fluids, effects, villagers | ⬜ planned |
| P13 | **Game tests**: In-game test framework | ⬜ planned |
| P14 | **Deep mods**: Source-level patching (the Mixin analogue) | ⬜ planned |
| P15 | **Dedicated server**: Server editions and launcher scripts | ⬜ planned |
| P16 | **Release**: Signed releases, ABI 1.0, crates.io, macOS, public docs | ⬜ planned |

## Right now

- Players can install Fish, press Play on the "Fish" instance and put `.fish` mods into `fish-mods/`.
- Mods can react to many game events, add commands, send network payloads and register registry entries.
- Next: P8 blocks and items (needs content hooks inside Horizon).

## Known gaps

- Networking is wired into Horizon but has not been tried between two real game instances yet.
- The installer has only been tested against a fake launcher.
- Some P6 events are approximations (see [Hook map](docs/en/src/architecture/edition.md)).
