# Решение проблем

**«Fish has no build for Horizon <tag>».** Экземпляр играет версией Horizon, для которой у Fish нет издания (например,
после обновления лаунчером). Запустите `fish-installer repair` (он заново закрепит версию) или верните версию
экземпляра на ту, для которой ставился Fish.

**«Fish edition … is damaged (checksum mismatch)».** Файлы издания изменены. Запустите `fish-installer repair`.

**Установщик отказывает: «already runs through another wrapper».** У названного экземпляра уже есть своя обёртка
(например, `nice -n 5`). Укажите другой `--instance` или `--force`, чтобы заменить её.

**Лаунчер не найден.** Передайте `--launcher <путь к horizon-launcher>` или задайте `FISH_LOADER_HORIZON_LAUNCHER`.

**«No release signing key» / ошибки подписи.** Ставятся только подписанные издания. Для локально собранного издания
используйте `--edition <zip> --allow-unsigned`.

**Мод не появляется.** Смотрите в `logs/latest.log` строки `[fish]`: нехватка зависимости, мод для другого ABI Fish
(`was built for Fish ABI X.Y; this Fish serves A.B`) или другой платформы (`has no native library for …`) сообщаются с
именем мода. `cargo fish check <файл>` показывает то же без запуска игры.

**Обёртка не запускает игру.** Смотрите `<домашний каталог Fish>/logs/fish-launch.log` и консоль лаунчера.

**Долго белое окно при старте.** Horizon готовит ресурсы до первого кадра (десятки секунд при холодном старте). Fish
почти ничего к этому не добавляет; сравните со строкой `resources loaded in …` в журнале.
