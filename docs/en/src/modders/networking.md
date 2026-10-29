# Networking

`fish_api::networking` sends custom payloads (a channel id plus bytes) between client and server.

```rust
use fish_api::networking::*;

PayloadTypeRegistry::play_s2c().register("mymod:hello", 1024)?;
PayloadTypeRegistry::play_c2s().register("mymod:reply", 1024)?;
ClientPlayNetworking::register_global_receiver("mymod:hello", |ctx| {
    let name = PacketReader::new(ctx.data()).read_string().unwrap_or_default();
    let _ = ClientPlayNetworking::send("mymod:reply", PacketBuf::new().string(&name).as_bytes());
})?;
ServerPlayNetworking::register_global_receiver("mymod:reply", |ctx| {
    log::info!("{} answered", ctx.player_name());
})?;
```

- Declare each type per direction first (`play_s2c`, `play_c2s`, `configuration_s2c`, `configuration_c2s`) with a
  maximum size. Clientbound payloads are at most 1 MiB, serverbound 32,767 bytes. Larger payloads are refused on send
  (`INVALID_ARGUMENT`) and dropped, with a warning, on receipt.
- One receiver per channel and direction. `ServerPlayNetworking`, `ServerConfigurationNetworking`,
  `ClientPlayNetworking` and `ClientConfigurationNetworking` offer `register_global_receiver`, `send`
  (`send_all` on servers), `can_send` (and `players` on servers).
- `PacketBuf` / `PacketReader` write and read big-endian numbers, varints and strings.
- Connection events: `event::networking::{client,server}::{INIT, JOIN, DISCONNECT}` (`actor()`, `subject()` = reason).

## Handshake and registry sync

In the configuration phase both sides exchange `fish:handshake` (Fish version, ABI, mods, registry hash, the channels
each side receives). Consequences:

- `can_send` is true only for channels the other side announced; `send_all` skips peers that cannot receive; a direct
  send to such a peer fails with `UNSUPPORTED`.
- Mods that run on both sides (`environment = "*"`) **and registered synchronised registry entries** must have the same
  version on both sides, otherwise the client is disconnected with
  `Missing or different Fish mods: a 1.0 (server) / none (client)`. Mods without such content need not match.
- The server then sends `fish:registry_sync`; the client renumbers its registries to the server's raw ids. Entry
  sets that differ disconnect the client with the missing ids named.
- A client without Fish may join a server that has nothing to synchronise; otherwise it is disconnected at the start of
  play. A Fish client with synchronised content refuses a server that does not answer the handshake.

## Status of the game side

The loader side (types, receivers, limits, handshake, registry sync, connection events) is complete and tested over an
in-memory network (`fish_test_host::net::Network`). **Wiring to Horizon's connections is not done yet**: the edition has
no hooks for custom payloads, so payloads do not cross a real connection until it lands (roadmap P7.1, P7.2).
