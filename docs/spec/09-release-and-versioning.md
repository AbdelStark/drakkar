# 09 — Release and Versioning

This document is the normative policy for how DRAKKAR is versioned, deprecated, pinned,
built, published, and supported. The decision record behind it is
[RFC-0012: Release Engineering](../rfcs/RFC-0012-release-engineering.md); the distribution
constraints derive from RFC-0002 D5
([Stack Selection](../rfcs/RFC-0002-stack-selection.md#proposed-design)) and the milestone
plan from [PRD §8](../../PRD.md#8-roadmap). Requirement IDs in this document are `RV-n`.

Licensing context: the engine repository is licensed Apache-2.0 (locked decision LD1; see
[00-overview](11-decision-log.md)). Every release artifact carries the license text.

## 1. Versioning scheme

- RV1. DRAKKAR versions MUST follow Semantic Versioning 2.0.0. Release tags are
  `v<MAJOR>.<MINOR>.<PATCH>` (e.g. `v0.2.1`), annotated and signed.
- RV2. **Pre-1.0 policy.** Each `0.MINOR` corresponds to a roadmap milestone
  ([PRD §8](../../PRD.md#8-roadmap)): `0.1` = "First light", `0.2` = "Convoy",
  `0.3` = "Fleet". Breaking changes to any public surface (§2) are permitted **only** at a
  minor bump, and every breaking change MUST ship with a written migration note in the
  changelog's `Changed`/`Removed` entries (old behavior, new behavior, exact user action).
  Patch releases (`0.x.y`, y > 0) MUST be backward compatible: fixes and additive changes
  only.
- RV3. **Post-1.0 policy.** From `v1.0.0` onward, strict SemVer applies: breaking changes to
  any Stable-tier surface (§2) require a major bump; additive changes require a minor bump;
  everything else is a patch. There is no scheduled `v2.0` — a major bump is an event, not a
  cadence.
- RV4. Pre-release builds use SemVer pre-release identifiers: `v0.2.0-rc.1`,
  `v0.2.0-beta.2`. Pre-releases are published through the same channels (§6) but marked
  pre-release and never promoted to the Homebrew tap's default formula.
- RV5. `drakkar --version` (and `GET /health` in the server) MUST report, in one line and in
  the `--json` schema `drakkar.version/1`: the SemVer version, the exact git commit, the
  vendored MLX commit hash (§5), the llama.cpp pin when the `gguf` feature is compiled in,
  and the Rust toolchain used to build. A binary whose reported pins do not match its build
  provenance (§6) is a release-blocking defect.

```json
{
  "$schema": "drakkar.version/1",
  "version": "0.2.1",
  "commit": "a1b2c3d",
  "mlx_commit": "e4f5a6b7...",
  "gguf_backend": {"enabled": true, "llama_cpp_commit": "c8d9e0f1..."},
  "rustc": "1.89.0",
  "target": "aarch64-apple-darwin"
}
```

## 2. What "public surface" means for SemVer

SemVer is meaningless without a defined surface. DRAKKAR's compatibility promise covers
exactly the tiers defined in [02-public-api](02-public-api.md#7-stability-tiers); this section
binds them to the version policy.

- RV6. The SemVer-governed public surface is:

| Surface | Contract | Reference |
| --- | --- | --- |
| HTTP endpoints and their wire schemas (`/v1/chat/completions`, `/v1/messages`, `/fit`, ...) | Stable tier | [02-public-api](02-public-api.md#7-stability-tiers), [RFC-0007](../rfcs/RFC-0007-api-server.md#proposed-design) |
| CLI command names, flags, exit codes, `--json` schemas (`drakkar.<cmd>/N`) | Stable tier | [06-cli](../rfcs/RFC-0008-cli-ux.md), RFC-0008 CLI6/CLI8 ([CLI and UX](../rfcs/RFC-0008-cli-ux.md#proposed-design)) |
| Error taxonomy: error codes and their meanings | Stable tier | [04-error-model](04-error-model.md), [RFC-0011](../rfcs/RFC-0011-error-taxonomy.md) |
| The engine C ABI (`dk_*`) consumed by external embedders (the v1.0 desktop shell and third parties) | Stable from v1.0; Evolving before | [RFC-0010](../rfcs/RFC-0010-backend-abi.md) |
| Config file keys (`config.toml`) and `DRAKKAR_*` env vars | Stable tier | RFC-0008 CLI10 |
| On-disk store layouts under `~/.drakkar/` (model store manifests, KV tier, calibration files) | Evolving tier: versioned, migrated automatically, no hand-editing promise | RFC-0006 MP10 ([Model Pipeline](../rfcs/RFC-0006-model-pipeline.md#proposed-design)) |

- RV7. Explicitly **not** public surface, changeable in any release: Rust crate APIs of the
  `drakkar-*` workspace crates (the workspace is an application, not a library; crates are
  not published to a registry pre-1.0), internal actor messages, the shim's C++ internals,
  log line formats on stderr, human-oriented (non-`--json`) terminal output, and benchmark
  internals. Anything a program should parse has a versioned JSON form (RFC-0001 I4); if it
  does not, parsing it is unsupported.
- RV8. Surfaces marked Experimental in [02-public-api](02-public-api.md#7-stability-tiers)
  (e.g. `/v1/responses` behind its config flag in v0.3, locked decision LD5) carry no
  compatibility promise until promoted; promotion to Stable happens at a minor release and
  is recorded in the changelog.

## 3. Deprecation policy

- RV9. **Warn one milestone, remove the next.** A deprecated Stable surface MUST keep
  working for at least one full minor release while emitting a deprecation warning, and MAY
  be removed in the following minor release (pre-1.0) or the next major (post-1.0). Warnings
  appear: on stderr for CLI surfaces, as a `Deprecation` response header plus a
  `warnings: []` field in JSON responses for HTTP surfaces, and in the changelog's
  `Deprecated` section the moment the warning ships.
- RV10. Every deprecation warning MUST name the replacement and the release in which removal
  is scheduled ("`--foo` is deprecated, use `--bar`; removal in v0.4"). A deprecation with
  no replacement path is not permitted; if the capability is being dropped outright, the
  warning states that and links the changelog entry explaining why.
- RV11. **JSON schemas are additive-only within a schema major version.** A
  `drakkar.<cmd>/N` or HTTP response schema may gain optional fields at any release; it MUST
  NOT remove fields, change field types, or change field semantics. Any of those requires
  minting `drakkar.<cmd>/N+1`, and the old schema keeps being served for the deprecation
  window of RV9. Consumers MUST ignore unknown fields (documented in
  [02-public-api](02-public-api.md#7-stability-tiers)).
- RV12. **Error codes are never reused.** Once an error code from the taxonomy
  ([04-error-model](04-error-model.md), [RFC-0011](../rfcs/RFC-0011-error-taxonomy.md)) has
  shipped in any release, its identifier is retired permanently upon removal — it is marked
  reserved in the taxonomy table and never assigned a new meaning. The same rule applies to
  CLI exit codes (RFC-0008 CLI8): the numeric assignments are append-only.
- RV13. On-disk formats (Evolving tier) version themselves in-band: store manifests, KV-tier
  metadata, and calibration files each carry a `format_version` integer. A newer binary MUST
  read all format versions shipped since the previous milestone and migrate forward
  automatically and atomically; downgrade is not supported, and `drakkar doctor` reports the
  format versions present.

## 4. MSRV and toolchain policy

- RV14. The Minimum Supported Rust Version is **stable minus two**: the newest stable Rust
  release minus two minor versions, evaluated at each DRAKKAR release. DRAKKAR builds are
  not promised on anything older.
- RV15. The exact toolchain is pinned in `rust-toolchain.toml` at the repository root
  (channel + version, e.g. `1.89.0`); CI and release builds use only the pinned toolchain,
  so a contributor's `cargo build` and the shipped binary are compiled identically.
- RV16. An MSRV bump is a **minor** release event, never a patch: it appears in the
  changelog under `Changed` with the old and new MSRV. Because DRAKKAR ships binaries rather
  than a library crate, an MSRV bump affects contributors and packagers, not end users; it
  is still surfaced because the Homebrew formula's `--build-from-source` path depends on it.
- RV17. The C++ shim pins its language standard (C++17, per RFC-0002 D2) and the release
  build records the exact Xcode/clang version in the build provenance (§6). Bumping the
  required Xcode toolchain follows the same rule as RV16: minor release, changelog entry.

## 5. Dependency pinning and the MLX upgrade cadence

- RV18. `Cargo.lock` is committed and authoritative. Release builds use `--locked`; a
  release build that would modify the lockfile fails. Crate upgrades land as ordinary PRs
  with the lockfile diff reviewed like code.
- RV19. MLX is vendored as a git submodule pinned to an **exact release tag** (resolved to
  and recorded as its commit hash by the submodule), never a branch (RFC-0002 D5). The shim links MLX C++ directly; the pinned hash is
  part of the release's identity (RV5) and of every published benchmark's reproducibility
  manifest (RFC-0009 PB-manifest, locked decision LD18). The same rule applies to the
  vendored llama.cpp under the `gguf` feature (RFC-0002 D4).
- RV20. **Upgrade cadence:** DRAKKAR MUST track MLX to within two upstream MLX releases
  (RFC-0002 D5). Falling more than two releases behind is a tracked defect against the
  `release` area, because the product's performance claim depends on consuming
  first-to-silicon kernel work (PRD §2.2) the day it lands.
- RV21. An MLX pin bump is a dedicated PR that MUST pass this validation checklist before
  merge, in order:
  1. Shim compiles against the new pin with zero warnings-as-errors; the `dk_*` ABI surface
     is byte-identical (header diff empty) or the change is an approved ABI revision per
     [RFC-0010](../rfcs/RFC-0010-backend-abi.md).
  2. Golden-token fixtures (RFC-0003 testing strategy) produce identical outputs for the
     fixture model set, or every diff is triaged to an intentional upstream numeric change
     and the fixtures are re-blessed in the same PR with justification.
  3. Full `drakkar bench` workloads A–E on at least one Tier-1 machine
     (RFC-0009 PB9/PB11): no metric regresses more than 3% versus the current pin; the NAX
     self-test (PB14) passes on M5-class hardware.
  4. Memory contract check: peak RSS on the bench matrix within the declared budget
     (RFC-0009 PB4) — an MLX allocator behavior change is exactly the class of regression
     this catches.
  5. ABI fuzz corpus and sanitizer suite (ASan/UBSan CI lanes, RFC-0002 §consequences) green.
  6. Changelog entry recording old hash, new hash, and upstream changes of note.
- RV22. All other native dependencies (Metal shaders embedded as metallib, tokenizer data,
  the shipped alias table) are build inputs versioned in-repo; nothing is fetched at
  runtime except models the user requests (PRD P13, RFC-0008 CLI16).

## 6. Release channels and cadence

- RV23. **GitHub Releases is the canonical channel.** A release exists when and only when a
  signed tag `vX.Y.Z` has a published GitHub Release carrying: the binary artifact, a
  `SHA256SUMS` file, a build provenance attestation, and release notes generated from the
  changelog (§7). Everything else (tap, docs site) derives from it.
- RV24. The artifact is a single `drakkar-vX.Y.Z-aarch64-apple-darwin.tar.gz` containing the
  arm64-only binary (locked decision LD21; Apple Silicon is the only target, PRD N1/N6),
  codesigned with a Developer ID certificate and notarized (RFC-0002 D5). No Python, no
  dylib payloads, no post-install steps.
- RV25. Integrity chain, all three verifiable by a user or a package manager:
  1. `SHA256SUMS` for every artifact in the release;
  2. codesign signature + Apple notarization ticket stapled to the binary;
  3. a build provenance attestation binding artifact digest → source commit → CI workflow,
     so a tampered rebuild is detectable.
- RV26. **Homebrew tap** (`drakkar` formula in the project tap) is the primary install path
  (PRD G6, M1). The formula pins the release URL and SHA-256 and is bumped as the final step
  of the release checklist (§8). Direct download from GitHub Releases is the secondary path;
  both install the identical artifact.
- RV27. Cadence: milestone releases follow the roadmap intervals (PRD §8: 10/10/8/12 weeks
  for v0.1/v0.2/v0.3/v1.0). Patch releases ship on demand — a confirmed regression, a
  memory-contract breach, or a security issue in a dependency triggers a patch release
  within days, not at the next milestone. There is no fixed calendar cadence pre-1.0.
- RV28. Auto-update is out of scope until v1.0 ("Harbor", desktop app); pre-1.0 the only
  update surface is `drakkar doctor --check-update`, which is on-demand and explicit
  (RFC-0008 CLI16).

## 7. Changelog discipline

- RV29. `CHANGELOG.md` at the repository root follows the Keep a Changelog format:
  an `## [Unreleased]` section at the top, then one `## [X.Y.Z] - YYYY-MM-DD` section per
  release, each with `Added` / `Changed` / `Deprecated` / `Removed` / `Fixed` / `Security`
  subsections as needed. Dates are ISO 8601.
- RV30. **Every user-visible change lands with its PR.** A PR that changes any public
  surface (§2), any performance characteristic worth claiming, or any documented behavior
  MUST include its `[Unreleased]` changelog entry in the same PR; CI enforces the presence
  of a changelog diff for PRs labeled with a user-facing area (server, cli, models, fit,
  kv-cache, engine). Internal refactors carry no entry.
- RV31. Release notes are generated from the changelog, not written from memory: cutting a
  release renames `[Unreleased]` to the version + date, and the GitHub Release body is that
  section verbatim, prefixed with the artifact table (name, SHA-256, MLX pin) and any
  migration notes required by RV2.
- RV32. Breaking changes and deprecations are listed first in the release notes, each with
  its migration note. A release note that buries a breaking change below feature bullets
  fails review.

## 8. The release checklist

- RV33. A release MUST NOT be tagged until every item below is green. The checklist is
  mechanical and lives in the repository as the release runbook; the released artifact links
  back to the CI run that proved each gate.

| # | Gate | Pass condition | Source |
| --- | --- | --- | --- |
| 1 | Bench gate on Tier-1 | Workloads A–E on the Tier-1 fleet; no metric regressed > 3% vs last release; memory-contract breaches and NAX self-test failures are non-waivable | RFC-0009 PB16 ([Performance](../rfcs/RFC-0009-performance.md#proposed-design)) |
| 2 | Soak | 24-hour mixed-load soak: RSS drift < 2% after warmup, zero request failures | PRD P14, [07-performance-and-bench](08-performance-budget.md) |
| 3 | Test suite | Unit, property, integration matrix (CLI exit codes × failure classes, RFC-0008 AC1), golden fixtures, ABI fuzz + sanitizer lanes all green on the pinned toolchain | per-RFC testing strategies |
| 4 | Docs | Spec/docs updated for every changelog entry touching a public surface; `--help` text and JSON schemas match the docs | RV30 |
| 5 | Changelog | `[Unreleased]` complete, migration notes for every breaking change, section renamed to the version | RV29–RV32 |
| 6 | Artifact | Built `--locked` on the pinned toolchain, codesigned, notarized (ticket stapled), `SHA256SUMS` and provenance attestation published | RV24–RV25 |
| 7 | Version identity | `drakkar --version` output matches tag, commit, and MLX pin | RV5 |
| 8 | Tap bump | Homebrew formula updated to the new URL + SHA-256; `brew install` from the tap verified on a clean machine | RV26 |

- RV34. A waiver for gate 1 (the only waivable gate, per RFC-0009 PB16) requires a written,
  reviewed justification recorded in the release notes. Gates 2–8 have no waiver path.

## 9. Support policy

- RV35. **Pre-1.0: latest release only.** Bug fixes, security fixes, and performance
  regressions are fixed on `main` and released in the next patch or milestone release; no
  backports to older 0.x lines. A user reporting a bug on an old release is asked to
  reproduce on the latest.
- RV36. Post-1.0 support windows (whether `1.(N-1)` receives security backports once `1.N`
  ships) are defined in RFC-0012's rollout section and finalized no later than the v1.0
  release checklist; until then the pre-1.0 rule stands.
- RV37. macOS support follows PRD P15: macOS 15+ baseline, with Neural Accelerator paths
  runtime-detected on macOS 26.2+. Dropping a macOS baseline version is a breaking change
  under RV2/RV3 (it removes users' ability to run the artifact) and follows the deprecation
  policy: announced one milestone ahead, removed the next.

## Cross-references

- Decision record: [RFC-0012 Release Engineering](../rfcs/RFC-0012-release-engineering.md)
- Stability tiers: [02-public-api](02-public-api.md#7-stability-tiers)
- Error code registry: [04-error-model](04-error-model.md),
  [RFC-0011 Error Taxonomy](../rfcs/RFC-0011-error-taxonomy.md)
- Bench gate and reproducibility manifests:
  [RFC-0009 Performance](../rfcs/RFC-0009-performance.md#proposed-design),
  [07-performance-and-bench](08-performance-budget.md)
- Distribution decision: RFC-0002 D5
  ([Stack Selection](../rfcs/RFC-0002-stack-selection.md#proposed-design))
- C ABI stability: [RFC-0010 Backend ABI](../rfcs/RFC-0010-backend-abi.md)
