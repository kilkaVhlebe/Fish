# Sound events

`fish_api::sound` registers the sound events a mod's resources describe and plays them on the player's window.

The files sit in the mod's archive exactly as in a resource pack:

```text
assets/content_demo/sounds.json           {"ruby_chime": {"sounds": ["content_demo:ruby_chime"], "subtitle": "..."}}
assets/content_demo/sounds/ruby_chime.ogg  Ogg Vorbis, mono for a sound that comes from a place
```

```rust
use fish_api::event::lifecycle;
use fish_api::sound::Sounds;

fn init() -> Result<(), fish_api::Error> {
    let chime = Sounds::register("content_demo:ruby_chime")?;
    lifecycle::client::END_TICK.register(move |c| {
        let client = c.client();
        if let Ok(Some(p)) = client.player() {
            let _ = chime.play_at(&client, [p.x, p.y, p.z], 1.0, 1.0);
        }
    })?;
    Ok(())
}
```

- **`Sounds::register(id)`** registers the event (`namespace:path`, the namespace is the mod's id) and returns a
  `SoundEvent`. The event also gets a raw id in the Fish registry `fish:sound_events`, synchronised with a server like
  every [registry](registry.md) entry.
- **`SoundEvent::play(client, volume, pitch)`** plays it on the listener, **`play_at(client, pos, volume, pitch)`** at a
  position in the world. Volume and pitch are in `(0, 10]`, 1 is normal.
- **`Sounds::play(client, id, pos, volume, pitch)`** plays any event the game knows, a mod's or a vanilla one
  (`minecraft:entity.experience_orb.pickup`).

## Rules

- **Registration** is allowed while `pre_launch` or `main` runs; an event registered twice fails with
  `ALREADY_EXISTS`.
- The loader copies the mod's `assets/` into its content folder (see [blocks](block.md)), so Horizon reads the mod's
  `sounds.json` together with vanilla's. When the entrypoints are over it warns if `sounds.json` does not name a
  registered event or a file it names is missing.

## Where a sound plays

Sounds play on the client of the player's own window. A server has no audio: playing there returns `UNSUPPORTED`.
Interaction events of items and blocks run on the (integrated) server, so a mod that wants a sound for a click notes it
and plays it on the window's next tick, as `content-demo` does. An event the game does not know (the resources are
missing) is `NOT_FOUND`.

## Limits

- There is no way yet to play a sound on other players' clients (that needs a sound packet of a mod event).
- Sounds are fixed for the run: a resource pack reload re-reads `sounds.json`, but registering is only possible at boot.
- Testing: the expectations `sounds` and `played_sounds` of [Testing mods](testing.md); the simulated window knows the
  events of the `sounds.json` files in the content folders.
