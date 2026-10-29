# The stable ABI

Mods and the loader talk through `fish-abi`: `#![no_std]`, zero dependencies, all `#[repr(C)]`.
Layout changes fail to compile (`size_of`/`offset_of` asserts for x86_64 and aarch64).

## Versioning

`FISH_ABI_MAJOR`/`FISH_ABI_MINOR` (currently 0.1). While the major is 0, host and mod minors must
be **equal**. From 1.0 the host minor must be at least the mod minor and majors must match.
A refused mod reports: `<id> was built for Fish ABI X.Y; this Fish serves A.B`.

## The mod export

Every mod library exports `extern "C" fn fish_mod_v1() -> *const FishModV1`, a pointer to static data:

```text
FishModV1 { size, abi_major, abi_minor, entrypoints, entrypoint_count }
FishEntrypointV1 { key, name, kind (0 init | 1 object), init, object }
```

The loader checks that every `[entrypoints]` entry of the manifest exists with the right kind:
`pre_launch`, `main`, `client` and `server` are `init` entrypoints, every other key is an
`object` entrypoint. `fish_api::export_mod!` generates all of this.

## The host table

`FishHostV1` is passed to every `init` entrypoint. It carries `host` (opaque, passed back to every
call), `mod_token` (identifies the calling mod) and function pointers: `log`, `get_service`,
`event_define`, `event_lookup`, `event_register`, `event_phase_order`, `event_invoke`,
`event_invoke_index`.

## Services

Each Fish API module is a service: id `fish:<module>`, a `#[repr(C)]` table starting with
`{ size, version }`. Functions are only appended; check `size` before calling a function added
after version 1. The first service is `fish:loader` (mods, directories, object share, object entrypoints).

## Rules

- **Status codes**: `FishStatus` (`OK=0`, `INVALID_ARGUMENT=-1`, `NOT_FOUND=-2`, `UNSUPPORTED=-3`,
  `PANICKED=-4`, `STALE_HANDLE=-5`, `WRONG_THREAD=-6`, `ALREADY_EXISTS=-7`, `TYPE_MISMATCH=-8`,
  `VERSION_MISMATCH=-9`, `INTERNAL=-100`). Action results travel as `u8` and are validated, never transmuted.
- **Strings** are `FishStr { ptr, len }` (UTF-8, borrowed for the call). Host strings (mod info,
  directories) live as long as the loader.
- **Memory** is freed only by the module that allocated it.
- **Panics** never cross the boundary. Every `extern "C"` function on both sides catches them; a mod-side
  panic returns `PANICKED`, the host marks the mod *failed* and skips its listeners from then on
  (or aborts with `FISH_LOADER_STRICT=1`). Build mods with `panic = "unwind"`.
- **Handles** (`FishHandle { index, generation }`) are valid only during the callback that received them.
- **Event payloads** are `#[repr(C)]` structs starting with `size: u32`; fields are only appended.
- **Threads**: listeners may run on the render thread and the server thread, so listeners are
  `Fn + Send + Sync + 'static`.
- **Isolation**: the loader extracts each library into `<game dir>/.fish/cache/<id>-<version>-<hash>/`,
  so every loader instance gets its own module instance (and its own mod statics).
