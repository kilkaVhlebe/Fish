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

## Game events

The events of [lifecycle](lifecycle.md), [interaction](interaction.md), [entity](entity.md) and [messages](messages.md)
carry one `GameContext`. Its accessors are filled per event (the others return zero or empty):

| Accessor | Meaning |
| --- | --- |
| `client()` | the game that raised it (`ClientHandle`); fails for detached events (chunks, client stopped) |
| `role()`, `is_client()` | which game; true for the window's prediction of a server decision |
| `actor()`, `actor_uuid()` | the acting player |
| `pos()`, `face()`, `hand()` | block position (chunk coordinates for chunk events), face, hand |
| `entity_id()`, `amount()` | entity network id; damage |
| `dimension()`, `subject()`, `item()`, `extra()` | dimension id; block, entity type, command or text; item id; qualifier |
| `replace_text(text)` | rewrites the text of events that allow it (`message::client::MODIFY_SEND`) |

Allow events use `ActionResult`: `Fail` cancels. The first listener that does not return `Pass` decides.
