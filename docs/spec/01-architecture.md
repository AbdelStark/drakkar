# 01 — System Architecture

- Status: Normative
- Sources: [RFC-0001](../rfcs/RFC-0001-architecture.md) (all), [RFC-0002](../rfcs/RFC-0002-stack-selection.md) D1–D6
- Applies to: all milestones (v0.1 "First light" through v1.0 "Harbor")

This document is the architectural contract for DRAKKAR. It fixes the system decomposition,
the crate map and its dependency direction rules, the process and threading model, the
request lifecycle, the on-disk state layout, and the five invariants ([I1–I5](#10-invariants-the-review-contract))
that every pull request is reviewed against. Where this document and an RFC disagree, this
document wins and the RFC MUST be amended. Requirement IDs (A1–A12, I1–I5) are carried
verbatim from [RFC-0001](../rfcs/RFC-0001-architecture.md#proposed-design); D1–D6 from
[RFC-0002](../rfcs/RFC-0002-stack-selection.md#proposed-design).

## 1. Architectural stance

DRAKKAR is a Rust control plane wrapping a native compute core
([RFC-0001 Summary](../rfcs/RFC-0001-architecture.md#summary)). The control plane owns
everything users touch: CLI, HTTP server, scheduler, model management, and the feasibility
engine. The compute core owns everything the GPU touches: model graphs, Metal kernels, KV
storage, sampling. The two meet at a narrow, versioned internal ABI (the backend seam,
[§6](#6-the-backend-seam); wire-level detail in
[RFC-0010](../rfcs/RFC-0010-backend-abi.md)).

Six design principles from [RFC-0001](../rfcs/RFC-0001-architecture.md#proposed-design)
govern every design decision in this corpus:

1. **Honest speed.** Every number surfaced to a user traces to a formula
   ([RFC-0004](../rfcs/RFC-0004-feasibility-engine.md)) or a benchmark
   ([RFC-0009](../rfcs/RFC-0009-performance.md)). Never asserted.
2. **The GPU budget is a contract.** The engine declares a memory budget at model load and
   never exceeds it. Admission control fails requests early and legibly; Metal MUST never be
   the component that discovers we are out of memory.
3. **One binary, zero ceremony.** No Python, no venvs, no containers, no post-install steps
   ([PRD](../../PRD.md#4-goals-and-non-goals) G1, G6).
4. **Agent-native.** Every interface is designed to be driven by a program as comfortably as
   by a human: stable JSON schemas, deterministic exit codes, both OpenAI and Anthropic
   dialects, prefix-cache behavior tuned for tool-call loops.
5. **Substrate humility, orchestration ambition.** DRAKKAR does not rewrite GEMMs that the
   platform vendor already tuned for its own silicon. It owns everything above the kernels:
   scheduling, memory policy, caching, UX.
6. **Fail legibly.** Every error a user can hit has an owner, a message that names the cause
   in domain terms, and where possible a remedy command
   ([RFC-0011](../rfcs/RFC-0011-error-taxonomy.md)).

## 2. System decomposition

```
+---------------------------------------------------------------------------+
|  drakkar (single binary, Rust)                                            |
|                                                                           |
|  CLI (drakkar-cli)     HTTP API (drakkar-server)      Desktop shim (v1.0) |
|   run/pull/fit/...      /v1/chat, /v1/messages         C ABI consumers    |
|        \                       |                           /              |
|         +------------ Session & Request Layer ------------+               |
|                       (dialect normalization,                             |
|                        chat templates, tool parsing,                      |
|                        structured-output grammar:                         |
|                        drakkar-grammar)                                   |
|                        |                                                  |
|                        v                                                  |
|                 Scheduler (drakkar-sched, RFC-0007)                       |
|                 continuous batching, chunked prefill,                     |
|                 admission control, fairness                               |
|                        |                                                  |
|   Model Manager <------+------> Feasibility Engine (drakkar-fit,          |
|   (drakkar-models,     |        RFC-0004): memory model, hw               |
|    RFC-0006)           |        profiles, calibration store               |
|   HF hub, storage,     v                                                  |
|   convert/quantize   Engine Actor (drakkar-engine, dedicated thread)      |
|                      owns the backend instance + KV pool (RFC-0005)       |
|                        |                                                  |
|           +------------+--------------+                                   |
|           v                           v                                   |
|  Backend A: drakkar-mlx        Backend B: drakkar-gguf (cargo feature)    |
|  C++ shim over MLX core        llama.cpp via FFI                          |
|  via drakkar-mlx-sys           GGUF coverage path                         |
|  (RFC-0002, RFC-0003,          (RFC-0002 D4)                              |
|   RFC-0010)                                                               |
|           \                           /                                   |
|            +------- Metal / macOS --------+                               |
+---------------------------------------------------------------------------+
```

The same decomposition in prose. A single process hosts, top to bottom: two front ends (CLI
and HTTP server) that share one session and request layer; the session layer normalizes
either API dialect into one internal `GenerationRequest`, renders chat templates, serializes
tools, and compiles structured-output grammars. Below it, the scheduler is the concurrency
brain: it admits requests against the memory contract, forms decode batches, slices prefill
into chunks interleaved with decode, and enforces per-request limits. The scheduler consults
two sibling services: the feasibility engine (the single source of truth for memory math,
used identically by CLI preflight, the `/fit` endpoint, and admission control) and the model
manager (reference resolution, downloads, integrity, conversion, the content-addressed
store). At the bottom, a per-model engine actor on a dedicated OS thread exclusively owns
the backend instance, the KV block pool, and the Metal stream; it is the only component
that touches GPU state. The actor drives one of two backends through the `InferenceBackend`
trait: the primary MLX backend (a C++17 shim statically linking a pinned MLX core, exposed
over a stable `dk_*` C ABI, bound in Rust via `drakkar-mlx-sys`;
[RFC-0002 D2](../rfcs/RFC-0002-stack-selection.md#proposed-design)) or the feature-gated
GGUF backend embedding llama.cpp
([RFC-0002 D4](../rfcs/RFC-0002-stack-selection.md#proposed-design)).

Component responsibilities (normative, from
[RFC-0001 §3.1](../rfcs/RFC-0001-architecture.md#proposed-design)):

| Component | Owns | Explicitly does not own |
|---|---|---|
| CLI front end | Parsing, validation, printing; human and `--json` output generated from the same internal structs ([RFC-0008](../rfcs/RFC-0008-cli-ux.md)) | Business logic (lives in library crates so the desktop app and tests reuse it) |
| HTTP API server | Stateless handlers; dialect normalization into `GenerationRequest`; streaming responses in the caller's dialect ([RFC-0007](../rfcs/RFC-0007-api-server.md)) | Scheduling, batching decisions |
| Scheduler | Admission control, decode-batch formation, chunked prefill interleaving, per-request limits, fairness | GPU execution, memory sizing formulas |
| Feasibility engine | Memory requirements, context ceilings, performance estimates from model metadata + hardware profile ([RFC-0004](../rfcs/RFC-0004-feasibility-engine.md)) | I/O beyond hardware info and its calibration store (pure library) |
| Model manager | Reference resolution, HF hub client, resumable downloads, integrity verification, convert/quantize, content-addressed store ([RFC-0006](../rfcs/RFC-0006-model-pipeline.md)) | Inference, memory policy |
| Engine actor | Backend instance, KV block pool, Metal stream; the message loop in [§5](#5-process-and-threading-model) | Scheduling policy (A6) |
| Backends | Math: graph construction, kernels, KV storage primitives, fused sampling | Scheduling decisions (A6); anything above the seam |

## 3. Crate map (LD24)

The workspace contains eleven crates. Names are frozen by locked decision LD24 and
[RFC-0002 D1](../rfcs/RFC-0002-stack-selection.md#proposed-design).

- **`drakkar-core`** — the bottom of the dependency graph. Shared vocabulary types
  (`GenerationRequest`, `ModelArtifact`, `MemoryBudget`, `MemoryReport`, `Capabilities`,
  `SamplerParams`, token/usage types), the error taxonomy
  ([RFC-0011](../rfcs/RFC-0011-error-taxonomy.md)), config schema, versioned JSON schema
  definitions (invariant I4), and tracing conventions. No I/O, no async runtime dependency
  beyond `serde`/`thiserror`-class utility crates. Everything depends on it; it depends on
  no other workspace crate.
- **`drakkar-fit`** — the feasibility engine ([RFC-0004](../rfcs/RFC-0004-feasibility-engine.md)).
  Pure library: memory model, hardware profiles, context ceilings, TTFT/decode estimates,
  calibration-store reader. Depends only on `drakkar-core` (invariant I3: sizing formulas
  live here and nowhere else). Its purity is what lets CLI preflight, the `/fit` endpoint,
  and scheduler admission control share one implementation.
- **`drakkar-grammar`** — structured-output engine: JSON-schema and grammar-constrained
  decoding via `llguidance` ([RFC-0002 D1](../rfcs/RFC-0002-stack-selection.md#proposed-design)).
  Compiles grammars and advances constraint state per sampled token, entirely in Rust.
  Depends only on `drakkar-core`.
- **`drakkar-engine`** — defines the `InferenceBackend` trait ([§6](#6-the-backend-seam))
  and the `KvPool` interface ([RFC-0005](../rfcs/RFC-0005-kv-cache.md)), and implements the
  engine actor: the dedicated thread, its message loop, channel plumbing, memory-contract
  enforcement, and (v0.3) the multi-engine pool manager. Depends on `drakkar-core` and
  `drakkar-fit` (for contract verification at load). Contains zero backend-specific code.
- **`drakkar-models`** — the acquisition pipeline ([RFC-0006](../rfcs/RFC-0006-model-pipeline.md)):
  reference resolution and aliases, HF hub client (`hf-hub`), resumable verified downloads,
  safetensors/GGUF inspection, convert/quantize orchestration, the content-addressed store,
  and HF-cache interop (LD4). Depends on `drakkar-core`. Tokenizer loading (`tokenizers`)
  lives here.
- **`drakkar-sched`** — the scheduler ([RFC-0007](../rfcs/RFC-0007-api-server.md)):
  admission control (calling `drakkar-fit`), continuous batching, chunked prefill,
  ITL-protection interleaving, per-request limits, prefix-hash computation for cache lookup.
  Depends on `drakkar-core`, `drakkar-fit`, `drakkar-engine` (it speaks to actors over the
  channel protocol `drakkar-engine` defines).
- **`drakkar-server`** — HTTP layer (`axum`/`tokio`/`tower`): OpenAI and Anthropic dialect
  handlers, SSE streaming, `/fit`, `/v1/models`. Library crate consumed by `drakkar-cli`'s
  `serve` subcommand. Depends on `drakkar-core`, `drakkar-sched`, `drakkar-engine`,
  `drakkar-fit`, `drakkar-models`, `drakkar-grammar`.
- **`drakkar-cli`** — the binary. Command parsing (`clap`), output rendering (human +
  `--json` from the same structs, [RFC-0008](../rfcs/RFC-0008-cli-ux.md)), and the
  composition root: the only crate that names backend crates, and only to call their
  factory functions ([§3.1](#31-dependency-direction-rules)). Depends on every other
  workspace crate.
- **`drakkar-mlx-sys`** — raw FFI bindings (bindgen) to the `dk_*` C ABI exported by the
  vendored C++ shim ([RFC-0002 D2](../rfcs/RFC-0002-stack-selection.md#proposed-design),
  [RFC-0010](../rfcs/RFC-0010-backend-abi.md)). Builds and statically links the shim and
  the pinned MLX core; embeds compiled Metal shaders (metallib) per
  [RFC-0002 D5](../rfcs/RFC-0002-stack-selection.md#proposed-design). Depends on no
  workspace crate. `unsafe` lives here and in `drakkar-mlx` only.
- **`drakkar-mlx`** — backend A. Safe Rust wrapper over `drakkar-mlx-sys` implementing
  `InferenceBackend` and `KvPool`; model-graph construction is config-driven per
  [RFC-0002 D3](../rfcs/RFC-0002-stack-selection.md#proposed-design) (launch set:
  Llama-family, Qwen3/3.5 dense + MoE, Gemma-family hybrid SWA, gpt-oss, Mistral-family,
  DeepSeek-lineage MLA). Depends on `drakkar-core`, `drakkar-engine`, `drakkar-mlx-sys`.
  Exposes exactly one public constructor: `pub fn backend() -> Box<dyn InferenceBackend>`
  (plus a capability probe); all MLX-typed items are `pub(crate)`.
- **`drakkar-gguf`** — backend B ([RFC-0002 D4](../rfcs/RFC-0002-stack-selection.md#proposed-design)).
  llama.cpp embedded via FFI, implementing the same trait with reduced `Capabilities`.
  Cargo feature `gguf`, on by default in release builds. Depends on `drakkar-core`,
  `drakkar-engine`. Same single-constructor public surface as `drakkar-mlx`.

### 3.1 Dependency direction rules

Strict layering, enforced in CI by a workspace dependency-graph check (`cargo tree`-based;
a PR that introduces a forbidden edge fails CI):

```
Layer 4  drakkar-cli
Layer 3  drakkar-server
Layer 2  drakkar-sched          drakkar-mlx   drakkar-gguf     (peers; no edges between them)
Layer 1  drakkar-engine  drakkar-models  drakkar-mlx-sys
Layer 0  drakkar-core    drakkar-fit*    drakkar-grammar*      (* depend on drakkar-core only)
```

| Rule | Statement |
|---|---|
| DEP1 | A crate MAY depend only on crates in strictly lower layers, and on `drakkar-core` always. Same-layer edges are forbidden. |
| DEP2 | `drakkar-core` MUST NOT depend on any workspace crate. `drakkar-fit` and `drakkar-grammar` MUST depend on `drakkar-core` only. |
| DEP3 | Backends (`drakkar-mlx`, `drakkar-gguf`) implement traits defined in `drakkar-engine`; `drakkar-engine` MUST NOT depend on any backend crate. |
| DEP4 | Only the composition root (`drakkar-cli`) MAY name backend crates as dependencies, and only to call their factory functions returning `Box<dyn InferenceBackend>`. `drakkar-server`, `drakkar-sched`, and everything else above the seam MUST NOT depend on backend crates. |
| DEP5 | No crate above the seam may name Metal, MLX, or llama.cpp types (invariant I5). Backend crates MUST NOT re-export FFI types in their public API; this is verified by a public-API lint in CI (`cargo public-api` diff gate). |
| DEP6 | `drakkar-mlx-sys` MUST NOT depend on any workspace crate; it exposes raw `dk_*` symbols only. |
| DEP7 | Non-workspace dependencies follow [RFC-0002 D6](../rfcs/RFC-0002-stack-selection.md#proposed-design): tokio, axum, tower, hf-hub, tokenizers, safetensors, serde, llguidance, sysinfo + IOKit bindings, tracing, criterion. Versions are pinned in the workspace `Cargo.toml`; MSRV pinned per [RFC-0002 D5](../rfcs/RFC-0002-stack-selection.md#proposed-design); MLX pinned per DRAKKAR release with an upgrade target of within two MLX releases. |

The v1.0 desktop app (SwiftUI) is not a workspace crate: it consumes the engine's C ABI
([RFC-0002 D2](../rfcs/RFC-0002-stack-selection.md#proposed-design),
[RFC-0010](../rfcs/RFC-0010-backend-abi.md)) and adds no edges to this graph.

## 4. Requirement register (process, state, security)

The lettered requirements below are normative and cited by ID throughout the corpus. This
register indexes A1–A12 and points to the section that specifies each in full.

| ID | Requirement | Specified in |
|---|---|---|
| A1 | Single process; `run` hosts the engine in-process, `serve` daemonizes under launchd from v0.3; no engine↔server IPC in v1 | [§5](#5-process-and-threading-model) |
| A2 | Exactly one engine actor thread per model owns all backend and GPU state and drives a message loop | [§5](#5-process-and-threading-model) |
| A3 | Scheduler and HTTP run on tokio workers, talking to the actor over bounded MPSC channels; the engine thread never performs blocking sends | [§5](#5-process-and-threading-model) |
| A4 | Downloads, conversion, large-prompt tokenization, and SSD KV-tier I/O run on the blocking-task pool, never on the engine thread | [§5](#5-process-and-threading-model) |
| A5 | Multi-model (v0.3) runs one actor per resident model under a pool manager enforcing a global memory contract with LRU/TTL eviction | [§5](#5-process-and-threading-model) |
| A6 | The backend trait is step-granular: scheduling policy stays in Rust, only math in the backend | [§6](#6-the-backend-seam) |
| A7 | `Capabilities` gates features at runtime; no caller may assume a capability `capabilities()` did not report | [§6](#6-the-backend-seam) |
| A8 | Everything under `~/.drakkar` is reconstructible; config is the only user-edited file; precedence flags > env > file > defaults | [§8](#8-state-on-disk) |
| A9 | Server binds `127.0.0.1:11711` by default; other interfaces require an explicit flag plus API key; CORS off by default | [§9](#9-security-and-privacy-posture) |
| A10 | No telemetry and no phone-home version checks by default (`doctor --check-update` is explicit and on-demand) | [§9](#9-security-and-privacy-posture) |
| A11 | Downloaded artifacts are data, never code: safetensors/GGUF only, no pickle or `trust_remote_code` | [§9](#9-security-and-privacy-posture) |
| A12 | HF tokens are read from standard HF locations or the keychain, never written to logs | [§9](#9-security-and-privacy-posture) |

## 5. Process and threading model

From [RFC-0001 §4](../rfcs/RFC-0001-architecture.md#proposed-design):

- **A1.** DRAKKAR runs as a single process. `drakkar run` hosts the engine in-process;
  `drakkar serve` optionally daemonizes under launchd from v0.3
  ([RFC-0008](../rfcs/RFC-0008-cli-ux.md)). No IPC between engine and server in v1.
  (Crash-isolating engine subprocess: kept open, owner abdelstark, revisit at v1.0 for the
  desktop app — LD13.)
- **A2.** **The engine actor owns the GPU.** MLX arrays are not thread-safe (`!Send`/`!Sync`
  at the binding layer; per-thread Metal streams underneath), and a single Metal command
  queue per stream serializes GPU work anyway. Therefore exactly one engine thread per
  loaded model owns all backend state and processes a message loop:

  ```rust
  enum EngineMsg {
      Admit(AdmitReq),          // reserve KV blocks for a sequence
      Prefill(PrefillChunk),    // one chunk, <= chunk_size tokens
      DecodeStep(DecodeBatch),  // one step across B sequences
      Evict(SeqSet),            // release / demote KV blocks
      Snapshot(SnapshotReq),    // KV persistence (RFC-0005)
      Unload,                   // drop model, drain, report final MemoryReport
  }
  ```

  This turns the substrate's threading constraint into an architectural feature: no locks
  around model state, deterministic memory accounting, trivial reasoning about GPU
  occupancy.
- **A3.** The scheduler and HTTP layer run on tokio worker threads and communicate with the
  engine actor via **bounded MPSC channels**; token events stream back per-request over
  dedicated channels feeding SSE writers. Backpressure is by channel capacity, never by
  blocking the engine thread. The engine thread MUST NOT perform blocking sends.
- **A4.** Downloads, conversion, tokenization of large prompts, and disk I/O for the SSD KV
  tier run on a blocking-task pool (`tokio::task::spawn_blocking`), never on the engine
  thread.
- **A5.** Multi-model (v0.3) instantiates one engine actor per resident model under a pool
  manager enforcing a global memory contract with LRU/TTL eviction; the Metal device is
  shared, budgets are not. Strict per-engine Metal residency isolation is the v0.3 design;
  sharing a residency set is kept open pending v0.3 profiling (LD12, owner abdelstark).

## 6. The backend seam

The trait, defined in `drakkar-engine`
([RFC-0001 §5](../rfcs/RFC-0001-architecture.md#proposed-design); C-ABI projection in
[RFC-0010](../rfcs/RFC-0010-backend-abi.md)):

```rust
trait InferenceBackend {
    fn load(&mut self, artifact: &ModelArtifact, budget: MemoryBudget) -> Result<ModelHandle>;
    fn prefill(&mut self, h: &ModelHandle, batch: PrefillChunk) -> Result<PrefillOut>;
    fn decode(&mut self, h: &ModelHandle, batch: DecodeBatch) -> Result<DecodeOut>;   // one step, B sequences
    fn kv(&mut self) -> &mut dyn KvPool;         // RFC-0005 interface
    fn sample(&mut self, logits: LogitsRef, params: &SamplerParams) -> Result<TokenOut>;
    fn memory_report(&self) -> MemoryReport;      // actual, vs contract
    fn capabilities(&self) -> Capabilities;       // nax, kv_quant bits, spec_decode, ...
}
```

- **A6.** The trait is deliberately step-granular (one decode step for a batch), keeping the
  scheduling policy in Rust and only math in the backend. Backends MUST NOT own scheduling
  decisions.
- **A7.** `Capabilities` gates features at runtime (for example Neural Accelerator tensor
  paths on macOS < 26.2 report absent, and the fit engine's TTFT model switches constants
  accordingly). No caller may assume a capability that `capabilities()` did not report
  ([PRD](../../PRD.md#52-non-functional) P15).

All types in the trait signature are `drakkar-core`/`drakkar-engine` types. This seam is the
only portability boundary in the system (invariant I5) and the revisit hinge for future
backends ([RFC-0002 R1–R3](../rfcs/RFC-0002-stack-selection.md#proposed-design)).

## 7. Request lifecycle (reference data flow)

Normative reference flow from [RFC-0001 §6](../rfcs/RFC-0001-architecture.md#proposed-design).
Thread placement per A2–A4 is part of the contract.

| Step | Action | Runs on |
|---|---|---|
| 1 | HTTP request arrives; dialect normalizer produces `GenerationRequest` (messages rendered through the model's chat template, tools serialized, grammar compiled if `response_format` demands it) | tokio worker |
| 2 | Tokenize; compute prefix hash chain for cache lookup ([RFC-0005](../rfcs/RFC-0005-kv-cache.md#proposed-design)) | blocking pool (A4) |
| 3 | Admission control: feasibility engine confirms KV headroom for `prompt_len + max_tokens` at current occupancy; reject with structured 429/413 carrying remediation fields if not ([RFC-0011](../rfcs/RFC-0011-error-taxonomy.md), [RFC-0007](../rfcs/RFC-0007-api-server.md)) | tokio worker (scheduler) |
| 4 | Scheduler splits the uncached prompt suffix into prefill chunks (default 512 tokens) and interleaves them with ongoing decode batches per the ITL-protection policy ([RFC-0007](../rfcs/RFC-0007-api-server.md#proposed-design)) | tokio worker (scheduler) |
| 5 | Engine actor executes prefill/decode messages; sampled tokens stream back over per-request channels; tool-call and reasoning-content deltas parsed incrementally; stop conditions and grammar advance in Rust | engine thread (A2) → tokio (parsing/SSE) |
| 6 | On completion: usage accounting, KV blocks released or retained per prefix-cache policy (honoring per-request `cache: false`, LD8), metrics updated | scheduler + engine thread |

Failure at any step MUST map to a named error in the taxonomy
([RFC-0011](../rfcs/RFC-0011-error-taxonomy.md)); no step may surface a raw Metal or FFI
error to a user.

## 8. State on disk

From [RFC-0001 §7](../rfcs/RFC-0001-architecture.md#proposed-design), consistent with LD23:

```
~/.drakkar/
  models/            content-addressed store + human-readable links (RFC-0006)
  kv-cache/          SSD tier, safetensors blocks (RFC-0005)
  calibration/       per-chip measured constants (RFC-0009)
  logs/
~/.config/drakkar/config.toml
```

- **A8.** Everything under `~/.drakkar` is reconstructible; deleting it is always safe.
  Config is the only file a user edits. Precedence: flags > env (`DRAKKAR_*`) > file >
  defaults (LD23, [RFC-0008](../rfcs/RFC-0008-cli-ux.md)). A custom store volume via
  `storage.path` is supported from v0.1 (LD14,
  [RFC-0006](../rfcs/RFC-0006-model-pipeline.md)).

## 9. Security and privacy posture

From [RFC-0001 §8](../rfcs/RFC-0001-architecture.md#proposed-design):

- **A9.** The server binds `127.0.0.1:11711` by default (LD22); binding other interfaces
  requires an explicit flag plus an API key. CORS off by default.
- **A10.** No telemetry, no phone-home version checks by default
  (`drakkar doctor --check-update` is explicit and on-demand). ([PRD](../../PRD.md#52-non-functional) P13.)
- **A11.** Downloaded artifacts are treated as data, never code: safetensors and GGUF only;
  no pickle, no `trust_remote_code`. Models whose architecture is unsupported fail with a
  named error, not arbitrary code execution
  ([RFC-0006](../rfcs/RFC-0006-model-pipeline.md#proposed-design)).
- **A12.** HF tokens are read from the standard HF locations or keychain, never written to
  logs.

## 10. Invariants (the review contract)

Restated verbatim from [RFC-0001 §9](../rfcs/RFC-0001-architecture.md#proposed-design).
Every PR is reviewed against these five statements; a PR that weakens one MUST instead
amend RFC-0001 first and say so in its description.

- **I1.** One engine thread per model; all GPU state confined to it.
- **I2.** Memory contract: `weights + kv_pool + activation_watermark + runtime_overhead <=
  declared_budget` at all times; enforced by the KV pool allocator and admission control,
  verified by `memory_report()` in debug soaks.
- **I3.** Single source of truth for memory math: the feasibility engine library is the only
  place sizing formulas live.
- **I4.** Every user-facing surface (CLI, HTTP) has a JSON representation with a versioned
  schema.
- **I5.** The backend seam is the only portability boundary; nothing above it may name
  Metal, MLX, or llama.cpp types.

Mechanical enforcement: I1 by the actor design (A2) and the `!Send` binding layer; I2 by
the KV allocator plus `memory_report()` assertions in debug soaks
([RFC-0009](../rfcs/RFC-0009-performance.md)); I3 by DEP2 (only `drakkar-fit` contains
sizing formulas — reviewers reject arithmetic on model dimensions elsewhere); I4 by schema
snapshot tests in `drakkar-core`; I5 by DEP4/DEP5 CI gates.

## 11. Open questions

| ID | Question | Owner | Resolution path |
|---|---|---|---|
| OQ-ARCH-1 | Multi-model pool: strict per-engine Metal residency isolation vs a shared residency set (LD12) | abdelstark | Profiling during v0.3 "Fleet"; isolation is the default until data says otherwise |
| OQ-ARCH-2 | Crash isolation: supervisor + engine subprocess so a Metal fault cannot kill the desktop UI (LD13) | abdelstark | Decide at v1.0 "Harbor" alongside the desktop app; measure IPC cost against the in-process baseline first |

## 12. Cross-references

- [RFC-0001 — Architecture](../rfcs/RFC-0001-architecture.md): source of A1–A12, I1–I5.
- [RFC-0002 — Stack Selection](../rfcs/RFC-0002-stack-selection.md): D1–D6, crate list, pins.
- [RFC-0010 — Backend ABI](../rfcs/RFC-0010-backend-abi.md): the `dk_*` C ABI behind `drakkar-mlx-sys`.
- [RFC-0011 — Error Taxonomy](../rfcs/RFC-0011-error-taxonomy.md): the named errors required by the lifecycle in [§7](#7-request-lifecycle-reference-data-flow).
- [00 — Overview](00-overview.md): product framing and milestone map.
- [PRD §4–5](../../PRD.md#4-goals-and-non-goals): the product-level contract this architecture serves.
