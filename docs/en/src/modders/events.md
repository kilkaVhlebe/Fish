# Events

`fish_api::event::Event<P, R>` mirrors Fabric's `Event<T>`. Events are identified by
`namespace:path` ids (namespace `[a-z0-9_.-]+`, path `[a-z0-9_./-]+`). Fish's own events are named
`fish:<module>/<event>` and exposed as `SCREAMING_SNAKE` statics.

```rust
static PING: Event<Ping> = Event::new("hello:ping", Combine::Notify);
static ASK: Event<Question, ActionResult> = Event::new("my:ask", Combine::FirstNonPass);
```

## Combine modes

- `Notify`: every listener runs. The payload is `&mut P`, so listeners can modify it.
- `FirstNonPass`: dispatch stops at the first listener that does not return `ActionResult::Pass`
  (`Pass`, `Success`, `Consume`, `Fail`). The result type must start with the action byte
  (`ActionResult`, `TypedActionResult<T>`).

## Phases

Every listener belongs to a phase, `fish:default` unless given:

```rust
EVENT.register_phase(Phase("mymod:early"), |p| { /* … */ })?;
EVENT.add_phase_ordering(Phase("mymod:early"), Phase::DEFAULT)?;   // early runs before default
```

Phases form a graph ordered by `add_phase_ordering`; the order is a topological sort, ties broken by the
order in which phases were first seen. A cycle logs a warning naming the event and phases and falls back
to first-seen order. Within a phase listeners run in registration order.

## Your own events

`Event::define` (called automatically) is idempotent, so several mods can name the same event.
Redefining it with a different combine mode fails with `ALREADY_EXISTS`. `Event::invoke(&mut payload)`
runs the listeners and returns the combined result.

## Payloads

```rust
#[repr(C)]
pub struct Ping { pub size: u32, pub flag: u32 }   // first field: size_of::<Ping>()
unsafe impl Payload for Ping {}
```

Payloads are `#[repr(C)]`, start with `size: u32`, carry only plain data and host handles, and only ever
grow by appending fields. A listener compiled against a bigger payload than the host sent is refused
(`INVALID_ARGUMENT`), never read out of bounds.

## Failure

A panicking listener returns `PANICKED`; the loader marks the mod failed, logs `Fish mod <name> stopped`,
and skips that mod's listeners from then on. Other mods' listeners keep running.
