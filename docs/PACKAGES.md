# Пакеты YAM

YAM использует `.yap` как переносимый пакет для колонки. Runtime принимает прежний ZIP и рекомендуемый gzip-сжатый TAR (`tar.gz`) с манифестом, Lua-скриптами, ресурсами и, при необходимости, исполняемым AArch64/armhf бинарником. Расширение в обоих случаях остаётся `.yap`.

Создать TAR-вариант вручную можно так:

```sh
tar -C package-source -czf my-package.yap plugin.toml main.lua assets/
```

## Манифест

```toml
id = "ambient-lights"
name = "Ambient lights"
version = "1.0.0"
entry = "launch.lua"
permissions = ["led"]

[launch]
kind = "lua"
mode = "service"
path = "launch.lua"

[compatibility]
features = ["led.ring.rgb"]
```

`entry` обязателен и остаётся точкой входа по умолчанию. Поле `launch.path` переопределяет его для запуска. Все пути должны быть относительными путями внутри пакета.

## Режимы запуска

- `oneshot` — стандартный режим. Пакет запускается командой `yam run <id>` и завершает работу сам.
- `service` — пакет предназначен для фоновой работы. После включения он запускается автоматически, а после старта `yamd` восстанавливается, если был включён до перезапуска.

Оба режима отслеживаются в `yam ps`. Остановка выполняется `yam stop <id>`. Для Lua остановка происходит при следующем `alice.sleep` или `alice.set`; бинарный процесс получает принудительное завершение.

## Lua-пакет

Укажите `kind = "lua"`. Скрипт получает `alice`, `device` и `event`. Долгоживущие пакеты должны регулярно вызывать `alice.sleep`.

```lua
while true do
  alice.set({"#3D7EFF", "#6F40FF", "#21D4FD"})
  alice.sleep(250)
end
```

## Бинарный пакет

Бинарный пакет требует permission `exec` и запускается из корня распакованного пакета. Его stdout и stderr записываются в `runtime.log` пакета.

```toml
id = "sensor-agent"
name = "Sensor agent"
version = "1.0.0"
entry = "launch.lua"
permissions = ["exec"]

[launch]
kind = "binary"
mode = "service"
path = "bin/sensor-agent"
args = ["--interval", "10"]
```

Бинарник обязан соответствовать архитектуре и libc целевой колонки. Для Station Lite используйте AArch64 musl.

Дополнительные поля `[launch]` для бинарных пакетов:

- `interpreter = "sh"` — запустить `path` как shell-скрипт (`sh <path> <args>`); другие интерпретаторы не принимаются;
- `library_paths = ["opt/lib"]` — относительные каталоги пакета, которые добавляются в начало `LD_LIBRARY_PATH`.

Если оба поля не заданы, YAM для совместимости с пакетами, сконвертированными ранее, сам ищет скрипт `<id>/<id>-run` и каталог `lib` рядом с каталогом исполняемого файла. Новые пакеты должны указывать эти поля явно; `tools/convert_apkg.py` делает это автоматически.

### API Alice для бинарников

Запущенный binary-пакет получает одноразовые переменные окружения `YAM_API_URL` и `YAM_API_TOKEN`. Токен действует только пока работает этот экземпляр пакета; передавайте его в заголовке `X-YAM-Token`. API повторно проверяет permission из `plugin.toml`.

```sh
# Нужны permissions = ["exec", "led", "sensors", "clock"]
curl -H "X-YAM-Token: $YAM_API_TOKEN" "$YAM_API_URL/device"
curl -H "X-YAM-Token: $YAM_API_TOKEN" "$YAM_API_URL/sensors"
curl -X POST -H "X-YAM-Token: $YAM_API_TOKEN" -H 'content-type: application/json' \
  -d '{"colors":["#4285F4","#EA4335","#FBBC05"]}' "$YAM_API_URL/led"
```

Доступны `GET /device` (`exec`), `GET /sensors` (`sensors`), `POST /led`, `POST /led/off`, `POST /logo`, `POST /logo/off` (`led`; logo — 6 цветов, только при `led.logo.rgb`) и `POST /clock` (`clock`). Форматы LED и clock совпадают с HTTP API YAM.

Для бинарного `oneshot`-пакета можно передать аргументы конкретного запуска. Они добавляются после статических `launch.args`:

```sh
yam run sensor-agent -- --interval 10 --verbose
```

Аргументы запуска не сохраняются, не передаются Lua-пакетам и намеренно недоступны service-пакетам.

### Интерактивные TUI

Для бинарного `oneshot`-пакета, например `nano`, доступен интерактивный терминал. Пакет должен быть включён, иметь permission `exec`, а настройка daemon `allow_plugin_binaries` — включена.

```sh
yam tui nano
yam tui nano -- /tmp/note.txt
```

Команда использует единственную подключённую через ADB колонку; при нескольких устройствах задайте `ADB_SERIAL`. Каталог пакета на колонке CLI получает из `GET /api/plugins` (поле `directory`).

## Конвертация apkg

Ветка `pkg` репозитория `initialfox/yandex-station-repo` содержит `packages.json` и tar-архивы пакетов. Скрипт `tools/convert_apkg.py` загружает индекс, проверяет MD5, безопасно распаковывает выбранный tar, определяет исполняемый файл по полям `check`, `dest` и `bin`, затем создаёт `.yap` с бинарным launch-манифестом.

```sh
python3 tools/convert_apkg.py \
  --index https://raw.githubusercontent.com/initialfox/yandex-station-repo/pkg/packages.json \
  --package htop \
  --output htop.yap
yam install htop.yap
yam enable htop
yam run htop
```

Для уже скачанных tar-архивов используйте `--asset-dir ./assets`. Если автоматический поиск исполняемого файла неоднозначен, задайте путь внутри tar через `--program bin/htop`. Конвертер не переносит старые абсолютные `dest` пути: YAM запускает программу только из каталога пакета.

## CLI

`yam` обращается к `yamd` по HTTP. По умолчанию используется `http://127.0.0.1:8099`; другой адрес задаётся через `--url` или `YAM_URL`.

```sh
yam install ambient-lights.yap
yam list
yam enable ambient-lights
yam run ambient-lights
yam run -d sensor-agent
yam ps
yam logs sensor-agent
yam stop ambient-lights
yam disable ambient-lights
yam rm ambient-lights
```
