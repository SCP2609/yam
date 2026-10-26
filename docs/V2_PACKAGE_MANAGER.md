# YAM v2 package manager

YAM v2 is an incompatible package manager currently developed on the `v2`
branch. Its package artifacts are `.yampkg`; it does not install legacy `.yap`
archives or read `plugin.toml`.

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

The first v2 increment implements packing, hash verification, safe extraction,
SQLite ownership state, common-tree links, removal, diagnostics, platform
profiles, a legacy converter, and opt-in Entware detection.

Dependency solving, service supervision, hooks, triggers, registry v2,
configuration merging, rollback and system overrides are intentionally not yet
implemented. Manifests declaring unresolved package relations are rejected
rather than being partially installed.

## Legacy conversion

Use an explicit converter for a v1 archive:

```sh
yam --platform yandexmicro convert-yap old-plugin.yap converted-plugin
```

It extracts ordinary payload files and writes a reviewable `yam.yaml`.
Review targets, package identity and runtime-specific metadata before packing.
It does not import v1 permissions, launch configuration, hooks or services.

## Entware

Profiles declare the `opkg` location. `yam entware status` checks availability
and `yam entware list` lists installed external packages. Packages may declare
an `entware.packages` list; installation requires `--yes` before invoking
`opkg`, and fails if Entware is absent.
