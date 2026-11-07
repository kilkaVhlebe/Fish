# Building and debugging editions

```text
cargo xtask horizon fetch            # .horizon/source/<commit>/ (idempotent)
cargo xtask mixins install           # horizon-mixins at fish.toml [mixins].rev
cargo xtask edition stage            # copy source + fish crates, edit Cargo.toml, update the lock
cargo xtask edition mixins           # assemble the bundle, horizon-mixins apply
cargo xtask edition build --profile fast
cargo xtask edition package --profile fast     # dist/fish-edition-….zip
cargo xtask edition selftest --profile fast    # runs the binary with hello.fish and content_demo.fish
cargo xtask edition all --profile fast         # all of the above
cargo xtask edition regressions                # Horizon's own tests: pristine vs staged
```

Everything lives in `.horizon/` (git-ignored): `source/<commit>`, `work/<commit>/{src,bundle,staged}`, `tools/<rev>`,
`target`. A first build compiles the whole game (several minutes); `fast` links quickly, `release` is thin-LTO.
On Windows the build uses `RUSTFLAGS=-C target-feature=+crt-static` like Horizon's release workflow.

## Reading horizon-mixins diagnostics

`apply` prints one JSON object on failure. `code` says what: `E_UNSUPPORTED_MACRO` (the target contains a macro: hook a
macro-free callee instead), `E_MODULE` (the support module payload does not parse, or has file attributes such as `//!`:
use plain `//` comments), `E_MODULE_CONFLICT`, a missing target. `injection_id` and `target` name the offending hook.
Hook targets are Cargo-target-qualified: `horizon_client::game::native_hooks::Game<'_>::native_tick`.
`horizon-mixins plan --workspace .horizon/work/<commit>/src --mods .horizon/work/<commit>/bundle --out plan.json`
shows the full plan without writing the stage.

## Editing the bridge

`horizon/mixins/fish/`: `horizon-mixin.toml` (placeholders `@FISH_VERSION@`, `@HORIZON_COMMIT@`), `helpers/*.rs` (one
function each, compiled as a local function inside the hooked target), and `modules/bridge/*.rs` (files are wrapped
into inline `mod`s and concatenated: horizon-mixins refuses external `mod` and `include!`).

## Re-pinning Horizon

Change `[horizon]` in `fish.toml` (or run `cargo xtask horizon pin --latest-dated`), rebuild with `edition all`. If
horizon-mixins reports a vanished target, fix the bundle: `plan` names it. Hook points of the `native_*` family are
only ever added by Horizon, so a newer commit normally just works.

## Pinning and the installer

`cargo xtask horizon pin --latest-dated [--check] [--no-build]` resolves the newest `nightly-YYYY-MM-DD` release, refuses
a commit that is older than the pin or lacks `native_mods` hook points, rewrites `[horizon]` in `fish.toml` and rebuilds.
`--check` only reports. `cargo xtask dist installer` builds `fish-launch`, then a single-file `fish-installer` that
carries it (`--features embed-launch`, `FISH_LAUNCH_BIN`) into `dist/`.

## Note on the horizon-mixins revision

The launcher at the pinned Horizon commit builds native mods with horizon-mixins `0500ff2`. Fish pins `0d6b103`, the next
commit, which adds optional public visibility for support modules (needed once hooks live in other crates).

## Patches

Four hook sites for networking sit in functions that horizon-mixins rejects (opaque macros, `?` plus closures). They are
unified diffs in `horizon/patches/0001..0004`, applied by `edition stage` (after a `git apply --check`). When Horizon
moves, fix a failing patch by hand; each adds a call into `fish_bridge::net`.

`0005-gamerule-screen.patch` is not a hook site: the Game Rules screen keeps its rows and edit state in private fields
of one struct, so showing the rules of mods needs new fields (`RulesScreen::with_fish`). It calls
`fish_bridge::rules::world_of` from `game/lan_screen.rs`.

`0006-content-mods.patch` is not a hook site either: Horizon builds its content overlay from the mods in its mods folder,
so `game/mods.rs::content_root` also takes the folders the loader wrote under `<game dir>/.fish/content/` and treats
them as always-on content mods. `0007-block-ticks.patch` makes the block simulation of horizon-world (`Sim`) report
neighbour updates, random ticks and scheduled ticks of blocks of Fish to `Dispatch::block_event`; the simulation keeps
that state privately, so no hook can reach it.

`0008-creative-inventory.patch` makes `Creative::load` pass the text of `creative-inventory.json` through
`fish_lib::creative_patch` (`Dispatch::creative_inventory`) before it becomes tabs. A hook cannot do it:
`Creative::from_json` returns `Result<Self>`, which horizon-mixins cannot cancel, and takes a reference that cannot be
replaced.

`0009-content-tables.patch` applies the content tables of mods (`fish:content`) where Horizon builds its own tables:
`load_world_rest` (the item table and the mob data, through `fish_lib::content_items` and `fish_lib::content_mobs`),
`Transformers::load` (each `block_transformer/<tool>.json` through `fish_lib::transformer_list`), `sculk::frequency`
(`fish_lib::sculk_frequency`) and horizon-world's `level/data.rs` `cached` (the fire odds on the loaded `Behaviours`,
after the snapshot, so the snapshot keeps vanilla's). Hooks cannot: `load_world_rest` and `Transformers::load` have
macros and `?`, `sculk::frequency` has closures (`E_UNSUPPORTED_CONTROL_FLOW`), and the behaviours load in generic code.

`0010-block-state-properties.patch` lets a block declaration of a content mod carry `state_properties` (a name, the
values, the default) and makes `mod_content.rs` emit every template state once per combination of their values: the
`facts.json` rows (state and description) and the NBT tables' `states` get the added properties, `cached_shapes`
repeats, `model-states.json` gets the schema and possible states, `render-definitions.json` the ids and the default.
Without added properties it changes nothing. It is not a hook site: the overlay builder is one private plan.

`0011-resources-reloaded.patch` adds one call at the end of `Game::apply_staged_pack` (the swap after a resource reload,
F3+T or a pack change): `fish_bridge::events::resources_reloaded` raises `fish:lifecycle/resources_reloaded`. The
function has closures and `?`, so horizon-mixins cannot hook it.

## The self test and the in-game test

`edition selftest` starts the built binary with `FISH_LOADER_ADD_MODS` (`hello` and `content_demo`) and
`FISH_LOADER_SELFTEST`: the loader boots inside the edition, runs 20 synthetic ticks, a frame, a HUD pass and a command,
and writes a report before the game opens a window. Besides the hello checks it has `content_blocks` (the example's
blocks and the lamp's state properties), `content_folder` (the content folder for Horizon with declarations, sounds,
particles and blockstates) and `content_tables`. `cargo xtask e2e --resources <26.3 folder> --horizon-ctl <path>` drives
a real game window through horizon-ctl and checks the events, game rules, blocks, items, content tables, sounds and state
properties of the example mods in `latest.log` and in the world, then reopens the saved world.
