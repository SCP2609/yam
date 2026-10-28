# YAM

Менеджер пакетов для Яндекс Станций.

> [!CAUTION]
> yam обновился: теперь это пакетный менеджер, который работает прямо на
> станции, а Lua-рантайм вынесен в отдельный пакет. Совместимость со старыми
> пакетами потеряна.

## Установка

**Linux / macOS**

```sh
curl -fsSL https://hackalice.me/install.sh | sh
```

**Альтернативная ссылка для Linux / macOS** (если короткий URL ещё обновляется):

```sh
curl -fsSL https://raw.githubusercontent.com/nyakokitsu/yam/main/install.sh | sh
```

**Windows (PowerShell)**

```powershell
irm https://hackalice.me/install.ps1 | iex
```

**Альтернативная ссылка для Windows PowerShell** (если короткий URL ещё обновляется):

```powershell
irm https://raw.githubusercontent.com/nyakokitsu/yam/main/install.ps1 | iex
```

Установщик подключается к колонке через ADB. Публикуются Linux ARM binaries
для `aarch64-unknown-linux-musl` и `armv7-unknown-linux-musleabihf`;
десктопных сборок нет. Платформа определяется по модели и Device Tree. При
необходимости её можно задать явно:

```sh
curl -fsSL https://hackalice.me/install.sh | YAM_PLATFORM=yandexmini_2 sh
```

Параметры установщика:

| `install.sh` | `install.ps1` | Назначение |
| --- | --- | --- |
| аргумент `[версия]` или `YAM_VERSION` | `-Version` или `YAM_VERSION` | Релиз `yam-v<версия>`. По умолчанию `install.sh` берёт первый тег `yam-v*` из GitHub Releases без фильтра prerelease, `install.ps1` — первый не-draft и не-prerelease |
| `YAM_PLATFORM` | `-Platform` или `YAM_PLATFORM` | Профиль платформы вместо автоопределения |
| `YAM_ROOT` | `YAM_ROOT` | Каталог YAM на колонке; по умолчанию `/data/yam` |
| `--remove-old` | `-RemoveOld` | Попытаться остановить legacy `yamd` и **полностью удалить** `/data/quazar/yam` |
| — | `-StationIp` | IP колонки для `adb connect` |

Если ADB-устройств несколько, установщик спрашивает номер; если их нет —
IP-адрес и подключается к `<IP>:5555`. Аргументы `install.sh` через пайп:

```sh
curl -fsSL https://hackalice.me/install.sh | sh -s -- 0.2.1
```

> [!WARNING]
> `--remove-old` выполняет `rm -rf /data/quazar/yam`: удаляются старый `yamd`,
> все пакеты v1 и их данные. Перед запуском сделайте резервную копию.

## Использование

```sh
yam --platform yandexmicro list
yam --platform yandexmicro search namespace/package
yam --platform yandexmicro install namespace/package
yam --platform yandexmicro doctor
yam --platform yandexmicro repair
yam --platform yandexmicro update
```

Реестр `official` по умолчанию не настроен: перед `search` и установкой по
имени добавьте его командой `yam registry add` (см. [CLI](docs/CLI.md)). На
Micro обёртка `/usr/bin/yam` уже задаёт платформу и каталог профилей.

Для локальной проверки пакета укажите отдельный каталог:

```sh
yam --platform yandexmicro --root ./yam-root install --yes package.yampkg
```

## Документация

- [Каталог документации](docs/README.md)
- [Командная строка](docs/CLI.md)
- [Упаковка](docs/PACKAGING.md)
- [Реестр](docs/REGISTRY.md)
- [Платформы](docs/PLATFORMS.md)
- [Быстрый старт](docs/QUICKSTART.md)
- [Сборка и публикация](docs/PUBLISHING.md)

Lua, Quasar, веб-интерфейс и старые `.yap`/`plugin.toml` находятся в отдельном
проекте [Luna](https://github.com/nyakokitsu/luna), а не в ядре YAM.

## Обновление

На Micro установщик создаёт `/usr/bin/yam` — ссылку на обёртку
`/data/yam/bin/yam-run` — и загрузочный скрипт `/etc/init.d/S99yam`, который
при старте выполняет `yam update` (ошибка обновления загрузку не прерывает).
На остальных платформах автообновления при загрузке нет: запускайте
`yam update` вручную.

Обновление принимается только при корректной Ed25519-подписи манифеста из
GitHub Releases; открытый ключ зашивается в бинарник при сборке. Для
публикации нужны repository variable `YAM_UPDATE_PUBLIC_KEY` и secret
`YAM_UPDATE_SIGNING_KEY`.
