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
заменяет.

## Зависимости

```yaml
depends: [yam/audio-base]
recommends: [yam/webpanel]
conflicts: [old/player]
replaces: [old/player]
```

`replaces` нужен, если пакет намеренно заменяет файлы другого пакета. Без него
коллизия путей приведёт к ошибке.

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
