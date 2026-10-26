# Packaging for YAM v2

This guide describes how to turn a package source tree into a safe,
platform-specific `.yampkg` artifact. It applies to ordinary YAM v2 packages;
it does not describe legacy Lua plugins. The Lua host owns `.yap` and
`plugin.toml` in [`packages/yam-lua-runtime/`](../packages/yam-lua-runtime/).

## 1. Create a source tree

A source tree contains `yam.yaml` and the files to ship:

```text
hello/
├── yam.yaml
├── bin/
│   └── hello
└── share/
    └── example.hello/
        └── README.txt
```

Use a namespace-qualified name and a SemVer version:

```yaml
# yam.yaml
name: example/hello
version: 1.0.0
license: MIT
description: Minimal YAM v2 package
homepage: https://example.invalid/hello
targets: [yandexmicro]
files:
  bin/hello: bin/hello
  share/example.hello/: share/example.hello/
```

`files` maps a destination inside the package payload to a source file or
directory. Directory mappings end in `/`. Payload paths must be relative and
may not use links, `..`, devices or other special files.

`yam pack` expands `$YAM_TARGET` and `$YAM_TRIPLE` in **source** paths only.
This is useful for native output:

```yaml
targets: [yandexmicro]
files:
  bin/hello: target/$YAM_TRIPLE/release/hello
```

A package with only portable data may use `targets: [any]`. Do not use `any` for
native executables, shared objects, or model-specific assets.

## 2. Declare package relationships

All relationship fields are optional lists. `depends` is mandatory at install
time; `recommends` and `enhances` are advisory metadata.

```yaml
depends:
  - yam/audio-base >=1.0
recommends:
  - yam/webpanel
provides:
  - spotify-connect
conflicts:
  - oldnamespace/old-player
replaces:
  - oldnamespace/old-player
```

Use concrete `namespace/name` dependencies whenever possible. Virtual
requirements (`provides`) need a registry provider-selection policy; the
resolver rejects ambiguous virtual selections rather than guessing.

`replaces` is required when an upgrade intentionally takes ownership of an
existing common-tree path. YAM otherwise rejects path collisions.

## 3. Optional Entware and service metadata

Entware remains opt-in and is never bundled into a `.yampkg`:

```yaml
entware:
  packages: [ffmpeg]
```

The install plan exposes those external packages and requires confirmation
before invoking `opkg`.

Packages may also declare neutral service metadata. `exec` is an argument array,
not a shell command:

```yaml
services:
  worker:
    exec: [bin/worker, --config, "${YAM_ETC}/worker.toml"]
    restart: on-failure
    after: [network]
    env:
      RUST_LOG: info
```

Supported restart values are `never`, `on-failure`, and `always`. Service
metadata is package data; platform init integration and supervision are not a
license to include systemd/OpenRC/BusyBox init scripts in a package.

## 4. Build and pack

Build native output for the selected platform, then pack with the same profile:

```sh
cargo build --release --target aarch64-unknown-linux-musl
yam --platform yandexmicro pack . --output example-hello-1.0.0.yampkg
```

The packer generates `MANIFEST.yaml`, computes SHA-256 for every payload file,
and writes a zstd-compressed tar archive. Do not hand-edit a packed manifest.

Inspect an artifact by installing into a disposable root:

```sh
yam --platform yandexmicro --root ./test-root install --yes example-hello-1.0.0.yampkg
yam --platform yandexmicro --root ./test-root list
yam --platform yandexmicro --root ./test-root files example/hello
yam --platform yandexmicro --root ./test-root doctor
```

## 5. Publish to a registry

Artifacts are immutable by `(name, version, target)`. Build and publish a new
SemVer version for every changed artifact. A registry must publish signed
catalogue and package-index JSON plus exact artifact size and SHA-256.

Before installation, YAM verifies signed metadata, downloads every dependency
artifact, verifies its size and SHA-256, then installs in dependency-first
order. See [`V2_REGISTRY.md`](V2_REGISTRY.md) for endpoint, signature, and
publication requirements.

## Package checklist

- [ ] Name is `<namespace>/<name>` and version is SemVer.
- [ ] Native payload targets one concrete platform profile.
- [ ] Every `files` source exists and contains only regular files/directories.
- [ ] `share/` paths for third-party namespaces are namespace-prefixed.
- [ ] Dependencies and conflicts are declared explicitly.
- [ ] Entware requirements are declarations only; no `/opt` payload files.
- [ ] Service commands use an argument array and no shell syntax.
- [ ] The artifact installs and `yam doctor` succeeds in an isolated root.
- [ ] The artifact SHA-256 and size are recorded by the registry.
