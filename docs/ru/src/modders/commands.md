# Команды

`fish_api::command`: построитель в стиле Brigadier. Дерево строится в моде и регистрируется один раз; хост сам
разбирает строку, проверяет права и приводит аргументы, и только потом вызывает ваш исполнитель.

```rust
use fish_api::command::{self, ArgKind, argument, literal};

command::REGISTER.register(|ctx| {
    let tree = literal("fishdemo")
        .then(literal("hello").executes(|c| {
            c.feedback(&format!("привет, {}", c.source_name()));
            Ok(1)
        }))
        .then(literal("add").then(
            argument("a", ArgKind::Integer { min: -100, max: 100 }).then(
                argument("b", ArgKind::Integer { min: -100, max: 100 }).executes(|c| {
                    let sum = c.get_integer("a")? + c.get_integer("b")?;
                    c.feedback(&format!("сумма = {sum}"));
                    Ok(sum)
                }),
            ),
        ))
        .then(literal("say").requires(2).then(
            argument("text", ArgKind::Greedy).executes(|c| { c.feedback(c.get_string("text")?); Ok(1) }),
        ));
    if let Err(e) = ctx.register(tree) {
        log::error!("не удалось зарегистрировать /fishdemo: {e}");
    }
})?;
```

## Два события регистрации

| Событие | Где работают команды |
| --- | --- |
| `command::REGISTER` | на сервере (и в одиночной игре), для того, кто их вводит, с его уровнем прав |
| `command::client::REGISTER` | в самом клиенте, сервер не нужен; уровней прав нет |

Оба приходят один раз, после выполнения точек входа всех модов. `ctx.side()` говорит, на какой стороне работает Fish.

## Узлы

- `literal(name)`, `argument(name, kind)`; `.then(child)`, `.executes(f)`, `.requires(level)`, `.redirect(root)`,
  `.suggests(&["a", "b"])`.
- `requires(level)` принимает 0 (все), 2 (гейм-мастер) или 4 (владелец); потомки наследуют. Узел выше уровня источника
  для него не существует: строка считается неизвестной.
- `redirect("root")` продолжает разбор в потомках другой корневой команды.
- Исполнитель возвращает `Ok(значение)` (результат команды) или `Err("сообщение")` (красным, команда провалена).
  `feedback(text)` показывает строку источнику.

| `ArgKind` | Значение |
| --- | --- |
| `Word`, `String` (допустимы кавычки), `Greedy` (остаток строки, только последний узел) | `get_string` |
| `Integer { min, max }` | `get_integer` |
| `Double { min, max }` | `get_double` |
| `Bool` | `get_bool` |
| `Player` (имя), `Entity` (селектор или имя, **не разворачивается**) | `get_string` |
| `BlockPos` (три координаты, `~` и `~n` относительно источника) | `get_block_pos` |

## Правила

- Имена: литералы `[A-Za-z0-9_.+-]{1,64}`; аргументы `[a-z][a-z0-9_]{0,63}`. В дереве не больше 4096 узлов.
- **Ваниль побеждает.** Если корень совпал с командой игры, Fish её не выполняет и пишет предупреждение. Если два мода
  зарегистрировали один корень, первый его сохраняет, второй получает предупреждение.
- Команды остановленного (упавшего) мода не выполняются.
- Подсказки: литералы и списки `.suggests(...)` предлагает загрузчик; покажет ли их чат игры — зависит от издания
  (см. заметки о хуках в разделе архитектуры).
