# События сообщений

`fish_api::event::message`. События allow — первый не-Pass: `Fail` отбрасывает сообщение.

## Сервер

| Событие | Поля |
| --- | --- |
| `server::ALLOW_CHAT`, `CHAT` | `actor`, `subject` = текст; `CHAT` приходит, когда сообщение разослано |
| `server::ALLOW_GAME`, `GAME` | `subject` = текст системного сообщения |
| `server::ALLOW_COMMAND`, `COMMAND` | `actor`, `subject` = строка команды; `COMMAND` приходит после выполнения |

## Клиент

| Событие | Поля |
| --- | --- |
| `client::ALLOW_RECEIVE`, `RECEIVE` | `subject` = простой текст, `extra` = `player`, `server` или `client` |
| `client::ALLOW_SEND`, `MODIFY_SEND`, `SEND` | `subject` = то, что набрал игрок |

`MODIFY_SEND` выполняется, если никто не отменил `ALLOW_SEND`; вызовите `ctx.replace_text("...")`, чтобы изменить
строку. Побеждает последняя замена. `SEND` видит итоговый текст.

```rust
use fish_api::event::message::client::MODIFY_SEND;

MODIFY_SEND.register(|c| {
    let text = c.subject().replace(":fish:", "><>");
    c.replace_text(&text);
})?;
```

## Замечания

- Переписывать можно только исходящий чат клиента; полученные строки и сообщения сервера можно отменить, но не изменить.
- Команды, начинающиеся с `/`, не проходят через `MODIFY_SEND`; используйте события команд или [команды](commands.md).
