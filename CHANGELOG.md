# Changelog

All notable changes to DRAKKAR are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and DRAKKAR adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(see [docs/spec/09-release-and-versioning.md](docs/spec/09-release-and-versioning.md)).

Every user-visible change lands with its entry under `[Unreleased]` in the same
PR (RV30). Dates are ISO 8601.

## [Unreleased]

### Added

- `drakkar config get|set|path` commands: `get` reports a key's effective value
  and the precedence layer it came from (flag/env/file/default) with a
  `drakkar.config/1` `--json` object; `set` validates and writes atomically at
  `0600`, rejecting unknown keys with exit 2; `path` prints the file location
  (`server.api_key` is redacted in every `get`) (#110).
- `drakkar.config/1` configuration library in `drakkar-core`: the `config.toml`
  schema, the four-level precedence resolver (flags > `DRAKKAR_*` env > file >
  defaults), the mechanical env mapping (`server.port` ⇔ `DRAKKAR_SERVER_PORT`),
  range/type validation returning `config.invalid_key`/`config.invalid_value`,
  suffixed-duration parsing, and an atomic (temp + rename) `set` writer that
  re-validates before touching the file (#126).

### Changed

### Deprecated

### Removed

### Fixed

### Security

- `config.toml` is written mode `0600` (owner-only) on every mutation, via an
  atomic temp-file + rename, since it may hold `server.api_key` (SEC20). The
  server API key resolves with the precedence `--api-key` > `DRAKKAR_API_KEY` >
  `server.api_key` and is held as a redacting `Secret<String>` (SEC28) (#52).
