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

Установщик подключается к колонке через ADB. Публикуется только `yam` для
`aarch64-unknown-linux-musl`; десктопных сборок нет.

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

Lua, Quasar, веб-интерфейс и старые `.yap`/`plugin.toml` находятся в отдельном
пакете [`packages/yam-lua-runtime/`](packages/yam-lua-runtime/), а не в ядре YAM.

## Обновление

Установщик добавляет `/usr/bin/yam` на Micro и проверяет обновление при загрузке.
Обновление принимается только при корректной Ed25519-подписи манифеста из
GitHub Releases. Для публикации нужны repository variable `YAM_UPDATE_PUBLIC_KEY`
и secret `YAM_UPDATE_SIGNING_KEY`.

## Обновление

Установщик добавляет `/usr/bin/yam` на Micro и проверяет обновление при загрузке.
Обновление принимается только при корректной Ed25519-подписи манифеста из
GitHub Releases. Для публикации нужны repository variable `YAM_UPDATE_PUBLIC_KEY`
и secret `YAM_UPDATE_SIGNING_KEY`.
