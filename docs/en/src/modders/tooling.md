# Tooling: `cargo fish`

`cargo-fish` is the Fish counterpart of Fabric Loom. Install it with `cargo install --path crates/cargo-fish`
(from a Fish checkout) and run it as `cargo fish <command>`. No remapping is needed: the API is stable.

## `cargo fish new <name>`

```text
cargo fish new my-mod [--template basic|client|content] [--fish-path <Fish checkout>]
```

Creates `Cargo.toml` (`crate-type = ["cdylib"]`, `[profile.release] panic = "unwind"`), `fish.mod.toml`,
`src/lib.rs` with `export_mod!`, `tests/hello.toml` and `assets/<id>/lang/en_us.json`. The crate name must be
lowercase; the mod id is the name with `-` replaced by `_`, and it must equal the library name.

- `basic`: a `main` entrypoint. `client`: `main` plus `client`.
- `content`: assets and data only, **no Cargo project and no library**.
- With `--fish-path` the `fish-api` dependency is a path dependency; without it, the crates.io version
  matching your `cargo-fish`.

## `cargo fish build [--release] [--target-platform <key>]`

1. `cargo build` (JSON messages) for projects with a `[library]`.
2. Refuses `panic = "abort"` (`[profile.*]` of the workspace root or `CARGO_PROFILE_*_PANIC`).
3. Validates `fish.mod.toml` and that the built library file matches `[library] name`.
4. Writes `target/fish/<id>-<version>.fish` with `bin/<platform>/`, `assets/`, `data/`, the icon and nested archives.

`--target-platform` takes a platform key (`windows-x86_64`, `linux-x86_64`, `macos-aarch64`, …) and passes the
matching `--target` triple to cargo.

## `cargo fish check <file.fish|dir>`

Boots a throwaway host with only that mod, **in a child process**, so a library that crashes cannot take the
tool down. Prints the loaded mods, warnings and errors; exits non-zero if anything failed.

## `cargo fish test [filter]`

Builds, then runs `tests/*.toml` scripts; see [Testing](testing.md).

## `cargo fish run`

Builds the mod, then starts the Fish Edition with it in development mode (`FISH_LOADER_ADD_MODS`, `FISH_LOADER_DEV=1`),
offline, in a scratch game folder next to the build output (`dev-home`). Options: `--resources <dir>` (prepared game
resources; default: the launcher's pinned Horizon version), `--edition <folder|binary>` (default: the newest installed
edition), `--name <player>` (default `Dev`), `--target-dir`. It needs an installed edition (`fish-installer`) and
resources prepared by a first Horizon launch.

## Running in a game

Put the `.fish` into `<game dir>/fish-mods/`, or point `FISH_LOADER_ADD_MODS` at it. Environment variables:

| Variable | Meaning |
| --- | --- |
| `FISH_LOADER_ADD_MODS` | extra mod paths (like `PATH`), loaded as development mods |
| `FISH_LOADER_DEV` | `1`: development mode |
| `FISH_LOADER_STRICT` | `1`: a mod panic aborts the process instead of disabling the mod |
