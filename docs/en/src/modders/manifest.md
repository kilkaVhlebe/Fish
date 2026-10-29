# Mod manifest (`fish.mod.toml`)

Every mod has a `fish.mod.toml` at the root of its `.fish` archive (or directory). Schema 1.
Unknown keys are errors. A manifest with a higher `schema` is refused with
`this mod was made for a newer Fish (schema N); update Fish`.

```toml
schema = 1                               # required
id = "example_mod"                       # required; ^[a-z][a-z0-9_]{1,63}$
version = "1.0.0"                        # required; SemVer 2.0
name = "Example Mod"                     # default: the id
description = ""
authors = ["…"]
contributors = []
license = "MIT"                          # SPDX string, not validated
icon = "icon.png"                        # archive path
environment = "*"                        # "*" | "client" | "server"
provides = ["alias_id"]                  # alias ids; same rules as `id`
nested = ["nested/lib.fish"]             # archives inside this one

[contact]                                # free string values
homepage = "https://example.com"

[library]                                # needed when entrypoints are declared
name = "example_mod"                     # file = DLL_PREFIX + name + DLL_SUFFIX

[entrypoints]                            # key → names exported by export_mod!
pre_launch = []
main = ["init"]
client = ["init_client"]
server = []
"fish:modmenu" = ["menu_api"]            # any other key is a custom entrypoint

[depends]                                # id = "req" or ["req", "req"] (array = OR)
fish = ">=0.1.0"
[recommends]                             # missing or mismatching → warning
[suggests]                               # informational
[conflicts]                              # present and matching → warning
[breaks]                                 # present and matching → hard error

[custom]                                 # arbitrary table, read-only for mods
```

## Ids

`^[a-z][a-z0-9_]{1,63}$`. The ids `fish`, `fish_api`, `horizon` and `minecraft` are reserved:
mods cannot use them as `id` or in `provides`, but may name them in dependency tables.

## Version requirements

Requirements use the Rust `semver` syntax (`>=1.2`, `^1`, `<2, >=1.5`, `*`). An array means *any of*.
Pre-release versions follow semver rules, except for the loader-provided builtins, where a
pre-release counts as its release (`0.2.0-nightly.1` satisfies `>=0.2.0`).

## Builtins

| id | version |
| --- | --- |
| `fish`, `fish_api` | the Fish version |
| `minecraft` | `26.3.0` |
| `horizon` | `<release line>.0-nightly.<YYYYMMDD>+<short commit>` |

## Resolution

- Mods whose `environment` excludes the running side are ignored.
- With duplicate ids the highest version wins (a development mod wins ties); both paths are named
  in a warning.
- Every `depends` must be satisfied, no `breaks` may match, and an id or alias belongs to one mod.
- Nested archives only load when they can be satisfied; the highest compatible version is chosen.
- Load order: dependencies first, then by id. A dependency cycle is ordered by id with a warning.
- The search is bounded (100,000 steps).

Errors name the mod, the requirement and what is installed, for example:
`mod "a_mod" 1.0.0 needs "b_mod" >=2, but only 1.4.0 is installed (fish-mods/b_mod.fish)`.
