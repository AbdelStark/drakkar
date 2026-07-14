# Changelog

All notable changes to DRAKKAR are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and DRAKKAR adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(see [docs/spec/09-release-and-versioning.md](docs/spec/09-release-and-versioning.md)).

Every user-visible change lands with its entry under `[Unreleased]` in the same
PR (RV30). Dates are ISO 8601.

## [Unreleased]

### Added

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
