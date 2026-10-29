# Launcher integration

Fish touches the Horizon launcher in exactly one way: the public `horizon-launcher` command line.

| Purpose | Command |
| --- | --- |
| Installed versions | `horizon-launcher versions --installed --json` |
| Install a Horizon build | `horizon-launcher install <release tag>` |
| List instances | `horizon-launcher instance list --json` |
| Create the instance | `horizon-launcher instance create <name>` |
| Pin its version | `horizon-launcher instance version <name> <installed tag>` |
| Read / set the wrapper | `horizon-launcher instance launch <name> --json` / `… --wrapper "\"<path>\""` |
| Select it | `horizon-launcher instance select <name>` |

Fish never writes `instances.json`, `settings.json`, `versions/`, `accounts.json` or `native/`.

## The wrapper

An instance can run its game *through* a program. With the wrapper set to `fish-launch`, the launcher runs
`fish-launch <…/versions/<tag>/horizon-client> <args…>` in the instance folder with the instance's environment and
session arguments. `fish-launch`:

1. derives the version tag from the program path (the folder below `versions/`; the macOS bundle layout is handled),
2. picks the installed edition for that Horizon version (commit prefix for `nightly-*` and `native-*` tags,
   `release_tag` for `v*`; the newest Fish wins),
3. verifies the edition binary's SHA-256,
4. starts it with identical arguments and `FISH_LOADER_HOME` set. On Windows the game is tied to a Job Object
   (`KILL_ON_JOB_CLOSE`) so Stop in the launcher reaches it; on Unix the wrapper `exec`s into it.

Failures print to the launcher console and `<Fish home>/logs/fish-launch.log` and exit with code 2.

## Instance rules

- absent: created;
- present with a `fish-launch` wrapper, or none: reused;
- present with another wrapper: refused unless `--force`.

`uninstall` clears the wrapper (`--wrapper ""`); if a launcher version rejects that, the installer says so and asks you to
clear it in the launcher's *Launch…* panel, and still removes its own files.

## Versions

Fish editions are built for one Horizon commit. When the launcher updates an instance to a newer Nightly, `fish-launch`
refuses with an actionable message until `fish-installer update`/`repair` brings an edition for it.
