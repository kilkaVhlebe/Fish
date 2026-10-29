# События

`fish_api::event::Event<P, R>` повторяет `Event<T>` из Fabric. События идентифицируются id вида
`namespace:path` (namespace `[a-z0-9_.-]+`, path `[a-z0-9_./-]+`). События самого Fish называются
`fish:<module>/<event>` и доступны как статики в `SCREAMING_SNAKE`.

```rust
static PING: Event<Ping> = Event::new("hello:ping", Combine::Notify);
static ASK: Event<Question, ActionResult> = Event::new("my:ask", Combine::FirstNonPass);
```

## Режимы объединения

- `Notify`: выполняются все слушатели. Нагрузка — `&mut P`, поэтому слушатели могут её менять.
- `FirstNonPass`: диспетчеризация останавливается на первом слушателе, вернувшем не `ActionResult::Pass`
  (`Pass`, `Success`, `Consume`, `Fail`). Тип результата должен начинаться с байта действия
  (`ActionResult`, `TypedActionResult<T>`).

## Фазы

Каждый слушатель принадлежит фазе — `fish:default`, если не указано иное:

```rust
EVENT.register_phase(Phase("mymod:early"), |p| { /* … */ })?;
EVENT.add_phase_ordering(Phase("mymod:early"), Phase::DEFAULT)?;   // early выполняется до default
```

Фазы образуют граф, упорядоченный `add_phase_ordering`; порядок — топологическая сортировка, ничьи
разрешаются порядком первого появления фаз. Цикл пишет предупреждение с именем события и фаз и
возвращается к порядку первого появления. Внутри фазы слушатели идут в порядке регистрации.

## Свои события

`Event::define` (вызывается автоматически) идемпотентен, поэтому несколько модов могут назвать одно
событие. Повторное определение с другим режимом объединения даёт `ALREADY_EXISTS`.
`Event::invoke(&mut payload)` запускает слушателей и возвращает объединённый результат.

## Нагрузки

```rust
#[repr(C)]
pub struct Ping { pub size: u32, pub flag: u32 }   // первое поле: size_of::<Ping>()
unsafe impl Payload for Ping {}
```

Нагрузки — `#[repr(C)]`, начинаются с `size: u32`, содержат только простые данные и дескрипторы хоста и
растут только добавлением полей. Слушатель, собранный под большую нагрузку, чем прислал хост, отклоняется
(`INVALID_ARGUMENT`), а не читает за границей.

## Отказы

Слушатель с паникой возвращает `PANICKED`; загрузчик помечает мод как failed, пишет
`Fish mod <name> stopped` и пропускает слушателей этого мода. Слушатели других модов продолжают работать.
