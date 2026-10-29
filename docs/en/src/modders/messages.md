# Message events

`fish_api::event::message`. Allow events are first non-pass: `Fail` drops the message.

## Server

| Event | Fields |
| --- | --- |
| `server::ALLOW_CHAT`, `CHAT` | `actor`, `subject` = text; `CHAT` fires when the message was broadcast |
| `server::ALLOW_GAME`, `GAME` | `subject` = text of a system message |
| `server::ALLOW_COMMAND`, `COMMAND` | `actor`, `subject` = command line; `COMMAND` fires after it ran |

## Client

| Event | Fields |
| --- | --- |
| `client::ALLOW_RECEIVE`, `RECEIVE` | `subject` = plain text, `extra` = `player`, `server` or `client` |
| `client::ALLOW_SEND`, `MODIFY_SEND`, `SEND` | `subject` = what the player typed |

`MODIFY_SEND` runs when no listener cancelled `ALLOW_SEND`; call `ctx.replace_text("...")` to change the line. The
last replacement wins. `SEND` sees the final text.

```rust
use fish_api::event::message::client::MODIFY_SEND;

MODIFY_SEND.register(|c| {
    let text = c.subject().replace(":fish:", "><>");
    c.replace_text(&text);
})?;
```

## Notes

- Rewriting is offered for outgoing client chat only; received lines and server messages can be cancelled, not edited.
- Commands starting with `/` do not go through `MODIFY_SEND`; use the command events or [commands](commands.md).
