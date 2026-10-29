# Roadmap

Executed as a checklist, one commit per item (`P<n>.<m>: <summary>`). Items are ticked in this
file and in the Russian copy at the same time.

## P0 — Foundation

- [x] **P0.1** Repository, license, readmes, contributing guide
- [x] **P0.2** Workspace, toolchain pin, `fish.toml`, lints, deny config
- [x] **P0.3** `xtask`: ci, docs-check, coverage, check-config
- [x] **P0.4** mdBook skeletons (en/ru) and this roadmap
- [x] **P0.5** GitHub Actions CI

## P1 — Metadata and dependency resolution

- [ ] **P1.1** Manifest model and parser (`fish.mod.toml`)
- [ ] **P1.2** Version requirement sets and builtin candidates
- [ ] **P1.3** Dependency resolver
- [ ] **P1.4** Tests (unit + property) with at least 95% coverage
- [ ] **P1.5** Manifest reference docs

## P2 — ABI, API core, loader runtime, test host

- [ ] **P2.1** `fish-abi`: stable C ABI types
- [ ] **P2.2** `fish-pack`: `.fish` archives
- [ ] **P2.3** `fish-api` core: identifiers, events, loader API, `export_mod!`
- [ ] **P2.4** `fish-loader` core: discovery, loading, event bus, services
- [ ] **P2.5** `fish-test-host`
- [ ] **P2.6** Example mods
- [ ] **P2.7** Loader integration tests
- [ ] **P2.8** ABI, getting-started and events docs

## P3 — Dev tooling (`cargo-fish`)

- [ ] **P3.1** `cargo fish new`
- [ ] **P3.2** `cargo fish build`
- [ ] **P3.3** `cargo fish check`
- [ ] **P3.4** `cargo fish test`
- [ ] **P3.5** Tooling and testing docs

## P4 — Fish Edition pipeline and first hooks

- [ ] **P4.1** `fish-hooks`: dispatch and view traits
- [ ] **P4.2** `xtask horizon fetch` and `xtask mixins install`
- [ ] **P4.3** `xtask edition stage`
- [ ] **P4.4** horizon-mixins bundle `fish`
- [ ] **P4.5** Fish-side implementations of the first hooks
- [ ] **P4.6** Build, package and selftest
- [ ] **P4.7** Edition workflow
- [ ] **P4.8** Local in-game e2e (`xtask e2e`)
- [ ] **P4.9** Edition docs

## P5 — Installer and launcher integration

- [ ] **P5.1** `fish-launch` wrapper
- [ ] **P5.2** `fish-installer` core
- [ ] **P5.3** Installer CLI
- [ ] **P5.4** Installer GUI
- [ ] **P5.5** `cargo fish run`
- [ ] **P5.6** `xtask horizon pin`
- [ ] **P5.7** Installer and wrapper tests
- [ ] **P5.8** Player docs

## P6 — Events I: lifecycle, interaction, entity, message, command, crash, debug

- [ ] **P6.1** `lifecycle`
- [ ] **P6.2** `interaction`
- [ ] **P6.3** `entity`
- [ ] **P6.4** `message`
- [ ] **P6.5** `command`
- [ ] **P6.6** Crash report section and mod failure notices
- [ ] **P6.7** `debug::F3_LINES`
- [ ] **P6.8** Test-host simulation, `events-demo`, e2e scenarios
- [ ] **P6.9** Docs per module

## P7 — Networking and registry sync

- [ ] **P7.1** `networking`
- [ ] **P7.2** Connection events
- [ ] **P7.3** `fish:handshake`
- [ ] **P7.4** `registry_sync`
- [ ] **P7.5** Docs

## P8 — Content I: registries, blocks, items, tags, sounds, game rules

- [ ] **P8.1** `registry`
- [ ] **P8.2** Blocks
- [ ] **P8.3** Custom state properties
- [ ] **P8.4** Items
- [ ] **P8.5** `item_group`
- [ ] **P8.6** `content` registries
- [ ] **P8.7** `tag` and convention tags
- [ ] **P8.8** Sound events
- [ ] **P8.9** Particle types
- [ ] **P8.10** `game_rule`
- [ ] **P8.11** Tests, `content-demo`, e2e
- [ ] **P8.12** Docs

## P9 — Resources and data

- [ ] **P9.1** `resource`
- [ ] **P9.2** `resource::conditions`
- [ ] **P9.3** `loot`
- [ ] **P9.4** `recipe`
- [ ] **P9.5** `biome`
- [ ] **P9.6** `dimension`
- [ ] **P9.7** `attachment`
- [ ] **P9.8** `serialization`
- [ ] **P9.9** `datagen` and `cargo fish datagen`
- [ ] **P9.10** Tests and docs

## P10 — Client rendering

- [ ] **P10.1** `fish-render` and render hooks
- [ ] **P10.2** World render events and buffers
- [ ] **P10.3** HUD layers
- [ ] **P10.4** Custom WGSL shaders
- [ ] **P10.5** Color providers and render layer map
- [ ] **P10.6** Model API
- [ ] **P10.7** `model_loading`
- [ ] **P10.8** `renderer` (FRAPI analogue)
- [ ] **P10.9** Particle factories
- [ ] **P10.10** `client::sound`
- [ ] **P10.11** `block_view` render data
- [ ] **P10.12** Tests, `render-demo`, docs

## P11 — Client UI and input

- [ ] **P11.1** `client::screen`
- [ ] **P11.2** Mod-owned screens and widget toolkit
- [ ] **P11.3** `config`
- [ ] **P11.4** `client::key_mapping`
- [ ] **P11.5** `menu`
- [ ] **P11.6** `ItemTooltipCallback`
- [ ] **P11.7** Tests, `ui-demo`, docs

## P12 — Gameplay systems

- [ ] **P12.1** Block entities
- [ ] **P12.2** Entities
- [ ] **P12.3** `transfer` and `lookup`
- [ ] **P12.4** Fluids
- [ ] **P12.5** Mob effects, potions, brewing
- [ ] **P12.6** Villager professions, trades, POI types
- [ ] **P12.7** Enchantment events
- [ ] **P12.8** Tests, `gameplay-demo`, docs

## P13 — Game tests

- [ ] **P13.1** `gametest`
- [ ] **P13.2** `client::gametest`
- [ ] **P13.3** Docs

## P14 — Deep mods (the Mixin path)

- [ ] **P14.1** Manifest `[mixins]` and `.fishmixin`
- [ ] **P14.2** `cargo fish edition --mixins`
- [ ] **P14.3** Player-side builder in the installer
- [ ] **P14.4** Docs

## P15 — Dedicated server

- [ ] **P15.1** Server editions
- [ ] **P15.2** `fish-installer server`
- [ ] **P15.3** Server selftest and docs

## P16 — Distribution and polish

- [ ] **P16.1** Release workflow
- [ ] **P16.2** Horizon tracking workflow
- [ ] **P16.3** Freeze ABI 1.0 and publish crates
- [ ] **P16.4** `fish.h` and C example mod
- [ ] **P16.5** macOS aarch64 editions
- [ ] **P16.6** Deploy docs sites, tutorials, Fabric to Fish migration table
