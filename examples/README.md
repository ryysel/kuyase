# Пример Headless-тестирования

Демо-скрипт [`test_yam_lineup.py`](./test_yam_lineup.py) тестирует автоопределение модели колонки (`detect_platform`) в установщике пакетного менеджера для Яндекс Станций [`nyakokitsu/yam`](https://github.com/nyakokitsu/yam) сразу по всем собранным в `work/` образам (`*-system.ext4`) через `kuyase run --headless --board all --snapshot --exec "..."`.

Скрипт скачивает `install.sh`, без модификаций вырезает из него функции `device_identity()` и `detect_platform()`, запускает их внутри каждой виртуальной колонки и сверяет результат с эталонным профилем YAM v0.2+ / Luna с учётом SoC (`amlogic,axg` vs `amlogic,a5`):

>[!warning]
>Внутреннее имя `yandex,station-midi` и `"deviceType": "yandexmidi"` в `/system/vendor/quasar/quasar.cfg` на SoC **Amlogic AXG** принадлежит розничной **Яндекс Станции 2** (`YNDX-00051`, профиль пакетов `yandexstation_2`), а розничная **Яндекс Станция Миди** (`YNDX-00054`) построена на SoC **Amlogic A5** и имеет кодовое имя **`cucumber`**. Поэтому полагаться только на `quasar.cfg` без проверки SoC (`/proc/device-tree/compatible`) нельзя.

## Легенда вывода

* `compatible` — строка из `/proc/device-tree/compatible` (SoC + идентификатор платы);
* `quasar.cfg` — поле `"deviceType"` из `/system/vendor/quasar/quasar.cfg`;
* `expected` — эталонный профиль YAM v0.2+ / Luna для данной платы и SoC;
* `yam_profile` — профиль, который вернула функция `detect_platform()` из `install.sh`.

## Запуск

```bash
# Проверить все образы в папке work/ против ветки main репозитория YAM:
python examples/test_yam_lineup.py

# Проверить только конкретную модель (например, station2 или cucumber):
python examples/test_yam_lineup.py --board station2
```
