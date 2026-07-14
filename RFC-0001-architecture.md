# RFC-0001: Architecture Overview and Design Principles

**Status:** Draft
**Author:** A. Bakhta
**Created:** 2026-07-14
**Requires:** none
**Required by:** RFC-0002 .. RFC-0009

## 1. Summary

DRAKKAR is structured as a Rust control plane wrapping a native compute core. The control plane owns everything users touch: CLI, HTTP server, scheduler, model management, and the feasibility engine. The compute core owns everything the GPU touches: model graphs, Metal kernels, KV storage, sampling. The two meet at a narrow, versioned internal ABI. This document fixes the decomposition, the process and threading model, and the invariants every other RFC assumes.

## 2. Design principles

1. **Honest speed.** Performance claims are measured or derived, never asserted. Any number surfaced to a user (fit verdicts, throughput estimates) traces to a formula (RFC-0004) or a benchmark (RFC-0009).
2. **The GPU budget is a contract.** The engine declares a memory budget at model load and never exceeds it. Admission control fails requests early and legibly; Metal must never be the component that discovers we are out of memory.
3. **One binary, zero ceremony.** No Python, no venvs, no containers, no post-install steps. `brew install drakkar; drakkar run qwen3:8b` is the entire onboarding.
4. **Agent-native.** Every interface (CLI, HTTP) is designed to be driven by a program as comfortably as by a human: stable JSON schemas, deterministic exit codes, both OpenAI and Anthropic dialects, prefix-cache behavior tuned for tool-call loops.
5. **Substrate humility, orchestration ambition.** We do not rewrite GEMMs that Apple's MLX team already tuned for their own silicon. We own everything above the kernels: scheduling, memory policy, caching, UX, and we contribute upstream when the kernel layer needs work.
6. **Fail legibly.** Every error a user can hit has an owner, a message that names the cause in domain terms (memory, download, format, engine), and where possible a remedy command.

## 3. System decomposition

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

### 3.1 Components

- **CLI front end.** Thin: parses, validates, prints. All logic lives in library crates so the desktop app and tests reuse it. Human output and `--json` output are generated from the same internal structs (RFC-0008).
- **HTTP API server.** axum on tokio. Stateless handlers that normalize OpenAI and Anthropic request shapes into one internal `GenerationRequest`, then stream results back in the caller's dialect (RFC-0007).
- **Scheduler.** The concurrency brain: admits requests against the memory contract, forms decode batches, slices prefill into chunks interleaved with decode, and enforces per-request limits. Runs on the tokio runtime; communicates with the engine actor over bounded channels.
- **Feasibility engine.** Pure library (no I/O beyond reading hardware info and its calibration store) computing memory requirements, context ceilings, and performance estimates from model metadata and a hardware profile (RFC-0004). Used by the CLI preflight, the `/fit` endpoint, and the scheduler's admission control (same code, one truth).
- **Model manager.** Resolves references, talks to the Hugging Face hub, downloads with resume, verifies integrity, converts and quantizes, and maintains the local content-addressed store (RFC-0006).
- **Engine actor.** A dedicated OS thread that exclusively owns the backend instance, the KV block pool, and the Metal stream. Everything GPU-adjacent happens here. See §4.
- **Backends.** `drakkar-mlx` is primary: a C++ shim linking the MLX core, exposing a stable C ABI (arrays, model graph construction, quantized matmul, fused attention, sampler ops). `drakkar-gguf` is a feature-flagged llama.cpp embedding for GGUF-only models. Both implement the same Rust `InferenceBackend` trait (§5).

## 4. Process and threading model

- A1. DRAKKAR runs as a single process. `drakkar run` hosts the engine in-process; `drakkar serve` optionally daemonizes under launchd (RFC-0008 §7). No IPC between engine and server in v1.
- A2. **The engine actor owns the GPU.** MLX arrays are not thread-safe (`!Send`/`!Sync` at the binding layer; per-thread Metal streams underneath), and a single Metal command queue per stream serializes GPU work anyway. Therefore exactly one engine thread per loaded model owns all backend state and processes a message loop: `Prefill(chunk)`, `DecodeStep(batch)`, `Admit`, `Evict`, `Snapshot`, `Unload`. This turns MLX's threading constraint into an architectural feature: no locks around model state, deterministic memory accounting, trivial reasoning about GPU occupancy.
- A3. The scheduler and HTTP layer run on tokio worker threads and communicate with the engine actor via bounded MPSC channels; token events stream back per-request over dedicated channels feeding SSE writers. Backpressure is by channel capacity, never by blocking the engine thread.
- A4. Downloads, conversion, tokenization of large prompts, and disk I/O for the SSD KV tier run on a blocking-task pool, never on the engine thread.
- A5. Multi-model (v0.3) instantiates one engine actor per resident model under a pool manager enforcing a global memory contract with LRU/TTL eviction; the Metal device is shared, budgets are not.

## 5. The backend seam

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

## 6. Request lifecycle (reference flow)

1. HTTP request arrives; dialect normalizer produces `GenerationRequest` (messages rendered through the model's chat template, tools serialized, grammar compiled if `response_format` demands it).
2. Tokenize (blocking pool). Compute prefix hash chain for cache lookup (RFC-0005 §4).
3. Admission control: feasibility engine confirms KV headroom for `prompt_len + max_tokens` at current occupancy; reject with a structured 429/413 carrying remediation fields if not.
4. Scheduler splits the uncached prompt suffix into prefill chunks (default 512 tokens) and interleaves them with ongoing decode batches per the ITL-protection policy (RFC-0007 §6).
5. Engine actor executes; sampled tokens stream back; tool-call and reasoning-content deltas are parsed incrementally; stop conditions and grammar advance in Rust.
6. On completion: usage accounting, KV blocks released or retained per prefix-cache policy, metrics updated.

## 7. State on disk

```
~/.drakkar/
  models/            content-addressed store + human-readable links (RFC-0006)
  kv-cache/          SSD tier, safetensors blocks (RFC-0005 §7)
  calibration/       per-chip measured constants (RFC-0009 §6)
  logs/
~/.config/drakkar/config.toml
```

- A8. Everything under `~/.drakkar` is reconstructible; deleting it is always safe. Config is the only file a user edits.

## 8. Security and privacy posture

- A9. Server binds 127.0.0.1 by default; binding other interfaces requires an explicit flag plus an API key. CORS off by default.
- A10. No telemetry, no phone-home version checks by default (`drakkar doctor --check-update` is explicit and on-demand).
- A11. Downloaded artifacts are treated as data, never code: safetensors and GGUF only; no pickle, no `trust_remote_code`. Models whose architecture is unsupported fail with a named error, not arbitrary code execution (RFC-0006 §5).
- A12. HF tokens are read from the standard HF locations or keychain, never written to logs.

## 9. Invariants (assumed by all other RFCs)

- I1. One engine thread per model; all GPU state confined to it.
- I2. Memory contract: `weights + kv_pool + activation_watermark + runtime_overhead <= declared_budget` at all times; enforced by the KV pool allocator and admission control, verified by `memory_report()` in debug soaks.
- I3. Single source of truth for memory math: the feasibility engine library is the only place sizing formulas live.
- I4. Every user-facing surface (CLI, HTTP) has a JSON representation with a versioned schema.
- I5. The backend seam is the only portability boundary; nothing above it may name Metal, MLX, or llama.cpp types.

## 10. Open questions

1. Should v0.3's multi-model pool share one Metal residency set across engines to reduce wiring churn, or keep strict per-engine isolation? (Leaning isolation until profiling says otherwise.)
2. Crash isolation: is a supervisor + engine subprocess worth the IPC cost for the desktop app, where a Metal fault must not kill the UI? Revisit at v1.0.

## References

- MLX threading and stream semantics: ml-explore/mlx docs; mlxrs crate documentation on `!Send` arrays and per-thread streams (2026)
- vLLM engine/scheduler separation and vllm-metal plugin layering (MetalPlatform/Worker/ModelRunner), docs.vllm.ai/projects/vllm-metal (2026)
- oMLX architecture notes (EnginePool, scheduler, cache stack), github.com/jundot/omlx (2026)
