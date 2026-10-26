# YAM v2 package manager

YAM v2 is an incompatible package manager currently developed on the `v2`
branch. Its package artifacts are `.yampkg`; the core does not install legacy
`.yap` archives or read `plugin.toml`. Those formats belong exclusively to the
separate `yam/lua-runtime` host package.

## Quick start

Create a package source tree:

```yaml
# yam.yaml
name: example/hello
version: 1.0.0
license: MIT
description: Minimal YAM v2 package
targets: [any]
files:
  bin/hello: bin/hello
```

Build and install it for a profile:

```sh
yam --platform yandexmicro pack . --output hello-1.0.0-any.yampkg
yam --platform yandexmicro install --yes hello-1.0.0-any.yampkg
```

For development on a host instead of a Station, set an isolated root:

```sh
yam --platform yandexmicro --root ./yam-root install --yes hello-1.0.0-any.yampkg
```

Use `list`, `files <name>`, `which <path>`, `doctor`, and `remove <name>` to
inspect package state.

For an author-oriented source descriptor, relationship fields, target builds,
services and publication checklist, see [PACKAGING.md](PACKAGING.md).

## Artifact rules

- An artifact is a zstd-compressed tar file with extension `.yampkg`.
- It contains generated `MANIFEST.yaml` and regular payload files below
  `files/`.
- Archives may not contain symlinks, hardlinks, devices, FIFOs, absolute paths
  or parent traversal.
- `bin/` and `share/` payload files are linked into YAM's common tree.
  Other files remain private to the package payload.
- Native payloads must name a concrete target. `any` is for target-independent
  packages only.

## Current implementation boundary

The current core implements packing, safe archive validation, SHA-256 payload
verification, SQLite ownership state, common-tree links, installed dependency
and conflict validation, reverse-dependency removal protection, diagnostics,
platform profiles, opt-in Entware detection, and signed static registry
metadata. Registry installation resolves concrete dependency graphs, verifies
every download before the first filesystem change, then installs in
dependency-first order.

Services are accepted as neutral manifest metadata (`exec` argument array,
restart policy, ordering and environment). A platform supervisor and boot
adapter, hooks/triggers, config merging, full upgrade rollback, autoremove and
system overrides remain future work. `system_overrides` is deliberately not
accepted.

## Legacy plugin formats

There is no `convert-yap` command in the v2 core. The legacy `.yap` archive,
`plugin.toml`, Lua permission model, Quasar API and APKG converter are owned by
[`packages/yam-lua-runtime/`](../packages/yam-lua-runtime/). They are not v2
package metadata and must not be added to `yam.yaml` or `MANIFEST.yaml`.

## Entware

Profiles declare the `opkg` location. `yam entware status` checks availability
and `yam entware list` lists installed external packages. Packages may declare
an `entware.packages` list; installation requires `--yes` before invoking
`opkg`, and fails if Entware is absent.
