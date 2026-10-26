# YAM v2

YAM is a package manager for Yandex Stations.

- Package source: `yam.yaml`
- Package artifact: `.yampkg` (`tar.zst`)
- Platform profiles: `platforms/`
- Registry and package workflow: [docs/V2_REGISTRY.md](docs/V2_REGISTRY.md)
- Package format and CLI: [docs/V2_PACKAGE_MANAGER.md](docs/V2_PACKAGE_MANAGER.md)

The project contains no embedded web UI, hardware runtime, Lua runtime, or
legacy `.yap` compatibility layer. Those are independent host packages.
