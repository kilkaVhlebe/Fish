# Интеграция с лаунчером

Fish касается лаунчера Horizon единственным способом: публичной командной строкой `horizon-launcher`.

| Зачем | Команда |
| --- | --- |
| Установленные версии | `horizon-launcher versions --installed --json` |
| Установить сборку Horizon | `horizon-launcher install <тег релиза>` |
| Список экземпляров | `horizon-launcher instance list --json` |
| Создать экземпляр | `horizon-launcher instance create <name>` |
| Закрепить версию | `horizon-launcher instance version <name> <установленный тег>` |
| Прочитать / задать обёртку | `horizon-launcher instance launch <name> --json` / `… --wrapper "\"<path>\""` |
| Выбрать его | `horizon-launcher instance select <name>` |

Fish никогда не пишет `instances.json`, `settings.json`, `versions/`, `accounts.json` и `native/`.

## Обёртка

Экземпляр может запускать игру *через* программу. Если обёртка — `fish-launch`, лаунчер запускает
`fish-launch <…/versions/<tag>/horizon-client> <args…>` в папке экземпляра с окружением экземпляра и аргументами сессии.
`fish-launch`:

1. выводит тег версии из пути программы (папка под `versions/`; раскладка macOS-бандла учтена),
2. выбирает установленное издание для этой версии Horizon (префикс коммита для тегов `nightly-*` и `native-*`,
   `release_tag` для `v*`; побеждает новейший Fish),
3. проверяет SHA-256 бинарника издания,
4. запускает его с теми же аргументами и `FISH_LOADER_HOME`. В Windows игра привязана к Job Object
   (`KILL_ON_JOB_CLOSE`), чтобы «Стоп» в лаунчере её останавливал; в Unix обёртка делает `exec`.

Ошибки печатаются в консоль лаунчера и в `<домашний каталог Fish>/logs/fish-launch.log`, код выхода 2.

## Правила для экземпляра

- нет: создаётся;
- есть с обёрткой `fish-launch` или без обёртки: используется;
- есть с другой обёрткой: отказ, если нет `--force`.

`uninstall` очищает обёртку (`--wrapper ""`); если версия лаунчера это отвергает, установщик сообщает об этом и просит
очистить её в панели *Launch…* лаунчера, но свои файлы всё равно удаляет.

## Версии

Издания Fish собираются под один коммит Horizon. Когда лаунчер обновляет экземпляр до более нового Nightly,
`fish-launch` отказывает с понятным сообщением, пока `fish-installer update`/`repair` не доставит издание для него.
