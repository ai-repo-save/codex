# App-Server and Protocol APIs

## Ownership

- App-server protocol types live in `codex-rs/app-server-protocol/src/protocol`.
- App-server implementation lives in `codex-rs/app-server`.
- Shared domain/wire types used across core and UI live in `codex-rs/protocol`.

## v2 API rules

- Active app-server API development should happen in v2.
- v2 request payloads use `*Params`; responses use `*Response`; notifications use `*Notification`.
- RPC methods use `<resource>/<method>` with singular resource names.
- v2 wire fields are camelCase by default, except config RPC payloads that mirror `config.toml` keys.
- New list methods should use cursor pagination with `cursor`, `limit`, `data`, and `next_cursor`.
- Experimental APIs must use the repository's experimental API annotations and schema generation flow.

## Schema and docs

- API shape changes require remote `just.py write-app-server-schema` only. Never use
  `--experimental` in this fork; it pollutes the working tree with unrelated fixtures.
- Validate with remote `just.py test -p codex-app-server-protocol`.
- Update app-server docs/examples when behavior changes.
- Keep Rust and generated TypeScript names aligned when using explicit serde/ts renames.

## Compatibility

- Treat app-server APIs, CLI parameters, config loading, and resumed sessions as external integration surfaces.
- Search for breaking changes before changing public payloads, method names, config keys, or resume semantics.
