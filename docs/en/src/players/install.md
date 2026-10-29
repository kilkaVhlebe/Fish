# Installing Fish

Fish plugs into the official Horizon launcher. You need the launcher with a Horizon Nightly installed.

## With the window

1. Run `fish-installer` (no arguments opens the window).
2. Check the launcher path (it is found automatically when the launcher is installed or running; use **Browse…** otherwise).
3. Leave **Select Fish in the launcher** ticked so Play starts the Fish instance, then press **Install**.
4. When it says *Done*, press **Open Horizon launcher**.

The window follows your system language (English or Russian).

## From the command line

```text
fish-installer install [--launcher PATH] [--instance NAME] [--edition ZIP] [--select] [--force] [--allow-unsigned] [--yes]
fish-installer repair | update | uninstall | status
```

- `install` fetches the newest edition for your platform from the release index, verifies its signature and
  SHA-256, unpacks it into Fish's home folder, installs Horizon in the launcher if the edition needs a version you
  do not have, creates the instance (default name **Fish**), pins it to that Horizon version and sets its *launch
  wrapper* to `fish-launch`.
- `--edition ZIP` installs a local edition archive instead (builds from source: see *Building editions*).
- `repair` reinstalls the recorded edition and fixes the instance; `update` moves to the newest edition.
- `uninstall` clears the wrapper and removes Fish's editions and binaries. **The instance, its worlds and its
  `fish-mods` folder are kept.**
- `--force` replaces a wrapper you set yourself on that instance; without it the installer refuses.
- `--allow-unsigned` (or `FISH_LOADER_ALLOW_UNSIGNED=1`) accepts an unsigned index. Development only.

## Where things go

| What | Where |
| --- | --- |
| Fish home | `%APPDATA%\fish-loader` (Windows), `~/.local/share/fish-loader` (Linux), `~/Library/Application Support/fish-loader` (macOS); override with `FISH_LOADER_HOME` |
| Editions, wrapper, installer | `<home>/editions`, `<home>/bin`, `<home>/installer.json` |
| Your mods | the instance's game folder, `fish-mods/` |

Fish never writes the launcher's own files. Everything goes through `horizon-launcher` commands.

## Checking by hand

1. Run the installer. The launcher now lists an instance called **Fish**; its *Launch…* panel says it runs through
   `…fish-launch.exe`.
2. Press **Play** on it. In game, **Options > Mods** lists mods as `Name 1.0 [Fish]`.
3. Drop `hello.fish` into `fish-mods/`: the text *Hello from Fish* appears at the top left of the HUD.
4. `fish-installer uninstall`, then Play: plain Horizon starts.
