# RFC-0001: Architecture Overview and Design Principles

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.1

## Summary

DRAKKAR is structured as a Rust control plane wrapping a native compute core. The control plane owns everything users touch: CLI, HTTP server, scheduler, model management, and the feasibility engine. The compute core owns everything the GPU touches: model graphs, Metal kernels, KV storage, sampling. The two meet at a narrow, versioned internal ABI ([RFC-0010](RFC-0010-backend-abi.md#proposed-design)). This document fixes the decomposition, the process and threading model, the backend seam, and the invariants (I1-I5) every other RFC assumes. It is required by RFC-0002 through RFC-0009 and requires none of them.

## Motivation

The PRD's v1.0 goals ([PRD §4](../../PRD.md#4-goals-and-non-goals), G1-G7) demand properties that cut across every subsystem and therefore must be settled before any subsystem RFC:

- G1 (one-command run) and G6 (dependency-free binary) force a single-process, single-binary shape: no sidecar daemons, no interpreter runtimes, no IPC topology to configure.
- G2 (best-in-class latency) and G4 (a feasibility engine that predicts memory within 7%) require that the GPU memory budget be a *contract* — declared at load, enforced by admission control, never discovered by Metal mid-generation (PRD P11). A contract needs exactly one owner; a decomposition with shared mutable GPU state cannot provide one.
- G3 (agent-grade serving) requires a scheduler that interleaves prefill and decode across concurrent requests, which in turn requires a clean seam between scheduling policy (Rust) and math execution (backend).
- G5 (paged KV, prefix CoW, SSD tier) requires KV ownership to sit with the same actor that owns the backend, or memory accounting fragments across threads.
- N1 keeps a backend seam even though Apple Silicon is the only v1 target: the architecture must make the backend replaceable (`drakkar-mlx` primary, `drakkar-gguf` coverage) without letting backend types leak upward.

This RFC is the decomposition that satisfies all of the above simultaneously: an engine actor that owns the GPU, a scheduler that owns policy, a feasibility library that owns memory math, and a trait seam that owns portability.

## Goals

- Fix a component decomposition in which every subsystem RFC (0002-0012) has exactly one home component and one owning crate from the workspace set (`drakkar-cli`, `drakkar-server`, `drakkar-sched`, `drakkar-fit`, `drakkar-models`, `drakkar-engine`, `drakkar-grammar`, `drakkar-core`, `drakkar-mlx-sys` + `drakkar-mlx`, `drakkar-gguf`).
- Make the GPU memory budget an enforceable contract: a single engine thread owns all backend state (I1), and `weights + kv_pool + activation_watermark + runtime_overhead <= declared_budget` holds at all times (I2), verifiable via `memory_report()`.
- Centralize memory math: the feasibility engine library is the only place sizing formulas live (I3), consumed identically by CLI preflight, the `/fit` endpoint, and admission control.
- Keep the backend replaceable: nothing above the `InferenceBackend` trait may name Metal, MLX, or llama.cpp types (I5), enforced at build time by dependency-edge bans.
- Give every user-facing surface a versioned JSON representation (I4), so the CLI, HTTP API, and future desktop shim are programmatically equivalent.
- Define a threading model with zero locks around model state: MLX's `!Send` arrays become an architectural feature (message-passing actor), not a hazard to be fenced.

## Non-Goals

- Cross-platform process or backend design (Linux/Windows/CUDA); PRD N1. The seam exists, but only the two Apple Silicon backends are specified.
- Crash isolation between engine and server via subprocess in v0.x; revisited at v1.0 (see [Open Questions](#open-questions)).
- Multi-tenant serving, authn/z beyond a local API key, or cross-organization fairness (PRD N3).
- Kernel-level design: graph construction, quantized matmul, and attention kernels belong to [RFC-0002](RFC-0002-stack-selection.md#proposed-design) and [RFC-0003](RFC-0003-inference-core.md#proposed-design); the C ABI surface belongs to [RFC-0010](RFC-0010-backend-abi.md#proposed-design).
- Scheduling policy details (batch formation, chunked prefill, fairness): [RFC-0007](RFC-0007-api-server.md#proposed-design). This RFC only fixes where the scheduler sits and how it talks to the engine.

## Proposed Design

### Design principles

1. **Honest speed.** Performance claims are measured or derived, never asserted. Any number surfaced to a user (fit verdicts, throughput estimates) traces to a formula ([RFC-0004](RFC-0004-feasibility-engine.md#proposed-design)) or a benchmark ([RFC-0009](RFC-0009-performance.md#proposed-design)).
2. **The GPU budget is a contract.** The engine declares a memory budget at model load and never exceeds it. Admission control fails requests early and legibly; Metal must never be the component that discovers we are out of memory.
3. **One binary, zero ceremony.** No Python, no venvs, no containers, no post-install steps. `brew install drakkar; drakkar run qwen3:8b` is the entire onboarding.
4. **Agent-native.** Every interface (CLI, HTTP) is designed to be driven by a program as comfortably as by a human: stable JSON schemas, deterministic exit codes, both OpenAI and Anthropic dialects, prefix-cache behavior tuned for tool-call loops.
5. **Substrate humility, orchestration ambition.** We do not rewrite GEMMs that Apple's MLX team already tuned for their own silicon. We own everything above the kernels: scheduling, memory policy, caching, UX, and we contribute upstream when the kernel layer needs work.
6. **Fail legibly.** Every error a user can hit has an owner, a message that names the cause in domain terms (memory, download, format, engine), and where possible a remedy command ([RFC-0011](RFC-0011-error-taxonomy.md#proposed-design)).

### System decomposition

```
+--------------------------------------------------------------------+
|  drakkar (single binary, Rust)                                     |
|                                                                    |
|  CLI (clap)          HTTP API (axum/tokio)      Desktop shim (v1)  |
|   run/pull/fit/...    /v1/chat, /v1/messages     C ABI consumers   |
|        \                    |                        /             |
|         +-----------+ Session & Request Layer +----+               |
|                     |  (dialect normalization,                     |
|                     |   templates, tool parsing,                   |
|                     |   structured-output grammar)                 |
|                     v                                              |
|              Scheduler (RFC-0007)                                  |
|              continuous batching, chunked prefill,                 |
|              admission control, fairness                           |
|                     |                                              |
|   Model Manager <---+---> Feasibility Engine (RFC-0004)            |
|   (RFC-0006)        |     memory model, hw profiles,               |
|   HF hub, storage,  |     calibration store                        |
|   convert/quantize  v                                              |
|            Engine Actor (dedicated thread)                         |
|            owns the backend instance + KV pool (RFC-0005)          |
|                     |                                              |
|        +------------+-------------+                                |
|        v                          v                                |
|  Backend A: drakkar-mlx     Backend B: drakkar-gguf (feature)      |
|  C++ shim over MLX core     llama.cpp via FFI                      |
|  (RFC-0002 §6, RFC-0003)    GGUF coverage path                     |
|        \                          /                                |
|         +------- Metal / macOS -------+                            |
+--------------------------------------------------------------------+
```

#### Components

- **CLI front end.** Thin: parses, validates, prints. All logic lives in library crates so the desktop app and tests reuse it. Human output and `--json` output are generated from the same internal structs ([RFC-0008](RFC-0008-cli-ux.md#proposed-design)).
- **HTTP API server.** axum on tokio. Stateless handlers that normalize OpenAI and Anthropic request shapes into one internal `GenerationRequest`, then stream results back in the caller's dialect ([RFC-0007](RFC-0007-api-server.md#proposed-design)).
- **Scheduler.** The concurrency brain: admits requests against the memory contract, forms decode batches, slices prefill into chunks interleaved with decode, and enforces per-request limits. Runs on the tokio runtime; communicates with the engine actor over bounded channels.
- **Feasibility engine.** Pure library (no I/O beyond reading hardware info and its calibration store) computing memory requirements, context ceilings, and performance estimates from model metadata and a hardware profile ([RFC-0004](RFC-0004-feasibility-engine.md#proposed-design)). Used by the CLI preflight, the `/fit` endpoint, and the scheduler's admission control (same code, one truth).
- **Model manager.** Resolves references, talks to the Hugging Face hub, downloads with resume, verifies integrity, converts and quantizes, and maintains the local content-addressed store ([RFC-0006](RFC-0006-model-pipeline.md#proposed-design)).
- **Engine actor.** A dedicated OS thread that exclusively owns the backend instance, the KV block pool, and the Metal stream. Everything GPU-adjacent happens here. See [Process and threading model](#process-and-threading-model).
- **Backends.** `drakkar-mlx` is primary: a C++ shim linking the MLX core, exposing a stable C ABI (arrays, model graph construction, quantized matmul, fused attention, sampler ops; [RFC-0002](RFC-0002-stack-selection.md#proposed-design), [RFC-0003](RFC-0003-inference-core.md#proposed-design), [RFC-0010](RFC-0010-backend-abi.md#proposed-design)). `drakkar-gguf` is a feature-flagged llama.cpp embedding for GGUF-only models. Both implement the same Rust `InferenceBackend` trait ([The backend seam](#the-backend-seam)).

### Process and threading model

- A1. DRAKKAR runs as a single process. `drakkar run` hosts the engine in-process; `drakkar serve` optionally daemonizes under launchd ([RFC-0008](RFC-0008-cli-ux.md#proposed-design)). No IPC between engine and server in v1.
- A2. **The engine actor owns the GPU.** MLX arrays are not thread-safe (`!Send`/`!Sync` at the binding layer; per-thread Metal streams underneath), and a single Metal command queue per stream serializes GPU work anyway. Therefore exactly one engine thread per loaded model owns all backend state and processes a message loop: `Prefill(chunk)`, `DecodeStep(batch)`, `Admit`, `Evict`, `Snapshot`, `Unload`. This turns MLX's threading constraint into an architectural feature: no locks around model state, deterministic memory accounting, trivial reasoning about GPU occupancy.
- A3. The scheduler and HTTP layer run on tokio worker threads and communicate with the engine actor via bounded MPSC channels; token events stream back per-request over dedicated channels feeding SSE writers. Backpressure is by channel capacity, never by blocking the engine thread.
- A4. Downloads, conversion, tokenization of large prompts, and disk I/O for the SSD KV tier run on a blocking-task pool, never on the engine thread.
- A5. Multi-model (v0.3) instantiates one engine actor per resident model under a pool manager enforcing a global memory contract with LRU/TTL eviction; the Metal device is shared, budgets are not. Per-engine Metal residency sets stay strictly isolated (see [Open Questions](#open-questions), OQ1).

### The backend seam

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

- A6. The trait is deliberately step-granular (one decode step for a batch), keeping the scheduling policy in Rust and only math in the backend. Backends MUST NOT own scheduling decisions.
- A7. `Capabilities` gates features at runtime (for example Neural Accelerator tensor paths on macOS < 26.2 report absent, and the fit engine's TTFT model switches constants accordingly).

The Rust-visible trait above is the seam; its FFI realization for `drakkar-mlx` (C ABI functions, versioning, ownership rules) is specified in [RFC-0010](RFC-0010-backend-abi.md#proposed-design).

### Request lifecycle (reference flow)

1. HTTP request arrives; dialect normalizer produces `GenerationRequest` (messages rendered through the model's chat template, tools serialized, grammar compiled if `response_format` demands it).
2. Tokenize (blocking pool). Compute prefix hash chain for cache lookup ([RFC-0005](RFC-0005-kv-cache.md#proposed-design)).
3. Admission control: feasibility engine confirms KV headroom for `prompt_len + max_tokens` at current occupancy; reject with a structured 429/413 carrying remediation fields if not ([RFC-0011](RFC-0011-error-taxonomy.md#proposed-design)).
4. Scheduler splits the uncached prompt suffix into prefill chunks (default 512 tokens) and interleaves them with ongoing decode batches per the ITL-protection policy ([RFC-0007](RFC-0007-api-server.md#proposed-design)).
5. Engine actor executes; sampled tokens stream back; tool-call and reasoning-content deltas are parsed incrementally; stop conditions and grammar advance in Rust.
6. On completion: usage accounting, KV blocks released or retained per prefix-cache policy, metrics updated.

### State on disk

```
~/.drakkar/
  models/            content-addressed store + human-readable links (RFC-0006)
  kv-cache/          SSD tier, safetensors blocks (RFC-0005)
  calibration/       per-chip measured constants (RFC-0009)
  logs/
~/.config/drakkar/config.toml
```

- A8. Everything under `~/.drakkar` is reconstructible; deleting it is always safe. Config is the only file a user edits. Precedence and the `DRAKKAR_*` environment layer are specified in [RFC-0008](RFC-0008-cli-ux.md#proposed-design); a custom store volume via `storage.path` is supported from v0.1 ([RFC-0006](RFC-0006-model-pipeline.md#proposed-design)).

### Security and privacy posture

- A9. Server binds `127.0.0.1:11711` by default; binding other interfaces requires an explicit flag plus an API key. CORS off by default ([RFC-0007](RFC-0007-api-server.md#proposed-design)).
- A10. No telemetry, no phone-home version checks by default (`drakkar doctor --check-update` is explicit and on-demand).
- A11. Downloaded artifacts are treated as data, never code: safetensors and GGUF only; no pickle, no `trust_remote_code`. Models whose architecture is unsupported fail with a named error, not arbitrary code execution ([RFC-0006](RFC-0006-model-pipeline.md#proposed-design)).
- A12. HF tokens are read from the standard HF locations or keychain, never written to logs.

### Invariants (assumed by all other RFCs)

- I1. One engine thread per model; all GPU state confined to it.
- I2. Memory contract: `weights + kv_pool + activation_watermark + runtime_overhead <= declared_budget` at all times; enforced by the KV pool allocator and admission control, verified by `memory_report()` in debug soaks.
- I3. Single source of truth for memory math: the feasibility engine library is the only place sizing formulas live.
- I4. Every user-facing surface (CLI, HTTP) has a JSON representation with a versioned schema.
- I5. The backend seam is the only portability boundary; nothing above it may name Metal, MLX, or llama.cpp types.

## Alternatives Considered

### Multi-process split: engine daemon + server front end

Run the engine as a separate process (as vLLM's engine-core/API-server split does) with the CLI and HTTP server talking to it over a local socket. Rejected for v1: the IPC layer adds serialization cost on the token hot path (every decode step crosses a process boundary), a protocol to version, and a second process to supervise — all to buy crash isolation that no v0.x consumer needs, since the CLI and server share the engine's fate anyway. The one consumer that genuinely needs isolation is the v1.0 desktop app, where a Metal fault must not kill the UI; that is exactly the revisit point recorded as OQ2 below. The single-process design keeps the option open because the scheduler-to-engine boundary is already message-passing (A3): promoting those channels to a socket is a transport change, not a redesign.

### Thread pool sharing the GPU under locks

Let tokio worker threads call the backend directly, guarding model state with a mutex (or finer-grained locks around the KV pool, sampler, and graph). Rejected on three grounds. First, MLX arrays are `!Send`/`!Sync` at the binding layer, so sharing them across threads is not merely risky but unrepresentable without unsafe laundering. Second, a single Metal command queue per stream serializes GPU submission regardless — the concurrency a lock-based design pretends to offer does not exist below it. Third, locks make memory accounting non-deterministic (which thread's allocation trips the budget?) and I2 unverifiable in practice. The actor gives up nothing (the GPU was serial anyway) and removes the entire lock-ordering and poisoning surface.

### Framework-owned scheduling (adopt a monolithic engine)

Embed an existing engine that owns its own scheduler, batching, and KV policy (the vLLM-style monolith, e.g. building the product as a thin wrapper over vllm-metal or mlx-lm's server). Rejected: the PRD's differentiators — the feasibility contract wired into admission control, prefix-cache policy tuned for agent loops, per-request memory accounting — all live in the scheduling and memory-policy layer. Ceding that layer to a framework reduces DRAKKAR to packaging, contradicts the stack-selection control constraint (RFC-0002 S3: we own everything above the kernels), and reintroduces the Python runtime that G6 exists to eliminate. A6 is the load-bearing consequence: backends do math, Rust does policy.

### Library-only design (no server)

Ship DRAKKAR as a Rust library plus CLI, letting integrators bring their own server. Rejected: the primary user (PRD §3, agent builder) consumes OpenAI/Anthropic HTTP dialects, and G3's serving requirements (continuous batching, streaming with usage accounting, structured output) are product features, not integration exercises. A library seam still exists internally — the CLI, server, and future C-ABI desktop shim all sit on the same library crates — but the served HTTP surface is the product.

## Drawbacks

- **Single process couples failure domains.** A backend fault (Metal error, shim bug, allocator corruption) takes down the HTTP server and any in-flight requests with it. Accepted for v0.x because all current consumers are CLI-lifetime processes; revisited for the desktop app (OQ2).
- **The actor serializes GPU work per model.** All prefill and decode for one model funnels through one thread. This is what the hardware imposes anyway (one Metal queue), but it also means CPU-side work accidentally placed on the engine thread delays GPU submission directly. A4 is the mitigation and must be policed in review; the [Testing Strategy](#testing-strategy) includes a stall detector.
- **The seam adds indirection.** Every feature must be expressed in seam vocabulary (`Capabilities`, `DecodeBatch`, `KvPool`) before a backend can ship it, and backend-specific fast paths risk lowest-common-denominator design. Accepted: `Capabilities` (A7) is the pressure valve, and the seam is precisely what keeps I5 true and the GGUF backend possible.
- **Bounded channels can deadlock if wired carelessly.** A3's backpressure discipline requires that the engine thread never blocks sending to a full per-request channel; token channels must therefore be sized or shed deterministically ([RFC-0007](RFC-0007-api-server.md#proposed-design) owns the policy).

## Migration / Rollout

| Milestone | Architectural scope |
|-----------|---------------------|
| v0.1 "First light" | Single engine actor, single model, single request; CLI + streaming OpenAI chat endpoint in one process (A1-A4). Seam trait lands with the `drakkar-mlx` backend only; `drakkar-gguf` is a declared cargo feature that does not yet compile to a working backend. Invariants I1-I5 in force from the first commit. |
| v0.2 "Convoy" | Scheduler grows continuous batching and chunked prefill behind the same actor message loop (`DecodeStep(batch)` becomes multi-sequence); `drakkar-gguf` backend activates behind its feature flag; no seam changes — the v0.1 trait already carries batches. |
| v0.3 "Fleet" | Pool manager lands: one actor per resident model, global memory contract, LRU/TTL eviction (A5), strict per-engine Metal residency isolation per OQ1's default. Daemon mode under launchd. |
| v1.0 "Harbor" | C-ABI consumers arrive (SwiftUI menu-bar app over the engine's exported ABI); OQ2 (engine subprocess for crash isolation) is decided before the desktop app ships. |

No schema migrations arise from this RFC itself; `GenerationRequest` and the seam types are versioned with the workspace and are internal until the C ABI freezes them ([RFC-0010](RFC-0010-backend-abi.md#proposed-design)).

## Testing Strategy

- **Actor message-loop unit tests** (`drakkar-engine`, with a `MockBackend` implementing `InferenceBackend`): `admit_then_prefill_then_decode_ordering` (messages processed FIFO per model), `unload_drains_inflight` (`Unload` completes only after in-flight sequences finish or evict), `evict_releases_kv_blocks` (mock pool balance returns to pre-admit level), `backend_error_propagates_as_named_error` (RFC-0011 taxonomy, engine thread survives).
- **Invariant I2 (memory contract equality)**: debug builds assert `memory_report().total <= budget` after every `Prefill`/`DecodeStep`/`Admit`/`Evict` message; the RFC-0009 24 h mixed-load soak (`soak_memory_contract`) runs with these assertions on and additionally checks PRD P14 (RSS drift < 2% after warmup, zero request failures).
- **Invariant I5 (seam is the only portability boundary)**: enforced at compile time as dependency-edge bans — a `cargo-deny` configuration forbids `drakkar-cli`, `drakkar-server`, `drakkar-sched`, `drakkar-fit`, `drakkar-models`, `drakkar-grammar`, and `drakkar-core` from depending (directly or transitively) on `drakkar-mlx-sys`, `drakkar-mlx`, `drakkar-gguf`, or any Metal/Objective-C binding crate; only `drakkar-engine` may. CI job `seam-deps-check` fails the build on violation, making "nothing above the seam names backend types" a property of the dependency graph rather than a review convention.
- **Lifecycle integration test** (`lifecycle_openai_stream`): boots the server on a loopback port with a small real model, drives the full reference flow (steps 1-6), and asserts: structured 429/413 with remediation fields when admission is forced to fail, SSE deltas arrive in order, usage accounting matches token counts, KV blocks are released after completion.
- **Backpressure property test** (`channels_never_block_engine`): under randomized slow/stalled SSE consumers, assert the engine thread never blocks on a send (drops or sheds per RFC-0007 policy) and no deadlock occurs across bounded scheduler-engine channels.
- **Engine-thread stall detector** (debug builds): any message handler exceeding a threshold wall time without a backend call in progress logs a named warning; the soak fails on occurrences, policing A4.
- **State-on-disk test** (`delete_state_dir_is_safe`, A8): remove `~/.drakkar` between runs; the next invocation reconstructs everything without error.

## Open Questions

- **OQ1 — Multi-model Metal residency sharing.** Should v0.3's pool share one Metal residency set across engine actors to reduce wiring churn, or keep strict per-engine isolation? Decision to date: strict per-engine isolation is the v0.3 design; sharing is adopted only if profiling shows wiring churn is a measured cost. Owner: abdelstark. Resolution path: profiling during v0.3 "Fleet" implementation, reported against the RFC-0009 fleet.
- **OQ2 — Crash isolation via engine subprocess.** Is a supervisor + engine subprocess worth the IPC cost for the desktop app, where a Metal fault must not kill the UI? Owner: abdelstark. Resolution path: decide at v1.0 "Harbor" scoping, informed by v0.x crash telemetry from opt-in reports and the desktop app's process model; the message-passing engine boundary (A3) keeps both outcomes cheap.

## References

- [PRD](../../PRD.md) — §1 vision, §4 goals/non-goals, §5.2 P11-P14, §8 roadmap.
- [RFC-0002: Stack Selection](RFC-0002-stack-selection.md) — substrate decision record; the C++ shim this architecture assumes.
- [RFC-0010: Backend ABI](RFC-0010-backend-abi.md) — FFI realization of the backend seam.
- MLX threading and stream semantics: ml-explore/mlx documentation; mlx-rs/mlxrs crate documentation on `!Send` arrays and per-thread streams (2026).
- vLLM engine/scheduler separation and vllm-metal plugin layering (MetalPlatform/Worker/ModelRunner), docs.vllm.ai/projects/vllm-metal (2026).
- oMLX architecture notes (EnginePool, scheduler, cache stack), github.com/jundot/omlx (2026).
