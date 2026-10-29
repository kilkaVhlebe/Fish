# Troubleshooting

**"Fish has no build for Horizon <tag>."** The instance plays a Horizon version Fish has no edition for (for example
after the launcher updated it). Run `fish-installer repair` (it re-pins the instance) or set the instance's version back
to the one Fish was installed for.

**"Fish edition … is damaged (checksum mismatch)."** The edition's files were changed. Run `fish-installer repair`.

**The installer refuses: "already runs through another wrapper".** The instance you named has a wrapper of its own
(for example `nice -n 5`). Use another `--instance`, or `--force` to replace it.

**The launcher is not found.** Pass `--launcher <path to horizon-launcher>` or set `FISH_LOADER_HORIZON_LAUNCHER`.

**"No release signing key" / signature errors.** Only signed editions are installed. For a locally built edition use
`--edition <zip> --allow-unsigned`.

**A mod does not appear.** Read `logs/latest.log` for `[fish]` lines: a missing dependency, a mod built for another Fish
ABI (`was built for Fish ABI X.Y; this Fish serves A.B`) or for another platform (`has no native library for …`) is
reported by name. `cargo fish check <file>` shows the same without starting the game.

**The wrapper does not start the game.** Look at `<Fish home>/logs/fish-launch.log` and at the launcher's console.

**White window for a long time at start.** Horizon prepares its resources before the first frame (tens of seconds on a
cold start). Fish adds almost nothing to that; compare with the log's `resources loaded in …` line.
