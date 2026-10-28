# YAM v2 agent guide

YAM v2 is a package manager for Yandex Stations. Its core responsibility is
to validate, resolve, install, link, update, remove and supervise packages.
It deliberately does **not** know about Lua, Quasar IPC, a web panel, or a
specific package host. Those are independent packages and hosts.

## Scope and migration

- Work for v2 happens on the `v2` branch until it is explicitly merged.
- v2 introduces the incompatible `.yampkg` format. Do not add new features to
  the legacy `.yap`/`plugin.toml` path except a converter and critical fixes.
- Keep the current v1 runtime usable while v2 is under construction. Do not
  silently alter installed v1 packages or delete their data.
- The v1-to-v2 migration entry point is an explicit command, planned as
  `yam convert-yap <archive.yap>`. It produces a reviewable v2 source tree;
  it must not install, execute hooks, or infer privileged behaviour.
- Never create a release or move a version tag unless the user explicitly
  requests it.

## Repository map

Current layout is transitional. Keep new v2 code separated from legacy code
as it is introduced.

- `src/bin/yam.rs` — user-facing CLI.
- `src/main.rs` — legacy `yamd`, v1 API, package handling and Lua bridge.
- `web/index.html` — legacy embedded daemon UI.
- `platforms/` — v2 platform profiles; add one YAML file per supported target.
- `docs/` — public format, CLI, platform and runtime documentation.
- `examples/` — example package source trees. New examples use `yam.yaml`.
- `tools/` — offline conversion, validation and release tooling.
- `install.sh` / `install.ps1` — deployment scripts; preserve the platform
  boot hook and always restore a remounted root filesystem to read-only.

Do not place package payloads, target binaries, registry downloads, device
dumps, logs, generated indexes, secrets, or credentials in the repository.

## Required checks

After changing Rust or shell code, run:

```sh
cargo fmt --check
cargo check
cargo test --locked
sh -n install.sh
```

After changing PowerShell, run a parser check when PowerShell is available:

```powershell
[void][scriptblock]::Create((Get-Content -Raw ./install.ps1))
```

For code shipped to Station Lite, Micro, Midi or Mini 2, build the target:

```sh
docker run --rm -v "$PWD":/home/rust/src -w /home/rust/src \
  messense/rust-musl-cross:aarch64-musl cargo build --release
```

Run the narrowest relevant package-format, resolver and transaction tests too.
Before committing, require a clean `git diff --check`. Never commit `target/`.

## Core package-manager rules

### v2 source and artifact format

- Source metadata is `yam.yaml`; package artifacts are `.yampkg` files.
- A `.yampkg` is a `tar.zst` archive containing root `MANIFEST.yaml`, `files/`
  and optional `SOURCE`. Do not revive ZIP, `.yap`, or `plugin.toml` for v2.
- `yam-publish pack --platform <platform>` generates the artifact manifest from source
  metadata. It expands only `$YAM_TARGET` and `$YAM_TRIPLE` in `files` paths.
- A generated manifest contains exactly one target, hashes every payload file,
  and records provenance. It does not retain the source `files` mapping.
- Package names are `<namespace>/<name>`. Validate both components strictly;
  do not reuse the legacy unrestricted package ID parser.
- Versions are SemVer. Pre-releases are never selected implicitly.
- Target `any` is only for target-independent payloads. Native code must name
  one or more concrete platform profiles.

### Archive safety

- Validate archives before extraction and validate again while extracting.
- Permit only regular files and directories in `files/`; reject symlinks,
  hardlinks, devices, FIFOs, absolute paths, `..`, duplicate normalized paths,
  and paths that escape the destination after canonicalization.
- Enforce compressed and unpacked size limits before allocating unbounded disk
  space. Extraction happens in a fresh staging directory under YAM's root.
- Verify registry hash/signature before extraction and verify `files_sha256`
  before a package becomes active.
- Never execute a hook, service, binary, or parser supplied by an unverified
  archive.

### Filesystem ownership and layout

- Logical package data lives below the profile's `$YAM_ROOT`, not hard-coded
  `/data/quazar`, `/opt`, or `/system` paths.
- Use the v2 layout: `pkgs/`, `bin/`, `share/`, `etc/`, `var/`, `db/` and a
  profile-defined runtime directory.
- Package payloads are immutable version directories:
  `pkgs/<namespace>/<name>/<version>/`. `current` is the only mutable version
  selector.
- Only YAM creates links in the common tree. Archive-provided links are banned.
- `bin/` and `share/` links are file-by-file; common-tree directories are real
  directories. Do not link an entire directory into another package's tree.
- A common-tree path has exactly one recorded owner. Refuse collisions unless
  the transaction explicitly resolves them through `replaces`.
- `lib/` remains private by default. Exporting a library is an explicit,
  validated feature, never an incidental side effect of installation.
- For non-`yam/` namespaces, require a namespace-prefixed component in every
  linked `share/` path, such as `nikita.soloist`.

### SQLite state

- SQLite is the source of truth for installed versions, current selections,
  payload hashes, links, path ownership, dependencies, config hashes,
  services, transaction state and firmware/profile observations.
- Use database transactions for all metadata mutations. Keep filesystem changes
  in an explicit, reversible transaction plan; do not rely on a best-effort
  sequence of ad-hoc writes.
- Hold an exclusive `flock` on `$YAM_RUN/yam.lock` for the complete package
  transaction. Hosts may use a shared lock while scanning shared directories.
- Record enough information to implement `yam doctor`, `yam repair`,
  `yam which`, `yam files`, `yam rollback` and safe removal without guessing.
- Do not put mutable runtime state, logs, cache downloads or lock files into
  SQLite when a normal file is a better fit.

### Dependency resolution

- Resolve before downloading or changing disk state.
- Support `depends`, `recommends`, `enhances`, `provides`, `conflicts` and
  `replaces` exactly as documented. `depends` is mandatory; recommends may be
  disabled with `--no-recommends`.
- Parse and test all version constraints: `=`, `>=`, `<`, `^`, `~` and compound
  constraints. Do not compare versions lexicographically.
- A short virtual dependency resolves to an already installed provider first;
  otherwise automatically select only an official `yam/` provider. Require an
  explicit user selection for another namespace.
- Detect dependency cycles, incompatible constraints, multiple providers and
  path ownership conflicts before modifying the filesystem.
- `yam autoremove` removes only orphaned packages installed solely as
  dependencies. Never auto-remove an explicitly requested package.

### Install, upgrade and removal transactions

1. Resolve dependencies, targets, conflicts, replaces and Entware requests.
2. Display package metadata, declared permissions and planned changes.
3. Require confirmation unless `--yes` was passed.
4. Download, authenticate and validate every artifact.
5. Take the exclusive lock and build the complete link/ownership plan.
6. Run `preinstall`, stage payloads, switch `current`, and update links.
7. Run `postinstall` or `upgrade`, then commit metadata.
8. Release the lock, execute affected triggers once, then restart services.

- On failure before commit, restore the previous `current` link, common-tree
  links and database state. Preserve useful diagnostics.
- Hooks run as root with a timeout and the documented `YAM_*` environment.
  They must be idempotent. Hook failure rolls back the transaction except for
  post-transaction trigger failures, which are warnings.
- Stop dependent services before removing their provider. Present the complete
  reverse dependency plan and require confirmation.
- Preserve user-modified config. On upgrade, place a changed new default beside
  it as `.new`; compare against the recorded previous default hash.

### Services, triggers and platform profiles

- Services use the neutral manifest schema: `exec`, `restart`, `after`, `env`
  and `oneshot`. Do not accept systemd, OpenRC or BusyBox init scripts inside
  packages.
- A platform profile selects the init adapter and durable boot hook. Never
  hard-code a model's sysfs path, ABI, writable partition, init script path or
  firmware-version path in generic package-manager code.
- The default supervisor owns service processes, logs and restart policy.
  Ensure it kills an entire process group, prevents duplicate starts, and
  avoids restart loops.
- All service and hook environments include `YAM_ROOT`, `YAM_PKG`, `YAM_ETC`,
  `YAM_VAR`, `YAM_RUN` and `YAM_PLATFORM`; add `/opt/bin:/opt/sbin` only when
  Entware is active.
- Triggers are glob/path based, run once per committed transaction, receive
  changed paths on stdin, and run only after the package lock is released.
- OTA handling compares the profile-defined firmware version at boot. `repair`
  restores only YAM-owned state and never overwrites arbitrary system files.
- `system_overrides`, bind mounts, `LD_PRELOAD` and overlayfs are out of scope
  for v2's first implementation. Reject them clearly; do not partially apply
  them.

### Entware

- Entware is opt-in. A package may declare `entware.packages`, but installation
  must show those external packages and ask for confirmation unless `--yes` is
  supplied.
- Detect Entware through the platform adapter, normally `/opt/bin/opkg`; never
  assume `/opt` exists on every target.
- Do not model Entware-owned files as YAM package files and never let a YAM
  transaction remove an Entware package implicitly.
- Provide transparent diagnostics through `yam entware ...` and `yam doctor`.
  Missing Entware dependencies make the dependent YAM package unhealthy, not
  silently successful.

## Lua runtime rules

Luna is a **host package**, published as `yam/luna`; it is not a special case
in the YAM package manager.

- Luna discovers plugin files under `$YAM_ROOT/share/luna/plugins/`. YAM only
  installs and links files;
  it does not parse, start or authorize Lua plugins.
- Lua plugin packages are ordinary `.yampkg` artifacts. Keep runtime-specific
  manifest, permission and API metadata inside the plugin's own files.
- Do not add Lua permissions, Quasar IPC permissions, `alice.*` APIs or LED
  assumptions to `MANIFEST.yaml` or the generic resolver.
- The runtime owns plugin isolation, cancellation, compatibility checks and
  runtime API authorization. It should use a shared lock while scanning plugin
  directories and react to its own reload trigger.
- Hardware access is deny-by-default. A new Lua API requires a runtime
  permission, validation of its manifest declaration, documentation and a
  hardware capability check.
- Do not expose raw Quasar or system control merely because a package declares
  a generic v2 permission. Keep powerful operations explicitly scoped in the
  runtime's own format.
- Long-running Lua code must be cancellable. Do not block the runtime executor
  indefinitely; provide cancellation-aware sleep, I/O and event waiting.
- Runtime code must discover hardware via platform capabilities or validated
  adapters. Never infer unknown-device paths from a model name.
- A Luna package can declare package-level dependencies like any other host.
  Lua plugins should depend on the Luna package/version they require.

## General Rust and CLI rules

- Prefer small modules with explicit ownership boundaries over adding more
  unrelated logic to `src/main.rs`. New v2 package-manager code must not grow
  the legacy daemon monolith.
- Return actionable errors: package name/version, target, failing path and the
  failed validation rule. Do not hide a partial transaction behind `Ok(())`.
- Paths received from archives, manifests, CLI arguments and registries are
  untrusted. Validate them before creating paths or invoking a process.
- Never build shell command strings from package-controlled values. Use process
  argument arrays and a deliberately constructed environment.
- Treat registry metadata, package archives, hooks and service definitions as
  untrusted until verified. Never log tokens, authorization headers, private
  config or raw audio/speech data.
- Preserve user work: do not reset, checkout, delete or overwrite unrelated
  changes. Stage only the files required by the current task.
- Update the matching public documentation and example package whenever a
  manifest, CLI or package lifecycle contract changes.
