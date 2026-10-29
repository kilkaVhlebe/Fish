# Сеть

`fish_api::networking` отправляет пользовательские данные (id канала и байты) между клиентом и сервером.

```rust
use fish_api::networking::*;

PayloadTypeRegistry::play_s2c().register("mymod:hello", 1024)?;
PayloadTypeRegistry::play_c2s().register("mymod:reply", 1024)?;
ClientPlayNetworking::register_global_receiver("mymod:hello", |ctx| {
    let name = PacketReader::new(ctx.data()).read_string().unwrap_or_default();
    let _ = ClientPlayNetworking::send("mymod:reply", PacketBuf::new().string(&name).as_bytes());
})?;
ServerPlayNetworking::register_global_receiver("mymod:reply", |ctx| {
    log::info!("{} ответил", ctx.player_name());
})?;
```

- Сначала объявите каждый тип для направления (`play_s2c`, `play_c2s`, `configuration_s2c`, `configuration_c2s`) с
  максимальным размером. Клиентские данные — до 1 МиБ, серверные — до 32 767 байт. Бо́льшие отклоняются при отправке
  (`INVALID_ARGUMENT`) и отбрасываются с предупреждением при получении.
- Один получатель на канал и направление. У `ServerPlayNetworking`, `ServerConfigurationNetworking`,
  `ClientPlayNetworking` и `ClientConfigurationNetworking` есть `register_global_receiver`, `send` (на сервере ещё
  `send_all`), `can_send` (и `players` на сервере).
- `PacketBuf` / `PacketReader` пишут и читают числа big-endian, varint и строки.
- События соединения: `event::networking::{client,server}::{INIT, JOIN, DISCONNECT}` (`actor()`, `subject()` = причина).

## Рукопожатие и синхронизация реестров

В фазе конфигурации стороны обмениваются `fish:handshake` (версия Fish, ABI, моды, хеш реестров, каналы, которые
сторона принимает). Следствия:

- `can_send` истинно только для каналов, объявленных другой стороной; `send_all` пропускает тех, кто не примет;
  прямая отправка такому получателю даёт `UNSUPPORTED`.
- Моды, работающие на обеих сторонах (`environment = "*"`) **и зарегистрировавшие синхронизируемые записи реестров**,
  должны совпадать по версии, иначе клиента отключают:
  `Missing or different Fish mods: a 1.0 (server) / none (client)`. Моды без такого содержимого могут отличаться.
- Затем сервер шлёт `fish:registry_sync`; клиент перенумеровывает реестры по серверным raw id. Различающиеся наборы
  записей отключают клиента с перечнем недостающих id.
- Клиент без Fish может зайти на сервер, которому нечего синхронизировать; иначе его отключают в начале игры.
  Клиент Fish с синхронизируемым содержимым отказывается от сервера, не ответившего на рукопожатие.

## Состояние игровой части

Часть загрузчика проверена на сетевой модели в памяти (`fish_test_host::net::Network`). Издание подключает её к
соединениям Horizon (конфигурация и игра клиента, LAN-гости, память встроенного сервера); издание собирается,
selftest и e2e проходят. **В настоящей игре пока не проверено:** обмен данными, рукопожатие с реальным партнёром,
LAN-гость. Серверные отправки в фазе игры задерживаются до одного тика сервера (50 мс); у миров `--in-frame` нет
соединения, а значит и сети.
