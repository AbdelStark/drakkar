# 07 — Testing Strategy

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Applies to: all milestones (v0.1 "First light" through v1.0 "Harbor")

This document defines the complete test pyramid for DRAKKAR, the corpora and fixtures the
tests consume, and the CI gates that decide whether a pull request merges and whether a
release ships. It is the single normative mapping from every acceptance criterion in
RFC-0003 through RFC-0009 to a named test layer and a named CI job. The product principle
"honest speed" ([PRD §1](../../PRD.md#1-vision)) applies to the test suite itself: a test
that can pass while the requirement it covers is broken is a defect in this document.

Requirement IDs minted here use the `TS` prefix. IDs cited from other documents (`IC*`,
`FE*`, `KV*`, `MP*`, `AS*`, `CLI*`, `PB*`, `A*`, `I*`, `AB*`, `ER*`, `RE*`) are defined in
their source RFCs and are not restated normatively.

## 1. Principles and the traceability rule

- TS1. **Total traceability.** Every acceptance criterion in RFC-0003..RFC-0009 (and every
  `AB*`/`ER*`/`RE*` criterion in RFC-0010..RFC-0012) MUST map to at least one automated
  test in a named layer (§3–§9) and at least one named CI job (§10). The matrix in §11 is
  normative; a new acceptance criterion merged without a matrix row is a CI failure
  (`traceability-check` job, §10).
- TS2. **Hermetic by default.** PR-triggered jobs MUST NOT touch the network. Hub
  interactions run against `hub-sim` (§9.1), model weights come from the pinned CI model
  set (§2.2), and clocks/randomness are injected. Only nightly and release jobs on fleet
  hardware may reach real endpoints, and only to refresh recorded fixtures.
- TS3. **No masked flakiness.** Test runners are configured with zero automatic retries.
  A test that fails intermittently is quarantined by adding it to
  `ci/quarantine.toml` with an owner and an expiry date at most 14 days out; an expired
  quarantine entry fails CI. Quarantined tests still run and report, they just do not gate.
- TS4. **Determinism policy.** Tests that assert token-level output run greedy
  (temperature 0) or with a fixed seed under a fixed batch schedule, per the documented
  determinism contract of RFC-0003 (LD6: "reproducible given identical batch schedule").
  Concurrency tests therefore assert *properties* (accounting equality, latency bounds,
  schema validity), never exact token streams.
- TS5. **Coverage is informational, not a gate.** Line/branch coverage is collected with
  `cargo llvm-cov` and published per PR; the merge gate is the traceability matrix and the
  job set in §10, not a coverage percentage. A percentage gate rewards trivial tests; the
  matrix rewards covering contracts.

## 2. Test infrastructure

### 2.1 Tooling and dev-dependencies

| Tool | Version constraint | Reason |
| --- | --- | --- |
| `cargo test` / `cargo-nextest` | nextest `^0.9` | Per-test process isolation (required for FFI abort tests, §7) and stable machine-readable results; no retries (TS3) |
| `proptest` | `^1` | Property tests with shrinking (§4) |
| `insta` | `^1` | Snapshot tests for JSON schemas and error envelopes (§5.3, §5.4) |
| `criterion` | `^0.5` | Micro-benchmarks for hot Rust paths (scheduler step, hash chain, grammar mask), per RFC-0002 D6 |
| `cargo-fuzz` / `libfuzzer-sys` | fuzz `^0.12`, sys `^0.4` | Coverage-guided fuzzing of untrusted-input parsers and the ABI array lifecycle (§7.2) |
| `assert_cmd` + `predicates` | `^2` / `^3` | CLI integration matrix (§6): spawn the real binary, assert stdout/stderr/exit codes |
| `wiremock` | `^0.6` | `hub-sim`: recorded Hugging Face hub API responses served locally (§9.1) |
| `cargo llvm-cov` | `^0.6` | Coverage reporting (TS5) |
| AddressSanitizer / UndefinedBehaviorSanitizer | Xcode clang (pinned per release) and Rust `-Z sanitizer` on the pinned nightly toolchain | FFI safety suite (§7); the shim is C++ and ASan is the only practical detector for its failure class |
| Python client harness | Python `3.12` (CI-only), `openai` and `anthropic` SDKs pinned by exact version per release | Dialect conformance is *defined* against the official client SDKs (RFC-0007 AS1); the shipped product contains no Python — this environment exists only inside the conformance job (§5A) |

The C++ shim's own unit tests build via the shim's CMake preset with
`-fsanitize=address,undefined` in the sanitizer configuration (§7.1) and without
sanitizers in the default `test-unit` job.

### 2.2 CI model set

Inference-bearing tests use a pinned, content-addressed model set (hashes recorded in
`ci/models.lock`, refreshed only by explicit PR):

| Name | Model | Approx. size | Used by |
| --- | --- | --- | --- |
| `ci-tiny` | Qwen3-0.6B, MLX affine 4-bit g64 | ~0.5 GiB | PR jobs that need a live engine (CLI matrix end-to-end rows, smoke dialect checks) |
| `ci-small` | Qwen3-8B, MLX affine 4-bit g64 | ~4.2 GiB | Nightly numerics, dialect conformance, chaos, soak |
| `ci-moe` | Qwen3-30B-A3B, MLX affine 4-bit g64 | ~17 GiB | Fleet-only: perf gate, agent-concurrency targets (PB12–PB13 table) |
| `ci-large` | Llama-3.3-70B, 4-bit | ~37 GiB | Fleet-only (64 GiB+ machines): perf gate rows, 70B conversion test (MP AC4) |
| `ci-gguf` | `ci-tiny`-equivalent GGUF (Q4_K_M) | ~0.5 GiB | Backend-B parity rows (feature `drakkar-gguf`) |
| `ci-native-fp4` | gpt-oss-20b, MXFP4 | ~12 GiB | Fleet-only: MXFP4 pass-through and FE7 anchor verification |

- TS6. `ci-tiny` and `ci-gguf` MUST fit and run on hosted arm64 macOS runners (8–16 GiB
  RAM). Everything larger runs only on fleet nodes (§10.3).

### 2.3 Named corpora

Corpora are versioned directories under `tests/corpora/`, changed only by PR, each with a
`MANIFEST.json` (content hashes, provenance, generation instructions):

- **structured-output corpus** — ≥ 200 JSON Schemas spanning depth, recursion, string
  patterns, enums, unions, and numeric bounds, each with a prompt. Consumed by RFC-0003
  AC3 and RFC-0007 AC3 tests. Validity is checked by an independent JSON Schema validator,
  not by the grammar engine that produced the mask (no self-grading).
- **agent-trace corpus** — recorded multi-turn tool-call sessions (coding-agent style:
  repeated scaffold, file snippets, tool results) used for speculation (IC18, RFC-0003
  AC4) and prefix-cache workloads. Sanitized of any user content.
- **template/tool-call corpus** — per-model-family chat template renderings, tool
  declarations, and expected token boundaries; drives KV cache-key correctness (RFC-0005
  AC1) and template-override tests (MP18).
- **dialect trace cassettes** — recorded request/response event streams from reference
  clients (§5A), one directory per client per endpoint.
- **fit fixture set** — FE7 anchors, RFC-0004 §9 worked examples, and hand-computed
  KV curves (§5.1, §5.2).

- TS7. Every corpus change MUST state in the PR description which acceptance criteria the
  change affects; corpus shrinkage (removing cases) requires a reviewed justification.

## 3. Layer 1 — Unit tests (`cargo test` per crate)

- TS8. Every workspace crate (LD24: `drakkar-cli`, `drakkar-server`, `drakkar-sched`,
  `drakkar-fit`, `drakkar-models`, `drakkar-engine`, `drakkar-grammar`, `drakkar-core`,
  `drakkar-mlx-sys`/`drakkar-mlx`, `drakkar-gguf`) MUST carry unit tests for its public
  surface, runnable in isolation with `cargo nextest run -p <crate>` and no model weights,
  no GPU, no network. GPU-dependent code paths are behind trait fakes at the
  `InferenceBackend` seam (RFC-0001 A6/I5 ([Architecture](../rfcs/RFC-0001-architecture.md#proposed-design))).

Representative required suites (non-exhaustive; the matrix in §11 binds them to ACs):

| Crate | Must-cover units |
| --- | --- |
| `drakkar-core` | Error taxonomy completeness (every `ER*` variant constructs, renders, and carries a remedy, RFC-0011), config precedence flags > env > file > defaults (LD23), schema version constants |
| `drakkar-fit` | FE5 bpw arithmetic, FE8–FE11 per-architecture KV formulas, FE16 os_floor selection, FE19 verdict boundaries (0.85 edge exactly), FE20 context solver monotonicity |
| `drakkar-models` | MP1 reference-form parsing (URL/`org/repo@rev`/alias/local path), MP6 pickle rejection, safetensors/GGUF header bounds checks (MP8), manifest/blob GC (MP12) |
| `drakkar-sched` | Chunk budget adaptation bounds (AS13, 256–2048), priority queue FIFO-within-priority (AS15), admission arithmetic delegation (FE18) |
| `drakkar-server` | Dialect normalization to `GenerationRequest` and back (AS1), unsupported-field 400s (AS2), SSE framing incl. heartbeat timer (AS4) |
| `drakkar-grammar` | Schema → mask compilation on the structured-output corpus heads, `json_object` permissive grammar (AS10) |
| `drakkar-cli` | Exit-code mapping (CLI8), `--json`/human render from the same struct (RFC-0008 §1), NO_COLOR/TTY detection (CLI9) |
| `drakkar-engine` | Actor message loop ordering, bounded-channel backpressure (A3), keep-alive unload timers (AS17) |

## 4. Layer 2 — Property tests (proptest)

- TS9. Property suites live beside their crate and run in two profiles: `quick`
  (256 cases per property, PR gate) and `extended` (65,536 cases, nightly). Failing seeds
  are persisted in-repo (`proptest-regressions/`) so every shrunk counterexample becomes a
  permanent regression test.

Required properties:

- TS10. **Cache-key invalidation fuzz** (KV12; discharges RFC-0005 AC5
  ([KV Cache](../rfcs/RFC-0005-kv-cache.md#testing-strategy))). Generate a prefix and a
  mutation over any of `(model_revision, tokenizer_hash, chat_template_hash, token_ids,
  kv_precision, rope_scaling)`; assert the radix index never returns a cached run whose
  key component differs — zero stale hits across all generated mutations, including
  single-token and single-bit changes at block boundaries.
- TS11. **Block-table / CoW invariants** (KV1–KV4, I2). Drive the pool with arbitrary
  interleavings of `admit / append / fork(shared prefix) / seal(donate) / evict /
  disconnect` and assert after every step: (a) `free + active + cached == pool_total`
  (accounting equality); (b) refcounts equal the number of referencing sequences;
  (c) after a CoW split, the writer's block is not aliased by any other sequence;
  (d) no operation ever allocates beyond the pool (growth impossible by construction,
  KV2); (e) a fully quiesced pool returns to `free == pool_total` (this is the same
  invariant the disconnect-storm chaos test checks at system level, RFC-0007 AC4).
- TS12. **Exit-code mapping totality** (CLI8, RFC-0011). For every constructible error
  variant in the `drakkar-core` taxonomy: exactly one exit code in {2,3,4,5,6,7} is
  produced, a remedy template exists and renders non-empty (CLI15), and the `--json` error
  object validates against its schema. The property is totality over the error enum, so
  adding a variant without a mapping fails the suite at compile-or-test time.
- TS13. **Tokenizer roundtrips** (MP17). For arbitrary Unicode strings (including
  combining marks, code points requiring surrogate pairs in UTF-16-derived tokenizers,
  and byte-fallback ranges):
  `decode(encode(s))` preserves content per the tokenizer's documented normalization; and
  the *incremental* detokenizer (streaming path, IC17) emits the identical byte sequence
  as one-shot decode for every prefix split point — no torn UTF-8 across streamed deltas.
- TS14. **Fit-engine algebraic properties** (FE8–FE20). `kv_bytes(ctx)` is monotonic
  non-decreasing in `ctx` for every architecture class; hybrid-SWA cost is ≤ the uniform
  cost for the same shape (FE9); `ctx_max` is the inverse of the same formula
  (solve-then-evaluate round-trips within one block); verdict tiers partition the input
  space with no gaps or overlaps (FE19).

## 5. Layer 3 — Golden fixtures and snapshots

Golden tests pin numbers and shapes that the product publicly defends.

### 5.1 Fit-engine anchors (FE7; discharges RFC-0004 AC1)

- TS15. The fit engine, run offline with `--machine` hardware profiles, MUST reproduce the
  published MLX footprints within 7%:

| Fixture | Anchor | Tolerance |
| --- | --- | --- |
| Qwen3-8B, MLX 4-bit | 5.61 GB total-inference | ±7% |
| Qwen3-30B-A3B, MLX 4-bit | 17.31 GB | ±7% |
| gpt-oss-20b, MXFP4, 4k context | 12.08 GB | ±7% |

These run on every PR (pure arithmetic, no GPU). The tolerance is the product metric M2
([PRD §7](../../PRD.md#7-success-metrics)); tightening it is an RFC-0009 calibration
outcome, never a test-side edit.

### 5.2 Worked examples and KV curves (discharges RFC-0004 AC2)

- TS16. The four worked examples of RFC-0004 §9
  ([Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)) — M2/16 GiB
  + Qwen3-8B, M5/24 GiB + Qwen3-30B-A3B, M4 Pro/48 GiB + Llama-3.3-70B (wired-raise
  remedy path), M5 Max/128 GiB + gpt-oss-120b — are executable golden tests via
  `drakkar fit --machine <profile> --json`: verdict, remedy ranking, and context ceilings
  are asserted exactly; memory figures within the FE24 modeled tolerance.
- TS17. Hand-computed `kv_bytes(ctx)` curves for one uniform-GQA model (Llama-3.1-8B,
  128 KiB/token fp16), one hybrid-SWA model (Gemma-class interleave, FE9), one MLA model
  (DeepSeek-lineage 576-element latent, FE10), and one SSM-hybrid (constant-state term,
  FE11) are stored as fixtures at ctx ∈ {1k, 4k, 8k, 32k, 128k} and asserted exactly
  (the formulas are integer arithmetic; there is no tolerance).

### 5.3 JSON schema snapshots

- TS18. Every versioned schema surface — `drakkar.<cmd>/1` CLI outputs (CLI6),
  `drakkar.fit/1` (FE26), `/v1/*` response bodies and `x_drakkar` extensions (AS6, AS20),
  `--stream-json` event lines (CLI7) — is snapshot-tested with `insta`. The snapshot
  differ enforces the additive-only rule: within a schema major version, a snapshot diff
  that removes or retypes a field fails CI; adding optional fields passes with a
  reviewed snapshot update.

### 5.4 Error envelope snapshots

- TS19. For each failure class in RFC-0011, snapshots pin: the OpenAI-dialect error body,
  the Anthropic-dialect error body (AS8 envelopes, including `max_admissible_tokens` on
  413 and `retry_after_ms` on 429), the CLI human rendering (what failed / why / next
  action, CLI15), and the CLI `--json` error object. Snapshots exist per dialect per
  failure class; a new failure class without all four snapshots fails the
  `traceability-check` job.

## 5A. Layer 4 — Dialect conformance via recorded traces

- TS20. Dialect fidelity (AS1; discharges RFC-0007 AC1
  ([API Server](../rfcs/RFC-0007-api-server.md#testing-strategy))) is tested by replaying
  recorded traces from four reference clients against `drakkar serve` on localhost:
  the official `openai` Python SDK, the official `anthropic` Python SDK, a
  Claude-Code-class coding-agent client, and OpenCode. Each cassette captures the client's
  requests and the expected response *shape* (event ordering, field presence, terminal
  frames, usage accounting), not model text.
- TS21. Replay has two modes. **Shape mode** (nightly, `ci-small`): the live server
  responds with real generations; assertions cover envelope structure — SSE event
  sequence (`chat.completion.chunk` + final `usage` frame; Anthropic
  `message_start/content_block_delta/message_delta/message_stop`), tool-call assembly,
  `finish_reason`/`stop_reason` values, cached-token accounting fields, and heartbeat
  timing (AS4–AS6). **Strict mode** (PR, `ci-tiny`, greedy): a pinned prompt set where
  full frames are compared against recorded output for the pinned model hash.
- TS22. SDK versions are pinned exactly per DRAKKAR release; bumping an SDK pin re-records
  cassettes in a dedicated PR so dialect drift in upstream clients is an explicit,
  reviewed event, never silent breakage.
- TS23. Negative conformance: for every dialect field DRAKKAR does not support, a trace
  asserts the loud `400` naming the field (AS2); silent acceptance is a test failure.

## 6. Layer 5 — CLI integration matrix

- TS24. The full command surface (RFC-0008 §2) is exercised as a matrix of
  *(command × outcome class)*, discharging RFC-0008 AC1–AC5
  ([CLI](../rfcs/RFC-0008-cli-ux.md#testing-strategy)). Rows spawn the real `drakkar`
  binary via `assert_cmd` against `hub-sim` and a temp store; every row asserts all
  three of: exit code (CLI8), stdout `--json` schema validity (CLI6: nothing but the JSON
  object on stdout), and the human rendering including the remedy line on failures
  (CLI15).

| Command | Success | Usage (2) | Not found (3) | Won't fit (4) | Network (5) | Engine (6) | Disk (7) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `run` | ✔ | ✔ | ✔ | ✔ (`--force` variant too) | ✔ | ✔ | ✔ |
| `serve` | ✔ | ✔ | ✔ | ✔ | ✔ | ✔ | ✔ |
| `fit` | ✔ | ✔ | ✔ | n/a (fit reports, exit 0) | ✔ | n/a | n/a |
| `pull` | ✔ | ✔ | ✔ | ✔ | ✔ | n/a | ✔ |
| `ls` / `ps` | ✔ | ✔ | n/a | n/a | n/a | ✔ (`ps`, engine gone) | n/a |
| `rm` / `prune` | ✔ | ✔ | ✔ | n/a | n/a | n/a | ✔ |
| `convert` | ✔ | ✔ | ✔ | ✔ | n/a | ✔ | ✔ |
| `bench` | ✔ | ✔ | ✔ | ✔ | n/a | ✔ | n/a |
| `cache ls/clear` | ✔ | ✔ | n/a | n/a | n/a | n/a | ✔ |
| `doctor` | ✔ | ✔ | n/a | n/a | ✔ (`--check-update` offline) | n/a | n/a |
| `config get/set/path` | ✔ | ✔ (type/range reject, CLI11) | n/a | n/a | n/a | n/a | ✔ (read-only config dir) |
| `completions` | ✔ | ✔ | n/a | n/a | n/a | n/a | n/a |

  "n/a" cells are asserted absent: the command MUST NOT be able to produce that class.
- TS25. Environment rows run for every command: `NO_COLOR=1`, non-TTY stdout, `--quiet`,
  `--verbose`, and the pipe test `drakkar fit <ref> --json | jq .verdict` (RFC-0008 AC2).
  First-run orientation prints once and only once (CLI14; asserted by running twice
  against a fresh store).
- TS26. Resolution end-to-end rows (discharges RFC-0006 AC1
  ([Model Pipeline](../rfcs/RFC-0006-model-pipeline.md#testing-strategy))): each of —
  full HF URL to an mlx-community 4-bit repo, bare `org/repo` bf16 source
  (auto-quantized), a GGUF repo URL, a local `.gguf` file, a shipped alias, a
  user-defined alias shadowing a shipped one (LD16: user wins with a warning) — resolves
  and reaches a running REPL with zero flags, report card shown first (CLI1, RFC-0008
  AC3), against `hub-sim` with `ci-tiny`-scale artifacts. HF-cache interop (MP11,
  RFC-0006 AC3): a pre-seeded hub cache yields zero network bytes for weights and
  clone-level disk cost, asserted via `hub-sim` byte counters and APFS clone stats.
- TS27. Panic containment: a debug-only `--crash-test` hook induces a panic past the CLI
  top level; the process MUST exit 6 with the bug-report hint and no raw backtrace unless
  `--verbose` (RFC-0008 AC5).

## 7. Layer 6 — FFI safety suite

The Rust↔C++ boundary (`dk_*` ABI, RFC-0002 D2, RFC-0010
([Backend ABI](../rfcs/RFC-0010-backend-abi.md#testing-strategy))) is the one place memory
safety is manual. This layer exists so that no ABI change merges without sanitizer
evidence.

- TS28. **Sanitizer builds.** The C++ shim's unit tests and the Rust `drakkar-mlx-sys`
  integration tests build and run under ASan+UBSan (shim: clang `-fsanitize=address,undefined`;
  Rust side: pinned nightly `-Z sanitizer=address` for the sys-crate tests). Any
  sanitizer report is a hard failure. PR runs the smoke subset (array lifecycle, load/
  unload, one prefill+decode on `ci-tiny`); nightly runs the full suite.
- TS29. **Leak counters on array-lifecycle fuzz.** The debug ABI exposes allocation
  counters (`dk_debug_live_arrays()`, `dk_debug_live_handles()`, per RFC-0010). A
  libFuzzer target drives arbitrary valid sequences of array create/retain/release/
  view/eval calls; after every quiesce point both counters MUST return to their
  baseline. The same counters are asserted zero after every integration test that loads
  and unloads a model.
- TS30. **Panic-at-boundary tests.** Rust panics MUST NOT unwind across the C ABI and C++
  exceptions MUST NOT unwind into Rust (RFC-0010). Tests (isolated per-process under
  nextest): (a) force a panic inside each Rust callback invoked from the shim — the
  process aborts with the documented diagnostic rather than exhibiting UB; (b) force a
  C++ exception inside a `dk_*` entry point — it is caught at the boundary and surfaced
  as the documented error status; (c) every `dk_*` function called with null/invalid
  handles returns the documented error status, never crashes.
- TS31. **Untrusted-input fuzzers** (MP8, A11). libFuzzer targets for the safetensors
  header parser, GGUF metadata parser, and manifest JSON: bounded allocations from
  untrusted lengths, no panics, structured errors only. Corpus seeded from real files
  plus the crash corpus accumulated in-repo. Nightly budget: ≥ 4 CPU-hours per target;
  new crash inputs are minimized and committed as regression cases.

## 8. Layer 7 — ML-numerics tests

Numerics bugs do not crash; they quietly degrade quality. This layer pins the compute
path against independent references.

- TS32. **Logit parity vs the mlx-lm reference** (supports RFC-0002 D3's "executable
  reference" and RFC-0003 AC1's parity stance). For every natively implemented
  architecture (launch set: Llama-family, Qwen3/3.5 dense + MoE, Gemma-family hybrid,
  gpt-oss, Mistral-family, DeepSeek-lineage MLA), a fixture stores reference outputs
  generated once from mlx-lm at pinned versions (model hash, MLX pin, prompt set, seed)
  and committed with a provenance manifest. Assertions per architecture, on `ci-small`
  or the smallest available family member:
  (a) greedy decode is token-identical to the reference for ≥ 128 steps on 8 fixed
  prompts;
  (b) prefill-step logits satisfy mean |Δ| ≤ 2e-2 and max |Δ| ≤ 1e-1 in bf16 compute
  (initial bounds; tightening or per-family adjustment is a named RFC-0009 calibration
  work item, and any loosening requires an RFC);
  (c) parity holds with chunked prefill on vs off (IC12 must not change results).
- TS33. **Quantizer/estimator agreement** (IC8, MP13; discharges RFC-0006 AC4). On-device
  conversion output bpw matches the fit-engine estimate within 1% for the recipe matrix
  (4/8-bit × g32/g64, embeddings/lm_head recipe bits, sensitive-layer exemptions per
  IC9/FE6). The 70B bf16→4-bit conversion runs on a 48 GiB fleet node inside the IC8
  streaming memory envelope (peak ≈ one shard above output size), with peak RSS asserted
  against the envelope — nightly-fleet, not PR.
- TS34. **Cache-on/cache-off equivalence** (discharges RFC-0005 AC1). Greedy generations
  are byte-identical with the prefix cache enabled and disabled across the
  template/tool-call corpus, including partial-block boundary reuse (KV10 tail
  recompute), CoW fork points, and post-restore disk-tier hits. Runs nightly on
  `ci-small`; a `ci-tiny` smoke row runs on PR.
- TS35. **KV quantization quality guard** (KV13–KV15). At `--kv-bits 8` and `4`, perplexity
  on a fixed 64k-token evaluation text stays within the recorded per-model envelope
  (fixture-pinned at first fleet measurement); the final-layer fp16 exemption (KV15) is
  asserted active on deep models by inspecting `memory_report()` layout metadata.
- TS36. **Sampling correctness.** Counter-based RNG reproducibility: same seed + same
  batch schedule → identical samples (LD6); statistical tests (chi-squared against the
  softmax distribution at temperature 1, 1e6 draws on a fixed logit vector) for the
  sampler; top-k/top-p/min-p truncation asserted against a scalar reference
  implementation in Rust.

## 9. Layers 8–10 — Perf gate, soak, chaos

### 9.0 Layer 8 — Performance regression gate (PB16)

- TS37. Every release candidate runs workloads A–E (PB9) on the Tier-1 fleet (PB11: M4,
  M4 Pro, M4 Max, M5, M5 Max) with `drakkar bench` itself (the shipped harness is the CI
  harness; PB8 median + IQR, never single runs). Gate rules, verbatim from RFC-0009 PB16
  ([Performance](../rfcs/RFC-0009-performance.md#proposed-design)):
  - any metric regressing > 3% versus the previous release blocks the release unless an
    explicit waiver is filed (§12);
  - memory-contract breaches (PB4, invariant I2) are **non-waivable**;
  - NAX tensor-op self-test failures (PB14, IC26) are **non-waivable** — the M5 prefill
    multiple must be demonstrated, not assumed.
- TS38. **Gate self-test** (discharges RFC-0009 AC4). The gate pipeline itself is tested:
  a build with a seeded 5% decode slowdown and a build with NAX force-disabled MUST both
  be blocked. The self-test runs on every change to the bench harness or gate code, and
  before every release-gate execution.
- TS39. Cross-engine baselines (PB17) — mlx-lm (decode parity within 5%, RFC-0003 AC1),
  llama.cpp Metal, and peer servers at equal model/quant/machine — are measured in the
  same run and *published, not gated*, each number carrying the LD18 reproducibility
  manifest (machine, macOS build, MLX pin, model hashes).
- TS40. Bench self-reproducibility (RFC-0009 AC1): three back-to-back runs of workload A
  on the same fleet node keep the median within the reported IQR; violation fails the
  gate run as an infrastructure error (results discarded, not published).

### 9.0.1 Layer 9 — Soak (P14)

- TS41. **24-hour mixed-load soak**, release-blocking, on at least M4 Pro and M4 Max
  fleet nodes with `ci-small` and `ci-moe`: a generator interleaves workloads A, C, and D
  plus a 1% stream of structured-output requests and periodic keep-alive
  unload/reload cycles (AS17). Pass criteria, all mandatory:
  - RSS drift < 2% after a 30-minute warmup window (P14, RFC-0003 AC5);
  - zero request failures and zero engine-thread panics;
  - `memory_report()` sampled every 60 s never exceeds the declared contract
    (I2; discharges RFC-0004 AC4 — no plan emitted by the fit engine, when executed,
    breaches the contract);
  - KV pool accounting equality holds at every sample (TS11 invariant, system level);
  - SSE clients receive heartbeats within the AS4 bound throughout.
- TS42. A 4-hour variant of the same soak runs nightly so leaks surface within a day of
  introduction rather than at release week.

### 9.1 Layer 10 — Chaos and fault injection

Faults are injected through `hub-sim` (network faults: 5xx, timeouts, mid-stream resets,
truncated bodies), a filesystem shim for the store path (ENOSPC, EIO on demand), and
process signals. Each scenario asserts the documented failure response
(RFC-0011 taxonomy) — never a crash, never silent corruption:

| Scenario | Injection | Required outcome | Discharges |
| --- | --- | --- | --- |
| Kill during download | `SIGKILL` at 60% of a multi-file pull; rerun | Completes; zero completed files re-downloaded (byte counters in `hub-sim`) | MP AC2 (RFC-0006 AC2) |
| Kill during serve, warm KV | `SIGKILL` with the 8k-scaffold fixture cached to the SSD tier; restart; re-issue | Disk-tier restore ≥ 3x faster than cold prefill (KV18 cost model) | KV AC4 (RFC-0005 AC4) |
| Disconnect storm | 100 mid-stream client disconnects at random decode steps | Each sequence cancelled within one decode step (AS4); pool accounting equality after quiesce — zero leaked blocks | AS AC4 (RFC-0007 AC4) |
| Disk full, preflight | Store volume below `required` before pull | Refusal before any bytes move, exact shortfall stated, exit 7 | MP9 |
| Disk full, mid-download | ENOSPC injected mid-write | Exit 7 with remedy; store left consistent (partials resumable, no orphaned blobs after `prune`) | MP9, MP10 |
| Kill during conversion | `SIGTERM`/`SIGKILL` mid-`convert` | No partial artifact registered (temp + rename atomicity); rerun succeeds | MP14 |
| Corrupt blob | Bit-flip in a stored shard | Integrity verification fails with the named error and re-fetch remedy; blob never loaded | MP8 |
| Hub degradation | 5xx bursts, stalls, truncated metadata | Bounded retries then exit 5 with remedy; no partial manifest registered | MP7, MP8 |
| Stale KV sidecar | Truncated/corrupted disk-tier index | Restore skipped, cold prefill proceeds, sidecar quarantined; no crash, no stale hit | KV17, KV12 |
| Gated repo, no token | `hub-sim` 403 with gate metadata | Named error with the acceptance URL, exit 3 | MP2 |

- TS43. Chaos scenarios run nightly with randomized injection points (seed logged and
  replayable); the fixed-point variants listed above additionally run as release gates.

## 10. CI pipeline: named jobs and cadence

### 10.1 Job registry

- TS44. The following job names are normative (referenced by branch protection and the
  release checklist in RFC-0012
  ([Release Engineering](../rfcs/RFC-0012-release-engineering.md#proposed-design))):

| Job | Layer(s) | PR | Nightly | Release | Runner class |
| --- | --- | --- | --- | --- | --- |
| `test-unit` | §3 | ✔ gate | ✔ | ✔ | hosted arm64 |
| `test-property-quick` | §4 (256 cases) | ✔ gate | — | — | hosted arm64 |
| `test-property-extended` | §4 (65,536 cases) | — | ✔ | ✔ | hosted arm64 |
| `test-golden` | §5 (anchors, worked examples, KV curves, snapshots) | ✔ gate | ✔ | ✔ | hosted arm64 |
| `test-cli-matrix` | §6 (with `ci-tiny` + `hub-sim`) | ✔ gate | ✔ | ✔ | hosted arm64 |
| `test-ffi-sanitizers` | §7 smoke (PR) / full (nightly) | ✔ gate | ✔ | ✔ | hosted arm64 |
| `fuzz-parsers` | §7 TS31 (4 h/target) | — | ✔ | ✔ | hosted arm64 |
| `dialect-strict` | §5A strict mode (`ci-tiny`) | ✔ gate | ✔ | ✔ | hosted arm64 |
| `dialect-conformance` | §5A shape mode (`ci-small`, 4 clients) | — | ✔ | ✔ gate | fleet |
| `numerics-parity` | §8 TS32/TS34/TS36 | smoke (`ci-tiny`) | ✔ full | ✔ gate | fleet |
| `numerics-convert-70b` | §8 TS33 | — | weekly | ✔ gate | fleet 48 GiB+ |
| `chaos` | §9.1 | — | ✔ randomized | ✔ fixed-point gate | fleet |
| `soak-4h` | §9.0.1 TS42 | — | ✔ | — | fleet |
| `soak-24h` | §9.0.1 TS41 | — | — | ✔ gate | fleet |
| `perf-gate` | §9.0 TS37–TS40 | — | trend run | ✔ gate | Tier-1 fleet |
| `traceability-check` | TS1, TS19 | ✔ gate | ✔ | ✔ | hosted |

- TS45. "✔ gate" on PR means required by branch protection; a PR MUST NOT merge with any
  gating job red or any expired quarantine entry (TS3). Release means the RFC-0012
  release pipeline; all release-column jobs are release-blocking except where PB17
  explicitly publishes without gating.

### 10.2 Cadence rules

- TS46. PR jobs MUST complete in ≤ 20 minutes wall-clock combined (hosted runners,
  `ci-tiny` only, hermetic per TS2). Anything slower moves to nightly; speed budgets are
  enforced so contributors never route around CI.
- TS47. Nightly runs on `main` publish a trend dashboard (perf trend run, extended
  properties, fuzz findings, soak-4h RSS curves). A nightly failure opens a tracked issue
  automatically assigned to the area owner (subsystem → area map in the corpus overview);
  two consecutive red nightlies freeze merges to the offending area until green.
- TS48. Release candidacy (RFC-0012) requires: all PR gates green on the release commit,
  the last nightly fully green, plus the release-only gates (`perf-gate`, `soak-24h`,
  `dialect-conformance`, `numerics-parity`, `numerics-convert-70b`, fixed-point `chaos`).

### 10.3 Runner classes

- TS49. **Hosted arm64**: managed macOS arm64 runners (M-series, 8–16 GiB), macOS pinned
  to the release-supported baseline (macOS 15) plus one runner on macOS 26.2+ for
  NAX-detection unit paths. **Fleet**: physically owned Tier-1 machines (PB11) attached
  as self-hosted runners; v0.2 requires owned M4 Pro and M4 Max online (RFC-0009 AC2),
  M5/M5 Max join as acquired, full fleet CI is a v1.0 "Harbor" deliverable
  ([PRD §8](../../PRD.md#8-roadmap)). Fleet jobs are serialized per machine (perf runs
  own the machine exclusively; no co-located jobs while `perf-gate` or soak runs).

## 11. Acceptance-criteria traceability matrix (normative, TS1)

| Criterion | Requirement | Test layer(s) | Gating job(s) |
| --- | --- | --- | --- |
| RFC-0003 AC1 | Decode within 5% of mlx-lm | §9.0 cross-engine (TS39) | `perf-gate` (published; parity < 5% asserted) |
| RFC-0003 AC2 | M5 prefill ≥ 3x M4-class (NAX) | §9.0 (TS37, non-waivable PB14) | `perf-gate` |
| RFC-0003 AC3 | 100% schema-valid JSON, ≤ 8% ITL overhead | §5A + §9.0 workload E | `dialect-strict`, `perf-gate` |
| RFC-0003 AC4 | n-gram speculation ≥ 1.2x, no workload > 3% worse | §9.0 agent-trace corpus | `perf-gate` |
| RFC-0003 AC5 | 24 h soak, zero panics, RSS < 2% | §9.0.1 (TS41) | `soak-24h` |
| RFC-0004 AC1 | FE7 anchors within 7% | §5.1 (TS15) | `test-golden` |
| RFC-0004 AC2 | Architecture-aware KV curves | §5.2 (TS17) + §4 (TS14) | `test-golden`, `test-property-quick` |
| RFC-0004 AC3 | Post-calibration accuracy (FE24) | §9.0 calibration run | `perf-gate` |
| RFC-0004 AC4 | No emitted plan breaches I2 | §9.0.1 (TS41 contract sampling) | `soak-24h` |
| RFC-0005 AC1 | Byte-identical greedy, cache on/off | §8 (TS34) | `numerics-parity` |
| RFC-0005 AC2 | Warm TTFT ≤ 1.15× bound | §9.0 workload C warm rows | `perf-gate` |
| RFC-0005 AC3 | Fan-out allocates prefix once; ITL targets | §4 (TS11) + §9.0 workload C | `test-property-quick`, `perf-gate` |
| RFC-0005 AC4 | Kill -9 → disk restore ≥ 3x | §9.1 (TS43) | `chaos` (release fixed-point) |
| RFC-0005 AC5 | Fuzzed invalidation, zero stale hits | §4 (TS10) | `test-property-quick`/`-extended` |
| RFC-0006 AC1 | Five reference forms run flag-free | §6 (TS26) | `test-cli-matrix` |
| RFC-0006 AC2 | Kill at 60% download, clean resume | §9.1 (TS43) | `chaos` |
| RFC-0006 AC3 | HF-cache interop, zero weight bytes | §6 (TS26) | `test-cli-matrix` |
| RFC-0006 AC4 | 70B convert on 48 GiB; bpw within 1% | §8 (TS33) | `numerics-convert-70b` |
| RFC-0006 AC5 | Pickle rejected with documented error | §3 + §6 | `test-unit`, `test-cli-matrix` |
| RFC-0007 AC1 | Four client trace suites pass unmodified | §5A (TS20–TS23) | `dialect-strict`, `dialect-conformance` |
| RFC-0007 AC2 | ITL guard ≤ 25% inflation under 32k prompt | §9.0 workload C | `perf-gate` |
| RFC-0007 AC3 | 0 schema violations at concurrency 8 | §5A + §9.0 workload E | `dialect-conformance`, `perf-gate` |
| RFC-0007 AC4 | 100 disconnects, zero leaked blocks | §9.1 (TS43) + §4 (TS11) | `chaos`, `test-property-quick` |
| RFC-0007 AC5 | 4 agent streams on M4 Max: ITL < 45 ms, warm TTFT < 500 ms | §9.0 workload C | `perf-gate` |
| RFC-0008 AC1 | Command × outcome matrix, schemas + exit codes | §6 (TS24) | `test-cli-matrix` |
| RFC-0008 AC2 | `--json \| jq` pipe cleanliness | §6 (TS25) | `test-cli-matrix` |
| RFC-0008 AC3 | Cold run to REPL, report card first, `--yes` | §6 (TS26) | `test-cli-matrix` |
| RFC-0008 AC4 | NO_COLOR / non-TTY behavior | §6 (TS25) | `test-cli-matrix` |
| RFC-0008 AC5 | Remedy on every error; no raw panic | §6 (TS27) + §4 (TS12) | `test-cli-matrix`, `test-property-quick` |
| RFC-0009 AC1 | Bench median stable within IQR ×3 runs | §9.0 (TS40) | `perf-gate` |
| RFC-0009 AC2 | Tier-1 targets met (owned fleet at v0.2) | §9.0 (TS37) | `perf-gate` |
| RFC-0009 AC3 | Calibration reaches FE24 post-cal bounds | §9.0 calibration run | `perf-gate` |
| RFC-0009 AC4 | Gate blocks seeded 5% regression + NAX-disable | §9.0 (TS38) | `perf-gate` self-test |

RFC-0010 (`AB*`), RFC-0011 (`ER*`), and RFC-0012 (`RE*`) criteria bind to §7
(`test-ffi-sanitizers`, `fuzz-parsers`), §5.4/§4-TS12 (`test-golden`,
`test-property-quick`), and §10 (`traceability-check`, release pipeline) respectively;
their rows are appended to this matrix in the same PR that lands each criterion (enforced
by `traceability-check`).

## 12. Waiver policy

- TS50. A perf-gate waiver (the only waivable gate) is a reviewed file in-repo
  (`ci/waivers/<release>.toml`) naming: the metric, the regression magnitude, the cause,
  the recovery issue, and an expiry (one release). Waivers for memory-contract breaches
  (PB4) or NAX self-test failures (PB14) are rejected by the gate tooling itself — the
  non-waivable list is code, not convention. All waivers ship in the release notes
  (honest speed applies to regressions too).

## 13. Milestone phasing

| Milestone | Layers active | Notes |
| --- | --- | --- |
| v0.1 "First light" | §3, §4, §5, §6 (v0.1 command set), §7, §5A strict mode (OpenAI chat only) | `hub-sim`, `ci-tiny`, hosted runners only; perf numbers published as `est.`/measured-on-dev-machine, no gate yet |
| v0.2 "Convoy" | + §8 full, §9.0 on owned M4 Pro/M4 Max, §9.0.1 (4 h nightly + 24 h release), §9.1, §5A all four clients | Gate baselines locked from first fleet measurements (PB16); KV block-size 16-vs-32 ablation executes here (LD7) |
| v0.3 "Fleet" | + multi-model pool rows (per-engine contracts, A5/LD12), SSD-tier chaos matrix complete, embeddings + `/v1/responses` (flagged, LD5) dialect rows | |
| v1.0 "Harbor" | + full Tier-1 fleet CI (M5, M5 Max online), desktop C-ABI consumer smoke suite, signed-artifact verification in release pipeline (RFC-0012) | `est.` targets for M5-family confirmed or revised to measured (RFC-0009 AC2) |

## 14. References

- [PRD](../../PRD.md) §5.2 (P10–P14), §7 (M2–M4, M6)
- [RFC-0003: Inference Core](../rfcs/RFC-0003-inference-core.md#testing-strategy) — AC1–AC5, IC12, IC16, IC26
- [RFC-0004: Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#testing-strategy) — AC1–AC4, FE7, FE24, §9 worked examples
- [RFC-0005: KV Cache](../rfcs/RFC-0005-kv-cache.md#testing-strategy) — AC1–AC5, KV12
- [RFC-0006: Model Pipeline](../rfcs/RFC-0006-model-pipeline.md#testing-strategy) — AC1–AC5, MP8–MP9, MP14
- [RFC-0007: API Server](../rfcs/RFC-0007-api-server.md#testing-strategy) — AC1–AC5, AS1–AS8
- [RFC-0008: CLI and UX](../rfcs/RFC-0008-cli-ux.md#testing-strategy) — AC1–AC5, CLI6–CLI9, CLI15
- [RFC-0009: Performance](../rfcs/RFC-0009-performance.md#proposed-design) — PB1–PB17, AC1–AC4
- [RFC-0010: Backend ABI](../rfcs/RFC-0010-backend-abi.md#testing-strategy) — sanitizer and boundary requirements consumed by §7
- [RFC-0011: Error Taxonomy](../rfcs/RFC-0011-error-taxonomy.md#testing-strategy) — envelope snapshots and exit-code totality (§5.4, TS12)
- [RFC-0012: Release Engineering](../rfcs/RFC-0012-release-engineering.md#proposed-design) — release pipeline consuming §10 job registry
