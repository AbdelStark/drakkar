# RFC-0003: Inference Core

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.1

## Summary

The inference core is the `drakkar-mlx` backend: model graph construction, Metal execution, quantized compute, sampling, and speculative decoding, built on the MLX core per [RFC-0002](RFC-0002-stack-selection.md#proposed-design). This RFC specifies execution semantics, the precision/quantization matrix, the token pipeline, and the Metal-level memory and capability handling. The KV pool it plugs into is [RFC-0005](RFC-0005-kv-cache.md#proposed-design); who gets scheduled when is [RFC-0007](RFC-0007-api-server.md#proposed-design). Requirements are numbered IC1-IC27; acceptance criteria AC1-AC5 are folded into [Testing Strategy](#testing-strategy).

## Motivation

PRD G2 ([PRD §4](../../PRD.md#4-goals-and-non-goals)) commits DRAKKAR to best-in-class single-stream latency on Apple Silicon: match or beat mlx-lm on decode, exploit the Metal 4 Neural Accelerators (NAX) on M5-family chips for prefill, and beat incumbents on warm TTFT. This RFC is where that commitment is either implemented or lost, because the two phases of inference stress the hardware in opposite ways ([PRD §2.1](../../PRD.md#21-hardware)):

- **Prefill is compute-bound.** M5-family chips embed a dedicated matmul unit in every GPU core, programmable through Metal 4 Tensor Operations; Apple's own MLX measurements show 3.3x-4.1x TTFT gains on M5 vs M4 at a 4,096-token prompt. Engines that miss the tensor-op path leave 2-3x prefill on the table — an observed failure mode, not a hypothetical (a shipped llama.cpp-based runtime failed its tensor check silently on M5 hardware). The inference core must hit the NAX matmul route on capable hardware (IC12) and must detect capability by functional self-test, never version sniffing (IC26).
- **Decode is bandwidth-bound.** Tokens/second scales almost linearly with memory bandwidth (120-614 GB/s across the M4/M5 range). Every wasted byte moved per step — un-fused elementwise chains, full-logit readbacks, redundant synchronization — comes directly out of the user-visible number. Hence lazy fused graphs (IC1-IC2), on-GPU sampling with minimal readback (IC4, IC14), and speculation that converts spare compute into tokens when bandwidth is the binding constraint (IC18-IC21).

Secondary motivations from the PRD: agent builders need structured output that never breaks JSON (PRD P4 and target user 1), which requires mask-based constrained decoding rather than best-effort retries (IC16); and the memory-safety contract (PRD P11, RFC-0001 I2) requires the backend to enforce MLX memory limits from the declared budget rather than trusting Metal to fail gracefully (IC25).

## Goals

- Decode throughput within 5% of mlx-lm at equal model/quant/machine, single stream (AC1).
- M5-family prefill demonstrates the tensor-op path: ≥ 3x M4-class prefill tokens/s at a 4k prompt on the 8B reference (AC2), with the path capability-gated and self-tested at load (IC26), never assumed (PRD P15).
- A fully on-GPU sampling pipeline with per-request counter-based RNG seeds, composable logit processors in a fixed order, and grammar-mask constrained output that is schema-valid by construction: 100% validity on the structured-output corpus at ≤ 8% ITL overhead at batch 1 (AC3, IC14-IC16).
- Native serving of the weight-format matrix in IC6, including streaming on-device quantization whose peak memory stays ~1 shard above output size so it works on machines that cannot hold the fp16 model (IC8).
- Speculative decoding tiers that never make things worse: n-gram path ≥ 1.2x on the agent-trace corpus with no workload regressing more than 3% (AC4, IC18); draft-model speculation budgeted explicitly by the fit engine (IC19).
- Bounded prefill memory: activation watermark a function of chunk size, not prompt length, charged against the budget as `activation_watermark` (IC13, RFC-0004 §5).
- Backend enforcement of the memory contract (RFC-0001 I2) via MLX memory/cache limits set from the declared budget, observable through `memory_report()` (IC25).
- 24-hour soak stability: zero engine-thread panics, RSS drift < 2% post-warmup (AC5).

## Non-Goals

- **Expert offload / SSD expert streaming for MoE** (IC23). It trades the product's latency promise for capacity; the fit engine recommends models that fit instead. Revisit only on strong user demand.
- **Training or fine-tuning** (PRD N2). The backend builds forward graphs only.
- **Vision input in v1** (PRD N5). Image embeddings feeding the paged cache are a v1.x extension point (§9 below); nothing in v1 blocks them.
- **Self-speculative methods** (MTP heads, EAGLE-style) as shipped features; the IC19 verification path is built generic so they slot in as v1.x research items (IC20).
- **A strict-determinism mode in v1.** Per LD6, v1 documents "reproducible given identical batch schedule"; a batch-stable-reduction-order mode at reduced throughput is v1.x (see [Open Questions](#open-questions)).
- **CPU inference paths or non-Metal GPU backends.** Backend B (GGUF via llama.cpp) is scoped in RFC-0002 and [RFC-0010](RFC-0010-backend-abi.md#proposed-design); this RFC specifies Backend A only, except where IC7 names the split.

## Proposed Design

The subsections keep their source numbering; other RFCs cite them by § number.

### 2. Execution model

- IC1. Computation uses MLX's lazy graphs: a decode step builds the full forward + sampling graph and evaluates once, minimizing kernel-launch and synchronization overhead (the documented weakness of un-fused Metal stacks).
- IC2. The hot loops (decode step per batch shape bucket, prefill chunk) are wrapped in MLX graph compilation to fuse elementwise chains (norm/rope/activation epilogues). Compiled variants are cached per (architecture, batch-bucket, dtype) key; shape buckets are powers of two up to `max_concurrency` to bound compilation churn.
- IC3. All GPU work is issued from the engine actor thread (RFC-0001 A2) on a single MLX stream; a second low-priority stream MAY be used for KV block quantize/dequantize and SSD-tier staging where overlap is measured to help (guarded by a flag until proven).
- IC4. Async evaluation: the actor issues step N+1's graph while draining step N's sampled tokens, keeping the GPU fed. Token readback is a single small transfer per step (sampled ids + optional logprobs), never full logits, except when the API requests top-k logprobs (then top-k is computed on-GPU and only k values cross).

### 3. Numerics and quantization

- IC5. Compute dtype: bf16 activations by default (fp16 fallback pre-M2 if measured faster); fp32 accumulation inside matmul/attention kernels as provided by MLX; RMSNorm epsilon and rope theta honored per model config.
- IC6. Weight formats supported natively (Backend A):

| Format | bits/weight (effective) | Source | Notes |
| --- | --- | --- | --- |
| MLX affine, group 64 | 4.5 / 5.5 / 6.5 / 8.5 | mlx-community repos or on-device convert | default serving format; group scale+bias fp16 adds 0.5 bpw at g=64 |
| MLX affine, group 32 | +0.5 bpw vs above | convert flag | quality bump for small models (< 4B) where 4-bit g64 degrades |
| MXFP4 | 4.25 | native checkpoints (gpt-oss) | pass-through, no conversion |
| bf16 / fp16 | 16 | HF safetensors | direct serve when it fits |
| NVFP4 | 4.x | upstream MLX support tracking | adopt when MLX lands it stably (Ollama 0.19 precedent) |

- IC7. Backend B (llama.cpp) serves the GGUF quant zoo (K-quants, i-quants) unmodified; the fit engine carries per-family bpw tables for both formats (RFC-0004 §3, [Feasibility Engine](RFC-0004-feasibility-engine.md#proposed-design)).
- IC8. On-device quantization ("fit-to-machine"): when only fp16/bf16 weights exist, the model manager quantizes at pull time to the fit engine's recommended bits/group (RFC-0006 §6, [Model Pipeline](RFC-0006-model-pipeline.md#proposed-design)). Quantization is per-tensor streaming (load shard, quantize, write, drop) so peak memory stays ~1 shard above the output size, and MUST be feasible on machines that cannot hold the fp16 model.
- IC9. Sensitivity guardrails: embeddings and lm_head follow the model-family recipe (default: quantize at the same bits, group 32); layers flagged sensitive by the recipe (for example the final full-attention block in deep hybrids) stay at 8-bit or bf16. Recipes live in the model-def layer and are versioned.

### 4. Attention and prefill

- IC10. Attention uses MLX fused scaled-dot-product attention with GQA support; the paged variant reads K/V through the block table supplied by RFC-0005 (gather-based fallback first, fused paged kernel as the optimization milestone; vllm-metal's unified paged varlen kernel is the design reference and upstreaming target).
- IC11. Sliding-window and hybrid-attention models (Gemma-family interleaved SWA:global, gpt-oss alternating) are first-class: SWA layers use fixed ring buffers, global layers use paged cache, per RFC-0005 §3. MLA-style latent KV (DeepSeek lineage) is a distinct cache layout with its own accounting (RFC-0004 §4).
- IC12. Chunked prefill: prompts are processed in scheduler-controlled chunks (default 512 tokens, adaptive 256-2048 by ITL pressure, RFC-0007 §6). The kernel path for chunks MUST hit the tensor-op (NAX) matmul route on capable hardware; this is where the 3-4x M5 gain lives, and a regression test pins it ([RFC-0009 PB14](RFC-0009-performance.md#proposed-design)).
- IC13. Long-context prefill activation watermark is bounded by chunk size, not prompt length: peak extra memory ≈ f(chunk, hidden, layers) and is charged against the budget as `activation_watermark` (RFC-0004 §5).

### 5. Sampling pipeline

- IC14. Fully on-GPU per batch step: temperature, top-k, top-p, min-p, repetition/frequency/presence penalties (penalty state per sequence on-device), seed-per-request via counter-based RNG for reproducibility, then sample. Greedy short-circuits.
- IC15. Logit processors are a small composable set evaluated in fixed order: penalties -> logit_bias -> grammar mask -> temperature -> truncation (top-k/p/min-p) -> sample. Adding a processor requires a benchmark showing < 2% ITL cost at batch 8.
- IC16. Structured output: llguidance compiles JSON Schema / regex / lark grammars to token masks in Rust; the mask (bitset over vocab) uploads per step only for constrained requests and applies as the grammar-mask stage. Constrained requests MUST guarantee valid output (mask, not retry).
- IC17. Stop handling in Rust on the streamed tokens: stop strings across token boundaries, max_tokens, EOS per template; tool-call and reasoning-content markers parsed incrementally per model family (RFC-0007 §5).

Determinism semantics (LD6, resolved): given the same model, quantization, seed, and an identical batch schedule, sampled output is reproducible. Continuous batching makes batch composition — and therefore reduction order — schedule-dependent, so v1 documents "reproducible given identical batch schedule" in the API and CLI docs; a strict-determinism mode (batch-stable reduction order at reduced throughput) is deferred to v1.x.

### 6. Speculative decoding

Decode is bandwidth-bound on this hardware, so speculation converts spare compute into tokens. Three tiers, all behind `Capabilities` and off unless they win in calibration:

- IC18. **Prompt-lookup (n-gram) speculation**, default-on for agent/RAG workloads: propose continuations matching recent context n-grams; zero extra memory; verified in the same batch step. Typical 1.2-1.8x on repetitive/tool-heavy text; never slower than 3% off-path (auto-disables per-request if acceptance < threshold).
- IC19. **Draft-model speculation**: a small same-tokenizer model (0.5-1.7B at 4-bit) drafts k=4-8 tokens verified in one target forward. The fit engine budgets the draft explicitly; `drakkar run --draft auto` picks a compatible draft from a curated table. Expected 1.5-2.2x single-stream on 30B+ targets (est., calibrate per [RFC-0009](RFC-0009-performance.md#proposed-design)).
- IC20. Self-speculative methods (MTP heads, EAGLE-style) are v1.x research items; the verification path in IC19 is built generic (verify B draft streams) so they slot in.
- IC21. Speculation composes with batching: the scheduler disables per-sequence speculation when batch occupancy already saturates bandwidth (crossover measured, stored in calibration).

Per LD9, draft and target share no KV state; each model owns its cache ([RFC-0005](RFC-0005-kv-cache.md#proposed-design)).

### 7. MoE execution

- IC22. MoE layers use MLX grouped/gathered expert matmul; router in bf16. Active-parameter accounting (for bandwidth-derived speed estimates) and total-parameter accounting (for memory) are separate figures fed to RFC-0004.
- IC23. Expert-offload ("stream experts from SSD") is explicitly out of scope for v1: it trades the product's latency promise for capacity (vMLX Smelt / Anemll flash-moe demonstrate feasibility and its throughput cost). The fit engine instead recommends models that fit. Revisit if user demand is strong.

### 8. Metal and platform specifics

- IC24. Weights load via mmap from the content store; MLX/Metal residency keeps them wired for the model lifetime. Load time budget is SSD-bandwidth-bound (14.5 GB/s on M5-gen MBP: ~4 s for 55 GB).
- IC25. The backend sets MLX's memory and cache limits from the declared budget at load: hard memory limit = budget, allocator cache limit sized so steady-state RSS stays within contract (I2). `memory_report()` exposes actual vs contract, plus Metal's `recommendedMaxWorkingSetSize` and the active wired limit for the doctor/fit surfaces.
- IC26. Capability probe at load: chip identity, GPU core count, bandwidth class, macOS version, Metal 4 tensor-op availability (functional self-test, not version sniffing, per the LM Studio M5 incident where shipped binaries failed the tensor check silently). Results flow into `Capabilities` and the fit engine's constants.
- IC27. Thermal/power: `drakkar bench` samples powermetrics for tokens/joule; the engine itself does not throttle (macOS does), but sustained-vs-burst throughput is reported separately (RFC-0009 §4) because fanless/14-inch chassis diverge.

### 9. Extension points (v1.x)

Vision input (image embeddings feeding the same paged cache, vision-encoder output caching keyed by image hash per vllm-mlx's 28x finding); embeddings models (mean-pool path, batch-oriented); ANE-hosted draft models via Core ML for IC19 on low-power targets.

## Alternatives Considered

- **Eager per-op execution instead of lazy compiled graphs.** Simpler mental model and easier debugging, but each decode step would issue hundreds of small kernel launches with per-launch and synchronization overhead — exactly the documented weakness of un-fused Metal stacks that MLX's lazy evaluation and graph compilation exist to fix (PRD §2.2: Candle's Metal kernels measurably trail MLX on fused-kernel throughput). Rejected; IC1-IC2 adopt lazy graphs with cached compiled variants, paying a bounded compilation-churn cost instead (see [Drawbacks](#drawbacks)).
- **CPU-side sampling instead of the on-GPU pipeline.** Sampling on the host would allow arbitrary Rust logit processors without benchmark gates, but requires reading the full logits tensor back every step (vocab 32k-256k × batch × 2 bytes), a per-step synchronization stall that grows with batch size and directly inflates ITL. Rejected; IC14 keeps the whole pipeline on-GPU and IC15 gates processor additions on a < 2% ITL benchmark at batch 8.
- **Retry-based structured output (sample, validate, resample on failure).** Cheaper per unconstrained step and no mask machinery, but it provides no validity guarantee — a model can emit unparseable output indefinitely — and its worst-case latency is unbounded, which is disqualifying for the agent-builder contract "structured output that never breaks JSON" (PRD §3). Rejected; IC16 mandates mask-based constrained decoding: valid by construction, cost bounded and measured (AC3).
- **Full-logits readback for pipeline flexibility.** On unified memory there is no PCIe hop, so readback is sometimes dismissed as free. It is not: the transfer still consumes the same memory bandwidth that bounds decode, and forcing evaluation of the full logits tensor defeats graph fusion of the sampling epilogue. Rejected; IC4 fixes the contract that only sampled ids (plus optional logprobs) cross per step, and when the API requests top-k logprobs, top-k is computed on-GPU so only k values cross.

## Drawbacks

- **Compiled-graph bucket churn.** Caching compiled variants per (architecture, batch-bucket, dtype) means recompilation stalls when occupancy crosses a power-of-two bucket boundary and padding waste inside a bucket. `max_concurrency` bounds the variant count, but the first request at each new bucket pays a compilation pause; mitigation (background pre-compilation of adjacent buckets) is a scheduler concern noted in [RFC-0007](RFC-0007-api-server.md#proposed-design).
- **bf16-first numerics.** The baseline and all parity fixtures assume bf16 activations. The pre-M2 fp16 fallback in IC5 is conditional ("if measured faster") and carries its own numerics nuances (narrower exponent range, different overflow behavior in long chains); fixture tolerances must be maintained per compute dtype, doubling the parity-testing surface for a shrinking hardware population.
- **Grammar-mask upload cost per constrained step.** The vocab bitset (~19 KiB at a 152k vocabulary) uploads every step for every constrained request. AC3 bounds the batch-1 cost at ≤ 8% ITL, but the cost scales with the constrained share of a batch, and mask compilation for pathological schemas can stall a request's first step.
- **Single-actor, single-stream execution** (IC3) makes GPU occupancy trivially reasonable but serializes all backend work through one command path; overlap opportunities (KV quantize/dequantize, SSD staging) are locked behind a measured-benefit flag rather than exploited by default.

## Migration / Rollout

- **v0.1 "First light" (this RFC's target).** IC1-IC17 land, with one carve-out: the paged variant in IC10 is not built yet — v0.1 serves single-request generation from a contiguous per-sequence KV allocation, and the attention path is plain MLX fused SDPA with GQA. The NAX tensor-op prefill route ships capability-gated per IC26 (functional self-test at load; non-NAX machines take the standard MLX path, PRD P15). The grammar-mask stage (IC16) lands in the engine in v0.1; the public JSON-schema API surface wires up in v0.2 per [RFC-0007](RFC-0007-api-server.md#proposed-design).
- **v0.2 "Convoy".** Paged attention arrives with the RFC-0005 block pool: gather-based fallback first, fused paged varlen kernel as a named v0.2 performance milestone resolved by the LD20 prototype-both spike (see [Open Questions](#open-questions)). KV quantization integrates on the (optional, flag-guarded) second stream per IC3 and RFC-0005 §5. Speculative decoding IC18-IC21 ships behind `Capabilities`, off unless calibration wins (IC18's auto-disable and IC21's occupancy crossover are part of the same gate). MoE execution IC22 lands with the 30B-A3B reference model. Backend B (IC7) ships as the GGUF coverage backend (cargo feature `drakkar-gguf`, LD24).
- **v0.3 "Fleet".** The second low-priority stream's SSD-staging use (IC3) activates with the SSD KV tier; the embeddings extension point (§9) gets its mean-pool batch path for the embeddings endpoint.
- **v1.0 "Harbor".** No new inference-core features; AC1-AC5 targets lock to measured values on the hardware fleet CI (RFC-0009 PB16), and `est.` figures in IC19 are replaced by calibrated numbers.
- **v1.x.** Extension points in §9; self-speculative methods (IC20); strict-determinism mode (LD6); expert offload reconsidered only on demand (IC23).

Feature flags: second-stream overlap (IC3), each speculation tier (IC18/IC19), and the fp16 fallback (IC5) are individually flaggable; NAX is not a flag but a probed capability (IC26). No on-disk schema is owned by this RFC; compiled-graph caches are in-memory only and carry no cross-version compatibility obligation.

## Testing Strategy

Release acceptance criteria (from the source RFC, gating as written):

- AC1. Decode throughput within 5% of mlx-lm on the RFC-0009 matrix (same model, quant, machine), single stream.
- AC2. M5-family prefill demonstrates the tensor-op path: ≥ 3x M4-class prefill tokens/s at 4k prompt on the 8B reference (matching Apple's published envelope).
- AC3. Constrained JSON output: 100% schema-valid across the structured-output test corpus, ≤ 8% ITL overhead at batch 1.
- AC4. Speculation: n-gram path ≥ 1.2x on the agent-trace corpus with no workload regressing > 3%.
- AC5. Soak: 24 h mixed load, zero engine-thread panics, RSS drift < 2% post-warmup.

Named test suites behind them:

- **Unit.** `nax_self_test`: the IC26 functional tensor-op self-test as a standalone unit (runs a known matmul through the tensor-op route and checks both the numeric result and that the fast path was actually taken); `sampler_stage_order`: each IC15 processor applied alone and in the fixed composition, asserting order-dependence cases (penalty-then-bias vs bias-then-penalty differ; the fixed order wins); `stop_string_boundaries` (IC17): stop strings split across token boundaries, multi-byte UTF-8 splits, EOS-vs-stop precedence.
- **Golden-fixture.** `logit_parity_<arch>`: per-architecture logit-parity fixtures vs mlx-lm at a fixed seed — same model, quant, and prompt; asserts top-k logit agreement within a per-dtype tolerance band (bf16 baseline; fp16 fallback gets its own band per IC5). One fixture per supported architecture family (dense GQA, SWA hybrid, MLA, MoE), refreshed only when the MLX pin changes.
- **Determinism (LD6).** `greedy_determinism`: greedy decode of a fixed prompt, single stream, repeated N times and across process restarts, MUST produce byte-identical token streams. `seeded_reproducibility`: same seed + same batch schedule reproduces sampled output exactly; the test constructs the schedule explicitly so the documented contract ("reproducible given identical batch schedule") is what is tested, no more.
- **Statistical.** `sampler_chi_square`: at high temperature over a small synthetic vocabulary, sampled frequencies vs the softmax-expected distribution under a chi-square test at a pre-registered significance level; run per truncation mode (top-k, top-p, min-p) and with penalties active, seeded so CI failures reproduce.
- **Grammar validity corpus.** `grammar_mask_corpus` (AC3): a fixed corpus of JSON Schemas (nested objects, enums, string patterns, numeric ranges), regexes, and lark grammars; every constrained generation MUST parse and validate — 100%, no flake budget. Includes adversarial schemas (deeply nested, large enums) to bound mask-compilation stalls.
- **Property.** `streaming_quant_memory` (IC8): on-device quantization of a synthetic sharded model asserts peak RSS ≤ output size + 1 shard + fixed slack; `quant_roundtrip_error` (IC6/IC9): per-format quantize/dequantize error bounds per group size, with sensitivity-recipe layers held at their pinned precision.
- **Integration.** `chunked_prefill_equivalence` (IC12/IC13): chunked vs single-shot prefill of the same prompt yields parity-tolerance-equal logits and an activation watermark bounded by f(chunk), independent of prompt length; `memory_contract_report` (IC25): `memory_report()` actual vs contract checked continuously during soaks (RFC-0001 I2).
- **Soak.** AC5 as written, on the mixed A-E workload set of [RFC-0009](RFC-0009-performance.md#proposed-design).
- **CI regression gate.** The seeded-regression check of RFC-0009 PB14: every Tier-1 fleet run re-executes the NAX self-test and the B-workload prefill ratio; a silent NAX-path failure fails the gate non-waivably (the incident class this exists to prevent is a shipped binary quietly losing 3x prefill). AC1/AC2/AC4 numbers feed the > 3% release-over-release regression gate (PB16).

## Open Questions

- OQ1 (kept open; LD20). Fused paged-attention Metal kernel: write in-house against MLX's custom-kernel API, or adopt/upstream vllm-metal's unified paged varlen kernel? Owner: abdelstark. Resolution path: v0.2 ships the gather-based fallback regardless; a prototype-both spike during v0.2 builds a minimal version of each, measured on the RFC-0009 harness, and the decision is made on maintenance cost at comparable performance. Target: v0.2 performance milestone.

Resolved since draft: the determinism question (source OQ2) is closed per LD6 — v1 documents "reproducible given identical batch schedule"; a strict-determinism mode (batch-stable reduction order, reduced throughput) is scheduled v1.x. The contract text lives in [§5](#5-sampling-pipeline).

## References

- [PRD](../../PRD.md) — G2, §2.1 (hardware), P4/P10/P11/P15
- [RFC-0001: Architecture](RFC-0001-architecture.md) (A2 engine actor, I2 memory contract), [RFC-0002: Stack Selection](RFC-0002-stack-selection.md) (MLX core decision), [RFC-0004: Feasibility Engine](RFC-0004-feasibility-engine.md), [RFC-0005: KV Cache](RFC-0005-kv-cache.md), [RFC-0006: Model Pipeline](RFC-0006-model-pipeline.md), [RFC-0007: API Server and Scheduler](RFC-0007-api-server.md), [RFC-0009: Performance](RFC-0009-performance.md) (PB14, PB16), [RFC-0010: Backend ABI](RFC-0010-backend-abi.md)
- Apple MLR M5/MLX study (TTFT and generation tables, TensorOps + Metal Performance Primitives), Nov 2025
- mlx-lm: batch generation and server batching (Dec 2025), prompt cache, speculative decoding; WWDC26 session 232
- vllm-metal v0.2.0 release notes (unified paged varlen Metal kernel; MTP/draft speculation)
- mlx-vlm server docs: KV quantization schemes incl. TurboQuant, last-layer sensitivity note (2026)
- llguidance (grammar-constrained decoding); mistral.rs ISQ design
- vMLX Smelt / Anemll flash-moe (SSD expert streaming trade-offs), 2026
