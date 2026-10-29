# Fish

Fish is a Fabric-style native mod loader for [Horizon](https://github.com/horizon-rust/horizon),
Minecraft: Java Edition rewritten in Rust.

- **Players** run one installer, press Play on the "Fish" instance in the Horizon launcher and put
  `.fish` mods into `fish-mods/`.
- **Modders** build mods with `cargo fish` against a stable, versioned C ABI and a layered Rust API.
- Fish never edits files owned by the Horizon launcher.

Fish is not affiliated with Horizon, Mojang or Microsoft. The [roadmap](roadmap.md) shows what
exists today.
