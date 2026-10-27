# Упаковка

Исходник пакета содержит `yam.yaml` и файлы для установки.

```yaml
name: example/hello
version: 1.0.0
license: MIT
description: Пример пакета
targets: [yandexmicro]
files:
  bin/hello: target/$YAM_TRIPLE/release/hello
  share/example.hello/: share/example.hello/
```

Имя имеет вид `<пространство>/<имя>`. Для нативного кода укажите конкретную
платформу; `any` допустим только для независимых от архитектуры файлов.

Соберите программу и создайте архив:

```sh
cargo build --release --target aarch64-unknown-linux-musl
yam --platform yandexmicro pack . --output hello.yampkg
```

`.yampkg` — архив `tar.zst`. YAM сам создаёт `MANIFEST.yaml` и хеши файлов.
Не редактируйте архив вручную.

Проверьте пакет в отдельном каталоге:

```sh
yam --platform yandexmicro --root ./test-root install --yes hello.yampkg
yam --platform yandexmicro --root ./test-root doctor
```

`repair` восстанавливает только ссылки, известные YAM; чужие файлы он не
заменяет. Если по пути ссылки уже лежит обычный файл или чужой симлинк
(не в `pkgs/` этого же пакета), установка и `repair` отказываются его
трогать и называют, что там лежит.

Для пакетов не из `yam/` каждый путь в `share/` должен содержать компонент
`<namespace>.<name>` или начинающийся с `<namespace>.`, например
`share/lua-runtime/plugins/nikita.soloist/main.lua`. Пакеты `yam/` от
правила освобождены; `bin/` правило не касается.

Архив ограничен по размеру и числу записей: не более 64 МиБ в сжатом виде,
256 МиБ в распакованном, 64 МиБ на файл, 10000 записей. Первой записью
архива должен быть `MANIFEST.yaml`. Лимиты проверяются по фактически
прочитанным байтам, а не по заголовку tar.

На время установки, удаления и `repair` YAM держит эксклюзивный `flock`
на `$YAM_RUN/yam.lock`; при занятом локе команда сразу завершается
ошибкой. Авторам хостов, читающим общие каталоги YAM, достаточно взять
разделяемый лок на тот же файл.

## Зависимости

```yaml
depends: [yam/audio-base]
recommends: [yam/webpanel]
conflicts: [old/player]
replaces: [old/player]
```

`replaces` нужен, если пакет намеренно заменяет файлы другого пакета. Без него
коллизия путей приведёт к ошибке. Пример взаимоисключающих провайдеров:

```yaml
provides:  [spotify-connect]
conflicts: [spotify-connect]
replaces:  [spotify-connect]
```

При установке такого пакета, если `spotify-connect` уже предоставляет
другой установленный пакет, тот удаляется целиком в той же транзакции
(требуется `--yes`), а зависимости по `spotify-connect` остаются
удовлетворены новым пакетом. Зависимость по полному имени заменённого
пакета сохранить так не получится — установка будет отклонена.

Entware не включается в пакет:

```yaml
entware:
  packages: [ffmpeg]
```

YAM покажет эти зависимости и запросит подтверждение перед вызовом `opkg`.

Сервис можно описать без shell-команды:

```yaml
services:
  worker:
    exec: [bin/worker, --config, "${YAM_ETC}/worker.toml"]
    restart: on-failure
```

Поддерживаются `never`, `on-failure` и `always`. Не добавляйте в пакет init-
скрипты systemd, OpenRC или BusyBox.

Посмотреть сохранённые декларации можно командой `yam service list`.
