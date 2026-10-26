# YAM v2 registry

YAM v2 registry combines APT-style installation with PyPI-style publication.
It is HTTP/CDN friendly: a static implementation needs only JSON files and
immutable `.yampkg` artifacts.

## Namespaces and artifacts

Package names are `<namespace>/<name>`. `yam/*` is the official namespace;
other namespaces belong to an author or organisation identity. An author keeps
`yam.yaml`, source and assets in their own repository and produces one
`.yampkg` per target with `yam pack`.

Published artifacts are immutable. A registry must reject a second upload for
the same `name`, version and target, even if its bytes differ.

## Static endpoints

```text
GET /v2/index.json
GET /v2/packages/<namespace>/<name>/index.json
GET /v2/packages/<namespace>/<name>/<version>/<target>.yampkg
GET /v2/packages/<namespace>/<name>/<version>/<target>.yampkg.sha256
GET /v2/namespaces/<namespace>.json
```

`index.json` is a lightweight catalogue for search. The package index is the
source of version and artifact selection:

```json
{
  "format": 2,
  "name": "nikita/soloist",
  "versions": {
    "1.4.0": {
      "published_at": "2026-10-26T20:00:00Z",
      "artifacts": {
        "yandexmicro": {
          "arch": "aarch64",
          "url": "packages/nikita/soloist/1.4.0/yandexmicro.yampkg",
          "sha256": "<64 lowercase hex characters>",
          "size": 1843200
        }
      },
      "source": {
        "url": "https://codeberg.org/nikita/soloist/archive/<commit>.tar.gz",
        "sha256": "<sha256>",
        "revision": "<commit>"
      },
      "provenance": { "level": "author", "builder": "yam pack 2.0.0" }
    }
  }
}
```

YAM downloads package metadata first, selects the latest stable version unless
the user asks for `name@version`, selects the artifact matching the platform
profile and architecture, then verifies its SHA-256 before opening the archive.

## CLI

```sh
yam registry add official https://registry.example/v2 --key ed25519:...
yam registry list
yam registry remove official
yam search spotify
yam info nikita/soloist
yam install nikita/soloist
yam install nikita/soloist@1.4.0
```

Registry configuration is local under YAM's state root. Adding a third-party
registry is explicit; YAM never discovers or trusts one automatically.

## Trust model

The design has three layers: HTTPS transport, a signature on canonical registry
metadata by a configured registry key, and author signatures bound to namespace
ownership. SHA-256 is mandatory for every artifact but is not a substitute for
metadata signatures.

The initial client implementation validates HTTPS and artifact SHA-256. It
stores the configured `ed25519:` key but deliberately does **not** claim to
verify signatures until the registry defines the signature envelope and
canonical JSON representation. Registries requiring signature verification
must not be used with that bootstrap client.

## Publication service

For an MVP, a Git repository/PR workflow can validate artifacts and generate
the static index. A full PyPI-like service later adds OAuth/API tokens,
namespace ownership, draft/quarantine review, upload endpoints, PostgreSQL
metadata and object storage/CDN delivery. Entware is never published in this
registry; packages only declare their local `opkg` requirements.
