# Fish

Fish is a Fabric-style mod loader for [Horizon](https://github.com/horizon-rust/horizon),
Minecraft: Java Edition rewritten in Rust.

- Players run one installer, press Play on the "Fish" instance in the Horizon launcher,
  and drop `.fish` mods into `fish-mods/`.
- Modders build native mods with `cargo fish` against a documented, versioned API.
- Fish never touches launcher-owned files; it plugs in as the instance's launch wrapper.

> Fish is not affiliated with Horizon, Mojang or Microsoft.

**Status:** early development. See the [roadmap](docs/en/src/roadmap.md).
Russian version of this file: [README.ru.md](README.ru.md).

## Documentation

- English: `docs/en` (mdBook)
- Русский: `docs/ru` (mdBook)

## License

MIT, see [LICENSE](LICENSE).
