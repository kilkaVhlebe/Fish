# Particle types

`fish_api::particle` registers particle types: the server-side half of a particle, an id and a number that payloads can
use.

```rust
use fish_api::particle::Particles;

fn init() -> Result<(), fish_api::Error> {
    let sparkle = Particles::register("content_demo:ruby_sparkle")?;
    // Particles::register_with(id, true) draws it even when the player asked for minimal particles.
    let _ = sparkle;
    Ok(())
}
```

- **`Particles::register(id)`** / **`register_with(id, always_show)`** register the type (`namespace:path`, the namespace
  is the mod's id) and return a `ParticleType`.
- **`ParticleType::raw_id()`** is its raw id in the Fish registry `fish:particle_types`. It is synchronised with a
  server for mods that run on both sides, so it is renumbered on joining one: ask for it while the game runs, do not
  store it.

The mod's `assets/<mod id>/particles/<path>.json` (the texture list, as in a resource pack) and its textures under
`assets/<mod id>/textures/particle/` are copied into the content folder; the loader warns when the description is
missing.

```json
{ "textures": ["content_demo:ruby_sparkle"] }
```

## Rules

- **Registration** is allowed while `pre_launch` or `main` runs; a type registered twice fails with `ALREADY_EXISTS`.

## Limits

- Horizon's particles are a closed set that it renders itself: a type of a mod has **no client factory yet**, so nothing
  is drawn for it and there is no call that spawns one. Both come with the rendering phase (P10, particle factories).
- Testing: the expectation `particles` of [Testing mods](testing.md).
