# The Soma package registry

A **sparse HTTP index** (the model Cargo moved to), served as static JSON.

- `GET /repo/<name>.json` → a package's versions, each pointing at a git
  source (+ optional `subdir` for monorepos).
- `GET /repo/index.json` → the catalog of all packages.

`soma add <name>` records `name = "*"` in soma.toml; `soma install`
fetches `<registry>/<name>.json`, resolves the version, and clones the
source. The registry base defaults to `https://soma-lang.dev/repo` and is
overridable with `SOMA_REGISTRY`.

To publish: add a `<name>.json` here pointing at your package's git repo
(or a subdir of one), and an entry in `index.json`.
