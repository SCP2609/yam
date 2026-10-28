# Быстрый старт

YAM ставит пакеты прямо на Яндекс Станцию. Компьютер нужен только для первой
установки через ADB; затем команды выполняются в shell колонки.

## 1. Подготовьте компьютер

1. Установите [Android Platform Tools](https://developer.android.com/tools/releases/platform-tools),
   чтобы команда `adb` была доступна в терминале.
2. Включите ADB на колонке и подключите её к той же сети, что и компьютер.
3. Откройте терминал на компьютере и выполните установщик:

```sh
curl -fsSL https://hackalice.me/install.sh | sh
```

Installer попросит IP-адрес, если не видит одну подключённую колонку, и сам
определит её платформу. Он не удаляет существующие пакеты YAM.

В Windows PowerShell:

```powershell
irm https://hackalice.me/install.ps1 | iex
```

После установки откройте ADB shell:

```sh
adb shell
yam --help
```

Если auto-detection не распознал устройство, повторите установку с явным
профилем, например `yandexmini_2`:

```sh
curl -fsSL https://hackalice.me/install.sh | YAM_PLATFORM=yandexmini_2 sh
```

Список profile ID: [PLATFORMS.md](PLATFORMS.md).

## 2. Подключите каталог пакетов

Каталог содержит список пакетов и его публичный ключ. Ключ не является
паролем: он нужен YAM, чтобы проверить подпись индексов.

```sh
yam registry add official https://repo.yse.wtf/v2 \
  --key ed25519:5Rz8F47uftWVU5kLgWYhprkEZ4rpP2kecUge/a7KQas=
yam registry list
```

## 3. Найдите и установите пакет

```sh
yam search luna
yam info yam/luna
yam plan yam/luna
yam install yam/luna
```

- `search` ищет по каталогу;
- `info` показывает опубликенные версии и metadata;
- `plan` показывает, что будет скачано, не меняя систему;
- `install` загружает пакет, проверяет подпись индекса, размер и SHA-256,
  затем устанавливает зависимости раньше самого пакета.

Используйте полное имя `namespace/name`. Короткое имя допустимо только когда
YAM может однозначно выбрать официальный пакет `yam/*`.

## 4. Полезные команды

```sh
yam list                    # установленные пакеты
yam files yam/luna          # файлы, принадлежащие пакету
yam which bin/luna          # кто владеет общим путём
yam doctor                  # проверить ссылки и payload
yam repair                  # восстановить только YAM-owned ссылки
yam remove yam/luna         # удалить пакет
yam update                  # обновить сам YAM из подписанного release
```

`repair` не заменяет обычные файлы или чужие симлинки. Если пакет требует
подтверждение в скрипте, добавьте `--yes` только после проверки `yam plan`.

## Если что-то пошло не так

1. Выполните `yam doctor`.
2. Проверьте сеть и снова запустите `yam plan <пакет>`.
3. При ошибке установки сохраните весь вывод команды: в нём есть имя пакета,
   URL или путь, который нужен для диагностики.
4. Не удаляйте вручную `$YAM_ROOT/pkgs` или `db`: используйте `yam remove`,
   `yam repair` и `yam doctor`.
