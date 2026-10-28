# YAM

Менеджер пакетов для Яндекс Станций.
> [!CAUTION]
> yam обновился! теперь yam это пакетный менеджер(который работает непосредственно на самой станции), а lua рантайм будет отдельным пакетом. совместимость со старыми пакетами была потеряна.

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

## Использование

```sh
yam --platform yandexmicro list
yam --platform yandexmicro search namespace/package
yam --platform yandexmicro install namespace/package
yam --platform yandexmicro doctor
yam --platform yandexmicro repair
yam --platform yandexmicro update
```

Для локальной проверки пакета укажите отдельный каталог:

```sh
yam --platform yandexmicro --root ./yam-root install --yes package.yampkg
```

## Документация

- [Упаковка](docs/PACKAGING.md)
- [Реестр](docs/REGISTRY.md)
- [Платформы](docs/PLATFORMS.md)
- [Быстрый старт](docs/QUICKSTART.md)
- [Сборка и публикация](docs/PUBLISHING.md)

Lua, Quasar, веб-интерфейс и старые `.yap`/`plugin.toml` находятся в отдельном
проекте [Luna](https://github.com/nyakokitsu/luna), а не в ядре YAM.
