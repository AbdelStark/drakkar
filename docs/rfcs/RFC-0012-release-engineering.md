# RFC-0012: Release Engineering and Distribution

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.1 (PR CI + unsigned dev builds); signing, notarization, and tap releases v0.2; fleet-gated releases v1.0

## Summary

This RFC locks how DRAKKAR is built, versioned, signed, and distributed. The artifact is
one arm64 macOS binary — statically linked C++ shim, embedded metallib, no dylib payload
beyond OS frameworks — published through GitHub Releases (canonical) and a Homebrew tap,
codesigned with a Developer ID certificate, notarized, stapled, checksummed, and carrying a
build provenance attestation. Versioning is SemVer with pre-1.0 minors mapped to roadmap
milestones (locked decision LD25). CI runs a correctness pipeline on every PR, a
soak/fuzz/supply-chain pipeline nightly, and a full release pipeline — including the
RFC-0009 PB16 performance gate on self-hosted M-series machines — on every tag. A
`cargo xtask release` command drives the mechanical steps so a release is reproducible by
any maintainer. The normative user-facing policy summary lives in
[09 — Release and Versioning](../spec/09-release-and-versioning.md) (RV1–RV37); this RFC is
the decision record and the engineering specification behind it.

## Motivation

Single-binary distribution is not packaging hygiene; it is the product wedge. The PRD names
Python environment friction as the top install failure class of the incumbent stacks
([PRD §2.3](../../PRD.md#23-where-existing-tools-fall-short)): wrong-arch Python, venv
drift, and source compilation at install time are why mlx-lm, vllm-metal, vllm-mlx, and
vMLX lose users before first token. PRD G6 makes the counter-position a goal — a signed,
notarized, dependency-free binary via Homebrew tap plus direct download
([PRD §4](../../PRD.md#4-goals-and-non-goals)) — and PRD M1 makes it measurable: fresh
machine, `brew install` to first generated token for an 8B model, under 5 minutes on a
300 Mbps connection ([PRD §7](../../PRD.md#7-success-metrics)).

RFC-0002 D5 sketched the distribution decision (arm64 binary, embedded metallib, codesign +
notarize, tap + GitHub Releases, pinned MSRV and MLX;
[Stack Selection](RFC-0002-stack-selection.md#proposed-design)) but left the pipeline,
gates, budgets, and process unspecified. A wedge that exists only as intent erodes: binaries
grow, unsigned builds leak into circulation, a release ships without the perf gate once and
then always. This RFC engineers the wedge — budgets with CI enforcement, a release process
that cannot skip its gates, and an integrity chain a third party can verify.

## Goals

- Define the release artifact exactly: contents, naming, size budget, signing and
  notarization requirements, and the verification assertions CI runs against it.
- Pin the entire toolchain (Rust, Xcode/clang, MLX, llama.cpp, lockfile) so a release is
  reproducible from a commit hash, and specify the MLX upgrade cadence and its validation
  checklist.
- Bind SemVer to the roadmap milestones and to the public-surface definition of
  [09 — Release §2](../spec/09-release-and-versioning.md#2-what-public-surface-means-for-semver).
- Specify the three CI pipelines (PR, nightly, release tag) including where each runs and
  why the performance gate requires physical M-series hardware.
- Specify the distribution channels for v0.x (GitHub Releases + Homebrew tap) and the
  channels deliberately not offered.
- Make the release process a single audited command (`cargo xtask release`) with changelog
  discipline enforced by CI, not convention.
- Design the v0.x pipeline so the v1.0 desktop-app pipeline reuses the engine build step
  without rework.

## Non-Goals

- Cross-platform artifacts (Linux, Windows, x86_64 macOS): excluded by PRD N1/N6 and locked
  decision LD21. The backend seam (RFC-0001) keeps the door open; this pipeline does not.
- Publishing `drakkar-*` crates to a package registry pre-1.0: the workspace is an
  application, not a library
  ([09 — Release RV7](../spec/09-release-and-versioning.md#2-what-public-surface-means-for-semver)).
- Auto-update before v1.0: pre-1.0 the only update surface is the explicit
  `drakkar doctor --check-update` (RFC-0008 CLI16,
  [CLI and UX](RFC-0008-cli-ux.md#proposed-design)).
- The desktop app's own release engineering beyond the interface it consumes (RE28–RE29):
  the `.dmg`, its update feed, and its identity are specified when v1.0 work starts.
- Reproducible builds in the bit-for-bit sense: the integrity chain (RE31) binds artifact to
  source and workflow via attestation instead; bit-reproducibility on macOS with codesigning
  is not attempted in v1.

## Proposed Design

### Artifact definition

- RE1. **One binary.** The release artifact is
  `drakkar-vX.Y.Z-aarch64-apple-darwin.tar.gz` containing the single arm64 `drakkar`
  executable, `LICENSE` (Apache-2.0, locked decision LD1), and `THIRD_PARTY_NOTICES`. The
  binary statically links the C++ shim and the pinned MLX core, embeds the Metal shaders as
  a metallib payload (no shader compilation at runtime, RFC-0002 D5), and statically links
  the llama.cpp backend when the `gguf` cargo feature is compiled in (on by default in
  release, RFC-0002 D4). The binary MUST link no dynamic libraries other than macOS system
  frameworks (`otool -L` output is asserted in CI against an allowlist: `libSystem`,
  `Metal`, `MetalPerformanceShaders`, `Foundation`, `IOKit`, `Accelerate`, and their
  transitive OS frameworks). No Python, no containers, no post-install steps, no runtime
  downloads except models the user requests (PRD P13).
- RE2. **Size budget.** The uncompressed binary size budget is 150 MB (est.; dominated by
  the static MLX core, embedded metallib, and the llama.cpp backend). CI records the size
  of every PR's release-profile build and posts the delta; a PR or release build exceeding
  200 MB fails CI absent a written, reviewed waiver recorded in the PR. The budget exists
  because "single binary" stops being a distribution advantage when the binary itself
  becomes the download problem; the number is revisited (not silently raised) if the v0.2
  measured baseline demands it.
- RE3. **Startup budget.** Binary launch to server-ready with a resident model MUST be
  under 200 ms (PRD P12). CI runs a startup-time check (`t_startup_check`, see Testing
  Strategy) on physical hardware from v0.2; on virtualized runners before that the check is
  advisory (recorded, not gating) because virtualized timing is unrepresentative.
- RE4. **Release contents.** A GitHub Release for tag `vX.Y.Z` MUST carry: the artifact
  tarball (RE1), a `SHA256SUMS` file covering every artifact, a build provenance
  attestation (RE31), the benchmark reproducibility manifest (RE30), and release notes
  generated from the changelog (RE25). A tag without all five is not a release
  ([09 — Release RV23](../spec/09-release-and-versioning.md#6-release-channels-and-cadence)).
- RE5. **Version identity.** `drakkar --version` MUST report the SemVer version, git
  commit, MLX pin, llama.cpp pin (when compiled in), and Rust toolchain, in the
  `drakkar.version/1` JSON schema defined in
  [09 — Release RV5](../spec/09-release-and-versioning.md#1-versioning-scheme). The release
  pipeline asserts this output matches the tag, the checked-out commit, and the submodule
  pins; a mismatch is a release-blocking defect.

### Toolchain and dependency pinning

- RE6. **Pinned Rust toolchain.** `rust-toolchain.toml` at the repository root pins the
  exact channel and version (e.g. `1.89.0`). CI and release builds use only the pinned
  toolchain, and release builds run `cargo build --locked --profile release`; a build that
  would modify `Cargo.lock` fails. A contributor's build and the shipped binary are
  compiled identically.
- RE7. **MSRV policy.** MSRV is stable-minus-two, evaluated at each release. An MSRV bump
  is a minor release event, never a patch, and appears in the changelog under `Changed`
  with old and new versions
  ([09 — Release RV14–RV16](../spec/09-release-and-versioning.md#4-msrv-and-toolchain-policy)).
  The same rule applies to the required Xcode Command Line Tools version for the C++17 shim
  (RFC-0002 D2); the exact clang version used is recorded in the build provenance.
- RE8. **Lockfile and supply chain.** `Cargo.lock` is committed and authoritative; crate
  upgrades land as ordinary PRs with the lockfile diff reviewed like code. `cargo audit`
  (RustSec advisories) and `cargo deny` (license allowlist, duplicate-version and source
  checks) run nightly (RE16) and as release gates (RE17); a release MUST NOT ship with an
  unwaived advisory against a shipped dependency
  ([06 — Security §3](../spec/06-security.md#3-threat-enumeration)).
- RE9. **Native pins.** MLX is vendored as a git submodule pinned to an exact commit hash
  — never a branch or tag reference. The pinned hash is part of the release identity (RE5)
  and of every published benchmark manifest (RE30, locked decision LD18). llama.cpp under
  the `gguf` feature is pinned the same way. No native dependency is fetched at build time
  from a mutable reference.
- RE10. **MLX upgrade cadence.** DRAKKAR MUST track MLX to within two upstream MLX
  releases (RFC-0002 D5): the product's performance claim depends on consuming
  first-to-silicon kernel work the day it lands ([PRD §2.2](../../PRD.md#22-software-landscape)),
  and falling further behind is a tracked defect against the `release` area. A pin bump is
  a dedicated PR that MUST pass the validation checklist of
  [09 — Release RV21](../spec/09-release-and-versioning.md#5-dependency-pinning-and-the-mlx-upgrade-cadence)
  before merge: (1) shim compiles warnings-as-errors with a byte-identical `dk_*` header or
  an approved ABI revision per [RFC-0010](RFC-0010-backend-abi.md); (2) golden-token
  fixtures identical or every diff triaged and re-blessed with justification (RFC-0003
  testing strategy); (3) `drakkar bench` workloads A–E on at least one Tier-1 machine with
  no metric regressing more than 3% and the NAX self-test passing on M5-class hardware
  (RFC-0009 PB14, [Performance](RFC-0009-performance.md#proposed-design)); (4) peak RSS
  within the declared memory contract (RFC-0009 PB4) — an MLX allocator behavior change is
  exactly the regression class this catches; (5) ABI fuzz corpus and sanitizer lanes green;
  (6) changelog entry with old hash, new hash, and upstream changes of note.

### Versioning and deprecation

- RE11. **SemVer, milestone-mapped pre-1.0.** Versions follow Semantic Versioning 2.0.0;
  tags are `vMAJOR.MINOR.PATCH`, annotated and signed. Pre-1.0, each `0.MINOR` is a roadmap
  milestone (locked decision LD25; [PRD §8](../../PRD.md#8-roadmap)): 0.1 "First light",
  0.2 "Convoy", 0.3 "Fleet". Breaking changes to any public surface are permitted only at a
  minor bump and MUST ship with a written migration note; patch releases are backward
  compatible, fixes and additive changes only. Pre-releases use `-rc.N` / `-beta.N`
  identifiers and are never promoted to the tap's default formula.
- RE12. **Strict post-1.0.** From v1.0.0, strict SemVer applies to the Stable-tier surface:
  breaking changes require a major bump; a major bump is an event, not a cadence
  ([09 — Release RV3](../spec/09-release-and-versioning.md#1-versioning-scheme)).
- RE13. **Deprecation.** Warn one minor, remove the next: a deprecated Stable surface keeps
  working for at least one full minor while emitting a warning that names the replacement
  and the removal release. JSON schemas are additive-only within a schema major version;
  error codes and CLI exit codes are append-only and never reused
  ([09 — Release RV9–RV12](../spec/09-release-and-versioning.md#3-deprecation-policy),
  [RFC-0011](RFC-0011-error-taxonomy.md)).
- RE14. **C ABI lifecycle.** The engine C ABI carries its own `DK_ABI_VERSION`, governed by
  [RFC-0010](RFC-0010-backend-abi.md), independent of the product version: Evolving tier
  through v0.x (revisions allowed at minors, tracked by the header-diff gate in RE10), and
  frozen — Stable tier, breaking revisions only at a product major — at v1.0, when the
  desktop shell and third-party embedders begin consuming it.

### CI pipelines

CI runs on GitHub Actions. Correctness pipelines use hosted macOS arm64 runners
(`macos-15` arm64 image or newer); performance and startup gates run only on self-hosted,
physically owned M-series machines, because virtualized Metal is unrepresentative of the
numbers the product defends (same reasoning as RFC-0009 PB11).

- RE15. **PR pipeline** (required checks; a PR cannot merge red):
  1. `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`;
  2. unit and property tests (`cargo test --workspace --locked`);
  3. the integration matrix (CLI exit codes × failure classes, RFC-0008 AC1; server
     endpoint conformance, RFC-0007 testing strategy);
  4. shim sanitizer job: the C++ shim's test suite and ABI boundary tests under ASan and
     UBSan (RFC-0002 §consequences; the FFI boundary is the one place Rust's guarantees
     stop, [06 — Security §2.3](../spec/06-security.md#23-b3-ffi-boundary));
  5. docs build: the spec/docs tree builds, all relative links and anchors resolve;
  6. schema-registry checks: every versioned `--json` schema (`drakkar.<cmd>/N`) and HTTP
     response schema in the registry is valid, and the additive-only rule (RE13) is
     mechanically checked against the previous release's registry;
  7. binary-size delta report against the RE2 budget;
  8. changelog presence check for PRs labeled with a user-facing area (RE25).
- RE16. **Nightly pipeline** (on `main`): a soak subset (2-hour mixed-load run with RSS
  drift assertion, scaled-down from the 24-hour release soak, PRD P14); the fuzz corpus
  (ABI fuzzers from RFC-0010, request-parsing fuzzers from RFC-0007, tokenizer/template
  fuzzers from RFC-0006) each run for a fixed time budget with new coverage persisted;
  `cargo audit` and `cargo deny`; and the release dry-run (RE18). Nightly failures page the
  maintainer via the repository's notification channel and block the next release until
  triaged.
- RE17. **Release-tag pipeline** (on `v*` tags), in order, each stage gating the next:
  1. full test matrix: everything in RE15 plus golden-fixture suites and the 24-hour soak
     (started ahead of the tag; the pipeline consumes its result);
  2. Tier-1 performance gate on the self-hosted M-series fleet: workloads A–E per
     RFC-0009 PB16 ([Performance](RFC-0009-performance.md#proposed-design)); a metric
     regressing more than 3% versus the last release blocks the release absent a written,
     reviewed waiver; memory-contract breaches (PB4) and NAX self-test failures (PB14) are
     non-waivable — no waiver path exists in the workflow for those two classes;
  3. package: build `--locked` on the pinned toolchain, assemble the RE1 tarball;
  4. sign and notarize: codesign with the Developer ID Application certificate
     (hardened runtime enabled), submit via `notarytool`, staple the ticket, then run the
     verification assertions (`codesign --verify --strict`, `spctl --assess`,
     `stapler validate`);
  5. publish: generate `SHA256SUMS`, produce the build provenance attestation (RE31),
     create the GitHub Release with changelog-derived notes, then bump the tap formula
     (RE21);
  6. post-publish: clean-VM install smoke test (RE21) and attestation verification test
     (Testing Strategy); failure of either yanks the release (the Release is marked
     pre-release/broken, the tap bump is reverted, and a patch release follows).
- RE18. **Release dry-run on every merge to `main`.** The package and sign stages run with
  a Development certificate and no publication: tarball assembled, codesigned, `codesign
  --verify --strict` asserted, `SHA256SUMS` and attestation generated and verified locally.
  Notarization and stapling are exercised end-to-end on `-rc.*` pre-release tags rather
  than per-merge (each notarization round-trips through Apple's service and is not free).
  The dry-run exists so that release day never discovers a broken packaging step: the
  pipeline that publishes is the pipeline that already ran today.
- RE19. **No performance gating on virtualized runners.** Perf, startup-time, soak, and
  energy numbers produced on hosted/virtualized runners MUST NOT gate anything or be
  published; they may be recorded as trend telemetry only. Gating numbers come exclusively
  from the self-hosted fleet (RFC-0009 PB11).

### Distribution channels

- RE20. **GitHub Releases is canonical.** A release exists when and only when a signed tag
  has a published GitHub Release with the full RE4 contents. Every other channel derives
  from it and pins its artifacts by SHA-256.
- RE21. **Homebrew tap.** The tap `abdelstark/homebrew-drakkar` carries the `drakkar`
  formula pinning the release URL and SHA-256. The release pipeline auto-bumps the formula
  as its final publish step (a PR to the tap repository opened and merged by the release
  automation), and the clean-VM smoke test then runs `brew install abdelstark/drakkar/drakkar`,
  `drakkar doctor`, and a tiny-model generation before the release is considered done. The
  tap is the primary install path (PRD G6, M1); direct download of the identical artifact
  is the secondary path.
- RE22. **Homebrew core: deferred.** A homebrew-core formula is not submitted before the
  public surface is Stable (v1.0+). Core's review latency and update process are
  incompatible with pre-1.0 milestone cadence and on-demand patch releases; shipping
  through core while the CLI and API surfaces still break at minors would strand users on
  stale formulas. The tap gives the same `brew install` experience under our own cadence.
- RE23. **No `curl | sh` installer in v0.x.** DRAKKAR does not publish a piped-to-shell
  install script: it trains users to execute unverified remote code, bypasses the RE25
  integrity chain of checksums + signature + notarization + attestation, and PRD G6 is
  fully satisfied by brew plus manual download. Documentation MUST NOT suggest third-party
  one-liner installers either. Revisit only post-1.0, and only as a thin verified-download
  wrapper, if install-funnel data shows brew and manual download genuinely losing users.

### Release process

- RE24. **`cargo xtask release` drives everything mechanical.** The xtask (an in-workspace
  automation crate; no external release tooling to install) executes, in order:
  preflight (clean tree on `main`, CI green at HEAD, changelog `[Unreleased]` non-empty and
  containing a migration note for every entry marked breaking, version-bump consistency
  across `Cargo.toml` workspace members), version bump commit, changelog cut
  (`[Unreleased]` renamed to `[X.Y.Z] - YYYY-MM-DD`, fresh empty `[Unreleased]` inserted),
  annotated signed tag, push, then monitors the RE17 pipeline and reports the result,
  including the tap bump. Interface:

  ```text
  cargo xtask release --level <major|minor|patch|rc> [--dry-run] [--allow-waiver <gate-id>]
  ```

  `--dry-run` performs every check and prints the would-be actions without committing,
  tagging, or pushing. `--allow-waiver` exists only for the single waivable gate (RE26) and
  requires the waiver document path as its argument.
- RE25. **Changelog discipline.** `CHANGELOG.md` follows the Keep a Changelog format
  (`Added`/`Changed`/`Deprecated`/`Removed`/`Fixed`/`Security`, ISO 8601 dates). Every
  user-visible change lands with its PR: CI (RE15.8) requires a changelog diff on any PR
  carrying a user-facing area label (`server`, `cli`, `models`, `fit`, `kv-cache`,
  `engine`, `release`); a `no-changelog` label with a stated reason is the only exemption
  and is reviewed like code. A release is blocked while any merged user-visible PR since
  the last release lacks its entry — the xtask preflight cross-checks merged PR labels
  against changelog entries. Release notes are the cut changelog section verbatim, prefixed
  with the artifact table (name, SHA-256, MLX pin) and migration notes, breaking changes
  listed first ([09 — Release RV29–RV32](../spec/09-release-and-versioning.md#7-changelog-discipline)).
- RE26. **Gates and waivers.** A release MUST NOT be tagged until the checklist of
  [09 — Release RV33](../spec/09-release-and-versioning.md#8-the-release-checklist) is
  green. The performance gate (RE17.2) is the only waivable gate, and only for its waivable
  metric classes; a waiver is a written, reviewed justification recorded in the release
  notes. Gates for tests, soak, docs, changelog, artifact integrity, version identity, and
  tap verification have no waiver path.
- RE27. **Patch cadence.** Milestone releases follow the roadmap intervals
  ([PRD §8](../../PRD.md#8-roadmap)); patch releases ship on demand. A confirmed
  user-facing regression, a memory-contract breach in the field, or a security advisory
  against a shipped dependency triggers a patch release within days, running the full RE17
  pipeline — there is no reduced "hotfix" pipeline, because the fast path that skips gates
  is how regressions ship.

### Desktop app pipeline (v1.0)

- RE28. **Separate artifact, shared engine.** The v1.0 menu-bar desktop app
  ([PRD §8](../../PRD.md#8-roadmap), "Harbor") ships as its own `.dmg` with
  Sparkle-framework (2.x) auto-update, signed under its own Developer ID identity and
  update feed keys, distinct from the CLI binary's identity so that a compromise or
  revocation of one does not strand the other. Its release engineering is out of scope for
  the v0.x pipeline and is specified when v1.0 work starts.
- RE29. **Engine step reusability.** The v0.x pipeline is factored so the desktop pipeline
  consumes the engine build as-is: the package stage (RE17.3) produces, alongside the CLI
  tarball, an engine library artifact (static library + the frozen `dk_*` header per
  [RFC-0010](RFC-0010-backend-abi.md)) with the same pins, checksums, and attestation. The
  desktop pipeline adds a shell, not a second engine build.

### Provenance

- RE30. **Reproducibility manifest.** Every release publishes the benchmark
  reproducibility manifest (locked decision LD18; RFC-0009
  [Performance](RFC-0009-performance.md#proposed-design)): machine identifiers, macOS
  version, MLX pin, model hashes, and harness settings for every published number, so a
  third party can reproduce or dispute any claim. The MLX and llama.cpp pins appear in the
  manifest, the release notes, and `drakkar --version` (RE5), and the three MUST agree.
- RE31. **Build attestation.** The release pipeline produces a build provenance attestation
  binding artifact digest → source commit → CI workflow run, published with the release.
  Combined with `SHA256SUMS` and the codesign/notarization chain, a user or package manager
  can verify (a) the bytes are what was published, (b) Apple scanned and ticketed the
  binary, and (c) the binary came from this repository's release workflow at a named
  commit — so a tampered rebuild is detectable at three independent layers
  ([06 — Security §3](../spec/06-security.md#3-threat-enumeration)).

## Alternatives Considered

- **`cargo install` distribution.** Rejected. It requires every user to hold a Rust
  toolchain plus Xcode CLT and compile MLX from source — the exact class of environment
  friction PRD G6 exists to eliminate, re-created in a different language. It also cannot
  carry the codesign/notarization chain. `cargo install --path .` remains a contributor
  convenience, never a documented install path.
- **Homebrew core from day one.** Rejected for v0.x (RE22). Core review latency and its
  no-tap-style-automation norms are incompatible with milestone cadence and days-not-weeks
  patch releases while surfaces still break at minors. The tap delivers the identical UX
  now; core is the v1.0+ graduation once the Stable tier is frozen.
- **`curl | sh` installer.** Rejected (RE23). It is a security anti-pattern (unverified
  remote code execution as the install ritual), it sidesteps every layer of the RE31
  integrity chain, and PRD G6 names brew plus direct download as sufficient. Convenience
  that costs the trust story of a binary whose pitch includes "numbers and bytes you can
  verify" is a bad trade.
- **nix / conda channels.** Rejected for v1. Audience mismatch — the target users
  ([PRD §3](../../PRD.md#3-target-users)) are reached by brew and GitHub; nix packaging of
  a codesigned, metallib-embedding macOS binary is real ongoing maintenance for a small
  population, and conda is the Python-ecosystem channel the product deliberately stands
  apart from. Community-maintained packages are welcome; they are not release-blocking
  channels and do not appear in the support matrix.
- **GitHub-hosted mac runners for the performance gate.** Rejected (RE19). Hosted runners
  virtualize the GPU path; virtualized Metal throughput, thermals, and memory behavior are
  unrepresentative of the machines the targets are defined on, and a gate built on them
  would pass regressions and fail healthy builds. This matches RFC-0009's fleet decision
  (PB11): gating numbers come from physically owned Tier-1 machines. Hosted runners keep
  the correctness pipelines cheap and parallel — the split is deliberate.

## Drawbacks

- **Self-hosted runner fleet.** Owned M-series machines (M4 Pro, M4 Max, then M5-class per
  RFC-0009 PB11) are capital cost plus real operations: OS updates that change performance
  baselines, runner security (a self-hosted runner executes repository code; it runs only
  tag builds from maintainer-created tags, never fork PRs), physical uptime. Accepted
  because the product's claims are indefensible without physical hardware.
- **Notarization dependency.** Every release round-trips through Apple's notarization
  service: added minutes per release, an external availability dependency, and a
  paid-developer-account requirement. Accepted as the cost of Gatekeeper-clean install UX;
  the dry-run (RE18) keeps failures early and rare rather than release-day surprises.
- **xtask automation is code to maintain.** The release xtask, the tap-bump automation,
  the schema-registry checker, and the size/startup trackers are internal tooling with
  their own bug surface. Accepted: the alternative is a human checklist, and human
  checklists skip steps under deadline pressure — which is how unsigned or ungated builds
  escape.
- **Strict changelog gating adds friction.** Contributors must write user-facing prose
  with code. Accepted deliberately; the `no-changelog` label with review is the pressure
  valve.

## Migration / Rollout

- **v0.1 "First light".** PR pipeline (RE15) required on every PR from the first commit;
  nightly `cargo audit`/`cargo deny`; unsigned development builds published as
  CI artifacts only (retention-limited, never as Releases — no unsigned binary is ever a
  release artifact); tap repository created with a formula skeleton pointing at nothing;
  `CHANGELOG.md`, `rust-toolchain.toml`, committed `Cargo.lock`, and pinned submodules in
  place; binary-size tracking live (advisory thresholds until the v0.2 baseline is
  measured); `cargo xtask release --dry-run` functional.
- **v0.2 "Convoy".** First signed, notarized, stapled public releases via the full RE17
  pipeline; Developer ID certificate provisioned; performance gate live on the owned
  M4 Pro / M4 Max machines (RFC-0009 AC2) — self-hosted runners commissioned this
  milestone; changelog gating (RE25) switched from advisory to required; tap formula live
  and auto-bumped; clean-VM install smoke and startup-time gate active; release dry-run on
  every `main` merge; size budget enforced at the RE2 hard limit.
- **v0.3 "Fleet".** Nightly channel: the nightly pipeline additionally publishes a signed
  (not notarized) rolling pre-release build for early adopters, clearly labeled, installed
  only via an explicit `--HEAD`-style tap formula variant; full soak-subset and fuzz
  corpus nightly (RE16); attestation verification in the post-publish stage.
- **v1.0 "Harbor".** Full Tier-1 fleet attached to CI (M5-class machines added, `est.`
  targets converted to measured per RFC-0009 AC2); desktop pipeline consuming the RE29
  engine artifact; `DK_ABI_VERSION` frozen (RE14); Homebrew core submission evaluated
  (RE22). Post-1.0 support windows, finalized at the v1.0 release checklist per
  [09 — Release RV36](../spec/09-release-and-versioning.md#9-support-policy), with this
  working policy going in: the latest minor receives all fixes; the previous minor
  `1.(N-1)` receives security backports for 90 days after `1.N` ships; nothing older is
  supported.

## Testing Strategy

The pipeline is itself a tested artifact; every gate has a test that proves it gates.

- **Unit (xtask):** `t_xtask_preflight_dirty_tree`, `t_xtask_preflight_missing_changelog`,
  `t_xtask_version_bump_consistency`, `t_xtask_changelog_cut_format` (Keep a Changelog
  round-trip), `t_xtask_waiver_requires_document`.
- **Artifact assertions (release and dry-run):** `t_artifact_dylib_allowlist` (`otool -L`
  against RE1's allowlist), `t_artifact_size_budget` (RE2), `t_artifact_metallib_embedded`
  (binary serves a generation with shader-compiler toolchain absent),
  `t_codesign_verify_strict`, `t_spctl_assess_accepts`, `t_stapler_validate` (release path
  only), `t_sha256sums_cover_all_assets`, `t_version_identity_matches_tag_and_pins` (RE5).
- **Attestation:** `t_attestation_verifies_digest_commit_workflow` — the post-publish stage
  downloads the published artifact as an anonymous client and verifies the attestation
  binds its digest to the release commit and workflow (RE31); a deliberately corrupted
  byte fails verification.
- **Gate-negative tests (seeded failures, run when the gate is commissioned and after any
  pipeline change):** a seeded 5% decode regression and a seeded NAX-disable MUST block a
  dry-run release (RFC-0009 AC4); a seeded 201 MB binary MUST fail RE2; a seeded
  user-facing PR without a changelog entry MUST fail RE15.8; a seeded lockfile drift MUST
  fail the `--locked` build (RE6).
- **Install smoke (release gate, clean VM image per macOS baseline):**
  `t_brew_install_from_tap` → `t_doctor_clean_exit_zero` (schema-valid `--json`, RFC-0008
  AC1) → `t_tiny_model_generation` (a fixture-scale model generates tokens) →
  `t_startup_under_200ms` on physical hardware (RE3, PRD P12). Manual-download variant:
  untar, `xattr` quarantine intact, first launch passes Gatekeeper without prompts beyond
  the standard first-open flow.
- **MLX upgrade rehearsal:** every pin-bump PR runs the full RE10 checklist on a branch —
  the parity bench (RFC-0009 PB17 cross-engine baseline plus 3% self-regression check) and
  the golden-fixture suite are the required evidence attached to the PR; `t_abi_header_diff`
  asserts the `dk_*` surface is unchanged or the RFC-0010 revision procedure was followed.
- **Soak and fuzz:** nightly 2-hour soak asserts RSS drift within budget and zero request
  failures; release 24-hour soak per PRD P14; fuzz corpus regressions (any crasher found
  nightly becomes a permanent regression test before the next release may ship).
- **Schema-registry checks:** `t_schema_additive_only` diffs every `drakkar.<cmd>/N` and
  HTTP schema against the previous release; a removed or retyped field without a schema
  major bump fails (RE13, RE15.6).

Acceptance for this RFC's own rollout: at v0.2, one full release executed end-to-end by
`cargo xtask release` with zero manual steps between preflight and tap verification, and
every gate-negative test demonstrably red-then-green in the pipeline's history.

## Open Questions

1. **Self-hosted runner topology.** Which owned machines join CI first (M4 Pro and M4 Max
   are committed; M5-class timing depends on fleet acquisition per RFC-0009 PB11), whether
   they are co-located with the maintainer or hosted in a managed Mac colocation facility,
   and the isolation model between the runner user and the bench user on shared hardware
   (bench runs need a quiet machine; CI runs need throughput). Owner: abdelstark.
   Resolution: decided in v0.2 when the performance gate activates; recorded as an
   amendment to this RFC's CI section and in the RE30 manifest's machine identifiers.

## References

- [PRD](../../PRD.md): §2.3 (packaging failure class), §4 G6, §7 M1, §8 roadmap
- [RFC-0002: Technology Stack Selection](RFC-0002-stack-selection.md) — D2/D4/D5, shim and
  pinning decisions
- [RFC-0009: Performance Targets and Benchmark Methodology](RFC-0009-performance.md) —
  PB4, PB11, PB14, PB16, PB17, reproducibility manifest (LD18)
- [RFC-0010: Backend C ABI](RFC-0010-backend-abi.md) — `DK_ABI_VERSION` lifecycle,
  header-diff gate
- [RFC-0011: Error Taxonomy](RFC-0011-error-taxonomy.md) — append-only error-code registry
- [RFC-0008: CLI and UX](RFC-0008-cli-ux.md) — CLI8 exit codes, CLI16 update check, AC1
- [09 — Release and Versioning](../spec/09-release-and-versioning.md) — normative policy
  summary (RV1–RV37) derived from this RFC
- [06 — Security Model](../spec/06-security.md) — supply-chain and artifact-tampering
  threats
- Semantic Versioning 2.0.0 (semver.org); Keep a Changelog 1.1.0 (keepachangelog.com)
- Apple: Developer ID signing, hardened runtime, `notarytool`, `stapler`, Gatekeeper/spctl
  documentation
- RustSec advisory database (`cargo audit`); `cargo deny` documentation
- Sparkle framework 2.x (desktop auto-update, v1.0 scope)
