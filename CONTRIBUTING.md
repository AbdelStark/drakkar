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

## Spec-first workflow

Code follows the corpus, never the other way around.

1. **Find or open the issue.** Every PR closes a GitHub issue, and every issue
   cites the spec section or RFC it implements. Work with no spec basis needs a
   spec/RFC PR first.
2. **Change the spec before the code when the contract moves.** New subsystems,
   algorithms whose correctness is not obvious from the type signature,
   cross-cutting concerns, external boundaries, or any choice a reasonable
   engineer would make differently require a **new RFC** — copy
   [`docs/rfcs/RFC-TEMPLATE.md`](docs/rfcs/RFC-TEMPLATE.md), which matches the
   shipped RFC structure (Status, Authors, Created, Target milestone, Summary,
   Motivation, Goals, Non-Goals, Proposed Design, Alternatives Considered,
   Drawbacks, Migration/Rollout, Testing Strategy, Open Questions, References).
   An accepted RFC is *superseded, never edited into a different decision*
   ([SPEC.md](SPEC.md#change-process)).
3. **Mint requirement IDs.** Each spec document and RFC assigns per-document IDs
   to its load-bearing statements (`A1–A12` in RFC-0001, `FE1–FE27` in RFC-0004,
   `KV1–KV24` in RFC-0005, …; spec sections mint `API-`/`DM-`/`SEC-`/… prefixes
   for contracts not already carried by an RFC). Use RFC 2119 keywords (MUST,
   SHOULD, MAY). Locked decisions are cited as `LDn`
   ([decision log](docs/spec/11-decision-log.md)).
4. **Cite the IDs from the implementation.** The PR body and, where useful, the
   commit message and code comments reference the requirement IDs the change
   satisfies, so a reviewer can trace intent → contract → code.

## The review gate: invariants I1–I5

Every PR is reviewed against the five architecture invariants
([docs/spec/01-architecture.md §10](docs/spec/01-architecture.md#10-invariants-the-review-contract)).
A PR that weakens one is rejected unless it amends RFC-0001 first and says so:

- **I1** — one engine thread per model; all GPU state confined to it.
- **I2** — the memory contract: `weights + kv_pool + activation_watermark +
  runtime_overhead <= declared_budget` at all times.
- **I3** — single source of memory math: sizing formulas live only in
  `drakkar-fit`.
- **I4** — every user-facing surface (CLI, HTTP) has a versioned JSON
  representation.
- **I5** — the backend seam is the only portability boundary; nothing above it
  names Metal, MLX, or llama.cpp types (enforced by the `cargo deny check bans`
  seam-deps gate and the `cargo public-api` diff).

The layering (DEP1–DEP7) is mechanically enforced. Run the gate locally:

```
cargo test -p drakkar-core --test dep_direction   # DEP1–DEP6 layer graph (via cargo metadata)
cargo deny check                                  # seam-deps bans (DEP4/DEP5), licenses, advisories
```

`dep_direction` reads the live workspace graph and fails on a same-layer edge, a
backend→engine edge, or `drakkar-core`/`drakkar-mlx-sys` gaining a workspace
dependency. The `cargo public-api` diff (DEP5: a backend crate re-exporting an
FFI type) additionally runs in CI once the shim lands and there is a public FFI
surface to diff; install it with `cargo install cargo-public-api`.

## Changelog discipline

Every user-visible change lands with its own `[Unreleased]` entry in
[`CHANGELOG.md`](CHANGELOG.md) in the same PR (RV30,
[docs/spec/09-release-and-versioning.md §7](docs/spec/09-release-and-versioning.md)).
The file follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/):
add lines under the appropriate `Added` / `Changed` / `Deprecated` / `Removed` /
`Fixed` / `Security` subsection. CI fails a PR that carries a user-facing area
label without a changelog entry; a change that is genuinely not user-visible
(internal refactor, test-only, docs) carries the `no-changelog` label to skip
the check.

## Development setup

- Apple Silicon Mac, macOS 15+ (macOS 26.2+ to exercise the Neural Accelerator paths).
- Rust toolchain pinned by `rust-toolchain.toml` (installed automatically by rustup).
- Xcode command line tools and CMake (the MLX shim builds via `build.rs`).
- `git submodule update --init` to fetch the pinned MLX core.

`cargo build` produces the `drakkar` binary; `cargo test` runs the unit and property
suites. Integration and conformance suites are described in
[docs/spec/07-testing-strategy.md](docs/spec/07-testing-strategy.md).

### Toolchain and MSRV

`rust-toolchain.toml` pins the exact channel, version, and target
(`aarch64-apple-darwin`), so CI, contributors, and release builds all compile
identically — `rustup` installs it automatically on the first `cargo` command,
and `cargo build` resolves it without a manual `+toolchain` (RFC-0012 RE6,
release §4 RV14–RV17).

The pinned version is also DRAKKAR's **MSRV**: the workspace is built on the
oldest supported stable, so the `rust-version` declared in `Cargo.toml` is
guaranteed to compile. The MSRV policy is **stable-minus-two**, evaluated at
each release; raising it is a **minor** release and lands with a `Changed`
[changelog](CHANGELOG.md) entry (RE6/RE7).

The C++17 MLX shim (`drakkar-mlx-sys`, built via `build.rs`) requires the
**Xcode Command Line Tools** (clang; Apple clang 15+ / Xcode 15+ for C++17 and
Metal) and CMake. The exact clang and Xcode CLT versions are recorded in the
release build provenance (RE7) so a build is reproducible.

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
