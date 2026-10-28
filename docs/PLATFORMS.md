# Профили платформ

Профиль выбирается через `--platform <id>` или переменную `YAM_PLATFORM`.
Он задаёт ABI пакета и каталоги YAM; он не подтверждает наличие boot hook,
Entware или возможность установки системных файлов.

| ID | Устройство | ABI |
| --- | --- | --- |
| `yandexmicro` | Станция Лайт | aarch64 |
| `plum` | Станция Лайт 2 | aarch64 |
| `yandexmini` | Станция Мини (1-е поколение) | aarch64 |
| `yandexmini_2` | Станция Мини 2 | aarch64 |
| `bergamot` | Станция Мини 3 | aarch64 |
| `pickle` | Станция Мини 3 Про | aarch64 |
| `cucumber` | Станция Миди | aarch64 |
| `yandexmidi` | Станция 2 | aarch64 |
| `mango` | Станция Стрит | armv7 |
| `yandexstation_2` | Станция Макс (включая Zigbee) | aarch64 |
| `yandexstation` | Станция первого поколения | aarch64 |
| `saturn` | Умный хаб Яндекса | aarch64 |
| `jbl_link_music` | JBL Link Music | armv7 |
| `orion` | Станция 3 | aarch64 |

`yandexplum` сохранён как совместимый ID для ранних конфигураций; для новых
пакетов и установок используйте `plum`.

Профили `mango` и `jbl_link_music` используют 32-битный ARM ABI. Installer
выбирает для них binary `armv7-unknown-linux-musleabihf`; остальные профили
получают `aarch64-unknown-linux-musl`.

Автоматический boot hook сейчас подтверждён только для `yandexmicro`. На
остальных платформах установите YAM с нужным `YAM_PLATFORM`, а обновление при
необходимости запускайте вручную: `yam update`.
