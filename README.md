# YAM v2

YAM is a package manager for Yandex Stations. The v2 branch uses signed,
platform-specific `.yampkg` artifacts and keeps package ownership and dependency
state in SQLite.

## Install

The bootstrap URLs remain unchanged:

**Linux / macOS**

```sh
curl -fsSL https://hackalice.me/install.sh | sh
```

**Windows (PowerShell)**

```powershell
irm https://hackalice.me/install.ps1 | iex
```

The bootstrap scripts use ADB to locate a Station. Release builds contain only
the `aarch64-unknown-linux-musl` `yam` CLI for speakers; YAM has no desktop CLI
release artifacts.

## Use on a Station

Choose a platform profile and use the matching persistent root. For Micro:

```sh
yam --platform yandexmicro list
yam --platform yandexmicro registry list
yam --platform yandexmicro search namespace/package
yam --platform yandexmicro install namespace/package
```

For host-side package development, pass an isolated root explicitly:

```sh
yam --platform yandexmicro --root ./yam-root install --yes package.yampkg
```

## Package authors

- Source descriptor: `yam.yaml`
- Package artifact: `.yampkg` (`tar.zst`)
- Platform profiles: [`platforms/`](platforms/)
- Package-author guide: [`docs/PACKAGING.md`](docs/PACKAGING.md)
- Package format and commands: [`docs/V2_PACKAGE_MANAGER.md`](docs/V2_PACKAGE_MANAGER.md)
- Registry format and publishing flow: [`docs/V2_REGISTRY.md`](docs/V2_REGISTRY.md)

A package is named `<namespace>/<name>`. Build one artifact per concrete target
when it contains native code. Artifacts are verified before extraction; the
manager owns the common `bin/` and `share/` links.

## Scope boundary

The `yam` core does not embed a web UI, Lua, Quasar IPC, hardware APIs, or a
legacy plugin permission model. [`packages/yam-lua-runtime/`](packages/yam-lua-runtime/)
is an independent host package that owns the legacy `.yap`/`plugin.toml` format,
Lua/Quasar APIs, web interface, and its APKG converter.
