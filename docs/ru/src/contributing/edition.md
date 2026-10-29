# Сборка и отладка изданий

```text
cargo xtask horizon fetch            # .horizon/source/<commit>/ (идемпотентно)
cargo xtask mixins install           # horizon-mixins на ревизии из fish.toml [mixins].rev
cargo xtask edition stage            # копия исходников + крейты fish, правка Cargo.toml, обновление lock
cargo xtask edition mixins           # сборка пакета, horizon-mixins apply
cargo xtask edition build --profile fast
cargo xtask edition package --profile fast     # dist/fish-edition-….zip
cargo xtask edition selftest --profile fast    # запускает бинарник с hello.fish
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
