# Building and debugging editions

```text
cargo xtask horizon fetch            # .horizon/source/<commit>/ (idempotent)
cargo xtask mixins install           # horizon-mixins at fish.toml [mixins].rev
cargo xtask edition stage            # copy source + fish crates, edit Cargo.toml, update the lock
cargo xtask edition mixins           # assemble the bundle, horizon-mixins apply
cargo xtask edition build --profile fast
cargo xtask edition package --profile fast     # dist/fish-edition-….zip
cargo xtask edition selftest --profile fast    # runs the binary with hello.fish
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
