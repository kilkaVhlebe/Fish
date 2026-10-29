# Using mods

Put `.fish` files into the `fish-mods/` folder of your Fish instance (the installer creates it; the launcher shows the
instance's game folder). Start the game with **Play** on the Fish instance.

- A mod you do not want right now: rename it to `name.fish.disabled`. Files starting with a dot are ignored too.
- A mod can be a plain folder containing `fish.mod.toml`.
- Mods that need other mods (or a newer Fish) say so in the game log and are skipped; the rest still load.
- **Options > Mods** lists every loaded Fish mod as `Name 1.0 [Fish]`.
- A mod that crashes is switched off for the session; the game keeps running. Logs are in the instance's `logs/latest.log`
  (lines start with `[fish]` or `[fish/<mod id>]`).

Mods are native code with the same rights as the game. Install only mods you trust.
