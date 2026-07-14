# Contributing to DRAKKAR

DRAKKAR is developed spec-first. The specification corpus ([SPEC.md](SPEC.md),
`docs/spec/`, `docs/rfcs/`) is the single source of truth: every change either implements
a part of it or changes it first.

## Ground rules

- **Every PR traces to an issue, and every issue traces to a spec section or RFC.** If the
  work you want to do has no spec basis, open a spec/RFC PR first (see the change process
  in [SPEC.md](SPEC.md#change-process)).
- **The invariants in [docs/spec/01-architecture.md](docs/spec/01-architecture.md) are
  review criteria.** A PR that violates I1-I5 (engine-thread GPU ownership, the memory
  contract, single source of memory math, JSON representation for every surface, the
  backend seam) is rejected regardless of its other merits.
- **Performance claims require harness numbers.** Optimization PRs cite a before/after run
  of the RFC-0009 harness on named hardware. No single-run numbers.
- **Errors follow the taxonomy.** New failure paths register a stable error code per
  [docs/spec/04-error-model.md](docs/spec/04-error-model.md) in the same PR. Adding a code
  is a two-file edit in one PR: the §4 registry table and the `ErrorCode` enum in
  `drakkar-core`. CI enforces the correspondence with an exhaustive match (a new unmapped
  variant fails to compile) plus a committed golden tuple snapshot
  (`crates/drakkar-core/tests/fixtures/error_registry_snapshot.txt`); regenerate it with
  `UPDATE_SNAPSHOT=1 cargo test -p drakkar-core --test error_registry_snapshot`. Exit codes
  and HTTP statuses are defined only in `drakkar-core::error::mapping`; a status literal
  anywhere else fails the single-mapping-site test.

## Development setup

- Apple Silicon Mac, macOS 15+ (macOS 26.2+ to exercise the Neural Accelerator paths).
- Rust toolchain pinned by `rust-toolchain.toml` (installed automatically by rustup).
- Xcode command line tools and CMake (the MLX shim builds via `build.rs`).
- `git submodule update --init` to fetch the pinned MLX core.

`cargo build` produces the `drakkar` binary; `cargo test` runs the unit and property
suites. Integration and conformance suites are described in
[docs/spec/07-testing-strategy.md](docs/spec/07-testing-strategy.md).

## Pull requests

- Keep PRs to one issue's scope. The issue's acceptance criteria are the review checklist.
- `cargo fmt` and `cargo clippy -- -D warnings` must pass; CI enforces both.
- User-visible changes add a `CHANGELOG.md` entry in the same PR
  ([docs/spec/09-release-and-versioning.md](docs/spec/09-release-and-versioning.md)).
- Commit messages: imperative subject line under 72 characters; body explains why, not
  what. No emojis, no vendor branding.

## Reporting bugs

File a GitHub issue with: `drakkar doctor --json` output, the exact command, the error
output (errors carry stable codes; include them), and macOS + hardware details. For
security reports, do not open a public issue — see [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE).
