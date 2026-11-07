# Сборка и отладка изданий

```text
cargo xtask horizon fetch            # .horizon/source/<commit>/ (идемпотентно)
cargo xtask mixins install           # horizon-mixins на ревизии из fish.toml [mixins].rev
cargo xtask edition stage            # копия исходников + крейты fish, правка Cargo.toml, обновление lock
cargo xtask edition mixins           # сборка пакета, horizon-mixins apply
cargo xtask edition build --profile fast
cargo xtask edition package --profile fast     # dist/fish-edition-….zip
cargo xtask edition selftest --profile fast    # запускает бинарник с hello.fish и content_demo.fish
cargo xtask edition all --profile fast         # всё перечисленное
cargo xtask edition regressions                # собственные тесты Horizon: pristine против staged
```

Всё лежит в `.horizon/` (в git-ignore): `source/<commit>`, `work/<commit>/{src,bundle,staged}`, `tools/<rev>`,
`target`. Первая сборка компилирует всю игру (несколько минут); `fast` линкуется быстро, `release` — thin-LTO.
В Windows сборка идёт с `RUSTFLAGS=-C target-feature=+crt-static`, как в релизном workflow Horizon.

## Диагностика horizon-mixins

При ошибке `apply` печатает один JSON-объект. `code` говорит, что не так: `E_UNSUPPORTED_MACRO` (в цели есть макрос:
вешайтесь на вызываемую функцию без макросов), `E_MODULE` (нагрузка вспомогательного модуля не разбирается или содержит
атрибуты файла вроде `//!`: используйте обычные комментарии `//`), `E_MODULE_CONFLICT`, отсутствующая цель.
`injection_id` и `target` называют проблемный хук. Цели включают имя Cargo-таргета:
`horizon_client::game::native_hooks::Game<'_>::native_tick`.
`horizon-mixins plan --workspace .horizon/work/<commit>/src --mods .horizon/work/<commit>/bundle --out plan.json`
показывает полный план без записи stage.

## Правка моста

`horizon/mixins/fish/`: `horizon-mixin.toml` (заполнители `@FISH_VERSION@`, `@HORIZON_COMMIT@`), `helpers/*.rs` (по одной
функции, компилируется как локальная функция внутри целевой), и `modules/bridge/*.rs` (файлы оборачиваются в
встроенные `mod` и склеиваются: horizon-mixins отклоняет внешние `mod` и `include!`).

## Перепривязка Horizon

Измените `[horizon]` в `fish.toml` (или запустите `cargo xtask horizon pin --latest-dated`), пересоберите через
`edition all`. Если horizon-mixins сообщает об исчезнувшей цели, поправьте пакет: её называет `plan`. Точки хуков
семейства `native_*` Horizon только добавляет, поэтому более новый коммит обычно работает сразу.

## Закрепление и установщик

`cargo xtask horizon pin --latest-dated [--check] [--no-build]` находит новейший релиз `nightly-YYYY-MM-DD`, отказывает,
если коммит старее закреплённого или в нём нет точек `native_mods`, переписывает `[horizon]` в `fish.toml` и
пересобирает. `--check` только сообщает. `cargo xtask dist installer` собирает `fish-launch`, затем однофайловый
`fish-installer` с ним внутри (`--features embed-launch`, `FISH_LAUNCH_BIN`) в `dist/`.

## О ревизии horizon-mixins

Лаунчер на закреплённом коммите Horizon собирает нативные моды через horizon-mixins `0500ff2`. Fish закрепляет `0d6b103` —
следующий коммит, добавляющий необязательную публичную видимость вспомогательных модулей (понадобится, когда хуки
появятся в других крейтах).

## Патчи

Четыре точки сети находятся в функциях, которые horizon-mixins отвергает (непрозрачные макросы, `?` с замыканиями).
Это unified diff в `horizon/patches/0001..0004`; `edition stage` применяет их после `git apply --check`. При смене
версии Horizon упавший патч правится вручную; каждый добавляет вызов в `fish_bridge::net`.

`0005-gamerule-screen.patch` — не точка хука: экран игровых правил хранит строки и состояние ввода в приватных полях одной
структуры, поэтому для правил модов нужны новые поля (`RulesScreen::with_fish`). Патч вызывает
`fish_bridge::rules::world_of` из `game/lan_screen.rs`.

`0006-content-mods.patch` тоже не точка хука: Horizon строит оверлей контента из модов своей папки модов, поэтому
`game/mods.rs::content_root` берёт ещё и папки, которые загрузчик записал в `<папка игры>/.fish/content/`, и считает их
всегда включёнными контент-модами. `0007-block-ticks.patch` заставляет симуляцию блоков horizon-world (`Sim`) сообщать
об обновлениях соседей, случайных и запланированных тиках блоков Fish в `Dispatch::block_event`; это состояние
симуляция держит приватно, хук до него не дотянется.

`0008-creative-inventory.patch` заставляет `Creative::load` пропускать текст `creative-inventory.json` через
`fish_lib::creative_patch` (`Dispatch::creative_inventory`), прежде чем он станет вкладками. Хук тут не справится:
`Creative::from_json` возвращает `Result<Self>`, который horizon-mixins не умеет отменять, и принимает ссылку, которую
нельзя подменить.

`0009-content-tables.patch` применяет таблицы контента модов (`fish:content`) там, где Horizon строит свои таблицы:
`load_world_rest` (таблица предметов и данные мобов, через `fish_lib::content_items` и `fish_lib::content_mobs`),
`Transformers::load` (каждый `block_transformer/<tool>.json` через `fish_lib::transformer_list`), `sculk::frequency`
(`fish_lib::sculk_frequency`) и `cached` в horizon-world `level/data.rs` (шансы огня в загруженных `Behaviours` после
снимка, так что снимок хранит ванильные). Хуки не справятся: в `load_world_rest` и `Transformers::load` есть макросы и
`?`, в `sculk::frequency` — замыкания (`E_UNSUPPORTED_CONTROL_FLOW`), а поведения загружаются в обобщённом коде.

`0010-block-state-properties.patch` позволяет объявлению блока контент-мода нести `state_properties` (имя, значения,
значение по умолчанию) и заставляет `mod_content.rs` выдавать каждое состояние шаблона по разу на комбинацию их
значений: строки `facts.json` (состояние и описание) и `states` NBT-таблиц получают добавленные свойства,
`cached_shapes` повторяется, `model-states.json` получает схему и возможные состояния, `render-definitions.json` — id и
значение по умолчанию. Без добавленных свойств он ничего не меняет. Это не точка хука: построитель оверлея — один
приватный план.

`0011-resources-reloaded.patch` добавляет один вызов в конец `Game::apply_staged_pack` (подмена после перезагрузки
ресурсов, F3+T или смена паков): `fish_bridge::events::resources_reloaded` поднимает
`fish:lifecycle/resources_reloaded`. В функции есть замыкания и `?`, поэтому horizon-mixins не может её перехватить.

## Самопроверка и внутриигровой тест

`edition selftest` запускает собранный бинарник с `FISH_LOADER_ADD_MODS` (`hello` и `content_demo`) и
`FISH_LOADER_SELFTEST`: загрузчик стартует внутри edition, прогоняет 20 синтетических тиков, кадр, проход HUD и команду и
пишет отчёт, прежде чем игра откроет окно. Кроме проверок hello в нём есть `content_blocks` (блоки примера и свойства
состояния лампы), `content_folder` (папка контента для Horizon с объявлениями, звуками, частицами и blockstates) и
`content_tables`. `cargo xtask e2e --resources <папка 26.3> --horizon-ctl <путь>` управляет настоящим окном игры через
horizon-ctl, проверяет события, игровые правила, блоки, предметы, таблицы контента, звуки и свойства состояния модов-
примеров в `latest.log` и в мире, а затем снова открывает сохранённый мир.
