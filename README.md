# YAM

Менеджер пакетов для Яндекс Станций.
> [!CAUTION]
> yam обновился! теперь yam это пакетный менеджер(который работает непосредственно на самой станции), а lua рантайм будет отдельным пакетом. совместимость со старыми пакетами была потеряна.

## Установка

**Linux / macOS**

```sh
curl -fsSL https://hackalice.me/install.sh | sh
```

**Windows (PowerShell)**

```powershell
irm https://hackalice.me/install.ps1 | iex
```

Установщик подключается к колонке через ADB. Используйте yam через adb shell

## Использование

```sh
yam list
yam search namespace/package
yam install namespace/package
yam doctor
yam repair
```

Для локальной проверки пакета укажите отдельный каталог:

```sh
yam --platform yandexmicro --root ./yam-root install --yes package.yampkg
```

## Документация

- [Упаковка](docs/PACKAGING.md)
- [Реестр](docs/REGISTRY.md)

Lua, Quasar, веб-интерфейс и старые `.yap`/`plugin.toml` находятся в отдельном
пакете [`packages/yam-lua-runtime/`](packages/yam-lua-runtime/). В планах перевести runtime в отдельный репозиторий.
