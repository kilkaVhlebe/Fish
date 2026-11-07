# Ресурсы

`fish_api::resource` — загрузчик ресурсов Fish (аналог resource loader API из Fabric): `assets/` и `data/` мода попадают
в игру, мод читает их, а также читает активные ресурсы игры.

## Файлы мода в игре

Каждый файл в `assets/` и `data/` архива мода попадает в игру, регистрирует мод блоки и предметы или нет: загрузчик кладёт
их в папку контента мода (`<папка игры>/.fish/content/<id_мода>/`), которую Horizon читает как папку контент-мода. Так
работают текстуры, модели, blockstates, звуки, переводы, таблицы добычи, рецепты и теги:

```text
assets/content_demo/lang/en_us.json
assets/content_demo/sounds.json
data/content_demo/loot_table/blocks/ruby_block.json     рубиновый блок роняет рубин
data/minecraft/tags/block/mineable/pickaxe.json         дополнения к ванильным и c:-тегам
```

Файлы чужих пространств имён пропускаются с предупреждением (см. [блоки](block.md#файлы)).

## Чтение файлов

```rust
use fish_api::event::lifecycle;
use fish_api::resource::Resources;

fn init() -> Result<(), fish_api::Error> {
    let sounds = Resources::own_text("assets/content_demo/sounds.json")?;
    let files = Resources::list_own("assets/content_demo/")?;
    log::info!("{} asset files, ruby_chime: {}", files.len(), sounds.contains("ruby_chime"));
    lifecycle::resources::RELOADED.register(|c| {
        if let Ok(text) = Resources::game_text(&c.client(), "assets/minecraft/lang/en_us.json") {
            log::info!("the game's en_us.json is {} bytes now", text.len());
        }
    })?;
    Ok(())
}
```

- **`Resources::own(path)`** / **`own_text`** читают файл из архива самого мода, начиная с `pre_launch`.
  **`list_own(prefix)`** перечисляет файлы под префиксом в порядке архива.
- **`Resources::game(client, path)`** / **`game_text`** читают файл активного корня ресурсов игры: ванильные ресурсы с
  выбранными ресурспаками и контентом всех модов поверх, как их использует игра. Нужна игра события (`c.client()`); у
  выделенного сервера корня ресурсов нет (`UNSUPPORTED`).
- Пути — `assets/<namespace>/...` или `data/<namespace>/...` без компонентов `.` и `..`; файлы не больше 64 МиБ.
  Отсутствующий файл — `NOT_FOUND`.

## Перезагрузки

`lifecycle::resources::RELOADED` срабатывает в окне после перезагрузки его ресурсов (в настройках сменились ресурспаки,
F3+T): файлы игры могли измениться. Перезагрузки данных сервера (`/reload`) — это `lifecycle::reload::START` и `END`
([события жизненного цикла](lifecycle.md)). Событие приходит из небольшого патча Horizon
(`0011-resources-reloaded.patch`) после того, как перезагрузка подменила ресурсы.

## Ограничения

- Файлы мода фиксируются на время запуска: они копируются при старте игры, а перезагрузка ресурсов перечитывает те же
  копии. Встроенных необязательных ресурспаков (`registerBuiltinResourcePack` в Fabric) пока нет.
- Слушателя перезагрузки со стадиями нет: моды получают одно событие после перезагрузки, а не участвуют в ней.
- Тестирование: шаг `resource_reload` из [Тестирования модов](testing.md); симулированный корень ресурсов содержит
  несколько ванильных заготовок с папками контента поверх.
