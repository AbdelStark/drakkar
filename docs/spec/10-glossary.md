# Glossary

Canonical vocabulary for the DRAKKAR specification corpus. Every document in `docs/spec/`
and `docs/rfcs/` uses these terms in exactly the senses defined here; where two source
documents phrased a concept differently, the canonical phrasing is stated and the synonym
noted. Each entry links to the specification section or RFC that defines the term
normatively. Memory figures are GiB unless noted; values marked `est.` are modeled
estimates pending measurement on the RFC-0009 fleet.

## activation watermark

The peak transient activation memory of a compiled forward graph, bounded by prefill chunk
size rather than prompt length, and charged against the memory contract as a fixed term.
Shipped defaults per architecture class are replaced by measured values on first load
(RFC-0003 IC13, RFC-0004 FE13 — [Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design),
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).

## active parameters

The parameter bytes actually read per generated token in an MoE model (routed experts plus
shared weights), as distinct from total parameters. Decode-speed estimates use active
parameters; memory sizing uses total parameters — the two figures are always tracked
separately (RFC-0003 IC22, RFC-0004 FE21 —
[Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design)).

## admission control

The scheduler gate that admits a request only if its KV need for `prompt + max_tokens`
fits the pool's free blocks plus reclaimable cache at live occupancy, computed by the same
feasibility-engine arithmetic as the preflight (RFC-0004 FE18, RFC-0007 AS14 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)). Rejections
are structured (`413 context_exceeded` with `max_admissible_tokens`, `429 kv_pool_exhausted`
with `retry_after_ms`); Metal is never the component that discovers an out-of-memory
condition (RFC-0001 invariant I2).

## alias

A curated short model name (for example `qwen3:8b`) resolving to a Hugging Face
`(repo_id, revision)`. The alias table ships inside the binary, is user-extensible, and is
refreshed only by an explicit `drakkar alias update` (LD3); on collision a user-defined
alias wins over a shipped one, with a warning (LD16) (RFC-0006 MP1 —
[Model Pipeline](../rfcs/RFC-0006-model-pipeline.md#proposed-design),
[CLI and UX](../rfcs/RFC-0008-cli-ux.md#proposed-design)).

## ANE (Apple Neural Engine)

Apple's dedicated NPU block, distinct from the GPU-core Neural Accelerators (see
[NAX](#nax-neural-accelerator)). The v1 engine does not use the ANE; hosting small draft
models on it via Core ML is a v1.x extension candidate only (RFC-0003 §Extension points,
RFC-0002 §ONNX/Core ML analysis —
[Stack Selection](../rfcs/RFC-0002-stack-selection.md#alternatives-considered)).

## artifact

A servable local model: weight files in a supported format (MLX affine, MXFP4, bf16/fp16
safetensors, or GGUF) plus tokenizer and chat-template metadata, registered in the
content-addressed store. Artifact selection is fit-driven: the resolver picks the candidate
whose effective bpw is closest to the plan without exceeding it (RFC-0006 MP4-MP5 —
[Model Pipeline](../rfcs/RFC-0006-model-pipeline.md#proposed-design)). The backend trait
consumes it as `ModelArtifact` (RFC-0001 §backend seam).

## attention sink

The first token positions of a sequence, pinned in a sliding-window layer's ring buffer so
window rotation never evicts them; enabled per model configuration (RFC-0005 KV6 —
[KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)).

## backend seam

The `InferenceBackend` trait boundary between the Rust control plane and a compute backend
(`drakkar-mlx`, `drakkar-gguf`). It is the only portability boundary in the system: nothing
above it may name Metal, MLX, or llama.cpp types (RFC-0001 A6, invariant I5 —
[Architecture](../rfcs/RFC-0001-architecture.md#proposed-design)).

## bandwidth roofline

The decode-throughput upper bound implied by memory bandwidth:
`decode_tps ≈ η_d × BW / (active_weight_bytes + kv_read_bytes(ctx))`, where `η_d` is the
calibrated kernel-efficiency factor (shipped 0.65, observed 0.6-0.85) (RFC-0004 FE21 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)). The headline
product target is decode at ≥ 85% of this roofline on M4 Pro and newer (RFC-0009 PB13,
[PRD §5](../../PRD.md#5-product-requirements) P10).

## block (KV)

The fixed-size unit of KV pool allocation: 32 tokens (build-time constant, LD7; a 16-vs-32
ablation is a named RFC-0009 v0.2 work item), storing K and V for all layers of its layout
class, contiguous per layer for coalesced reads. A block is in exactly one state: `free`,
`active(seq)`, or `cached(prefix, refcount)` (RFC-0005 KV1, KV4 —
[KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)).

## block table

The per-sequence mapping from logical token positions to physical KV blocks; paged
attention kernels read K/V through it (RFC-0005 KV3, RFC-0003 IC10). Table metadata costs
roughly 96 B per block and is charged by the fit engine (RFC-0004 FE12 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).

## bpw / effective bits-per-weight (bpw_eff)

Storage cost per parameter including quantization metadata. MLX affine at `b` bits and
group `g`: `bpw_eff = b + 32/g` (4-bit g64 → 4.5, 4-bit g32 → 5.0, 8-bit g64 → 8.5);
MXFP4 → 4.25; bf16 → 16; GGUF types use a shipped per-type table (Q4_K_M ≈ 4.85,
Q5_K_M ≈ 5.7, Q6_K ≈ 6.6, Q8_0 ≈ 8.5) (RFC-0004 FE5, RFC-0003 IC6 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).

## calibration

The `drakkar bench --calibrate` loop that measures per-chip constants — decode efficiency
`η_d`, prefill anchors per architecture class with the NAX multiplier, the
`runtime_overhead` floor, activation watermarks, and the speculation occupancy crossover —
into `~/.drakkar/calibration/<chip>.json`. The feasibility engine prefers calibrated values
over shipped defaults and labels the resulting predictions `calibrated` (RFC-0009 PB15,
RFC-0004 FE4/FE24 — [Performance](../rfcs/RFC-0009-performance.md#proposed-design)).

## capabilities

The runtime feature set a backend reports at load (`Capabilities`): NAX tensor-op
availability, supported KV quantization bits, speculative-decoding support, and similar.
Probed by functional self-test, never by version sniffing, and used to gate feature paths
and switch fit-engine constants (RFC-0001 A7, RFC-0003 IC26 —
[Architecture](../rfcs/RFC-0001-architecture.md#proposed-design)).

## chunked prefill

Processing a prompt in scheduler-controlled slices (default 512 tokens, adaptive 256-2048
by ITL pressure) interleaved with ongoing decode steps, so one long prompt never wrecks
another stream's inter-token latency (RFC-0003 IC12, RFC-0007 AS13 —
[API Server](../rfcs/RFC-0007-api-server.md#proposed-design)). Chunking also bounds the
activation watermark independently of prompt length (IC13).

## cold / warm (TTFT)

Two TTFT reporting conditions. Cold: the model is resident but no prefix is cached for the
prompt, so prefill covers the full prompt. Warm: a cached prefix is reused and prefill
covers only the uncached suffix. Load-from-disk time is always reported separately, never
folded into either (RFC-0009 PB1, RFC-0004 FE23 —
[Performance](../rfcs/RFC-0009-performance.md#proposed-design)).

## content-addressed store

The local model store: blobs under `~/.drakkar/models/blobs/sha256-*` with human-readable
manifests mapping `<org>/<repo>/<rev>` to blobs. Identical tensors dedupe across revisions
by construction, and everything under `~/.drakkar` is reconstructible — deleting it is
always safe (RFC-0006 MP10, RFC-0001 A8 —
[Model Pipeline](../rfcs/RFC-0006-model-pipeline.md#proposed-design)).

## continuous batching

Token-level scheduling in which the decode batch recomposes every step: new sequences join
as soon as their prefill completes, and finished sequences exit without draining the batch
(RFC-0007 AS12 — [API Server](../rfcs/RFC-0007-api-server.md#proposed-design)). A v0.2
milestone.

## CoW (copy-on-write)

The block-sharing discipline of the KV pool: refcounted blocks are shared read-only across
sequences, and a shared block splits — copying exactly once — only when a sequence writes
into it (a partial tail block) (RFC-0005 KV4 —
[KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)). The same principle at file level
(APFS `clonefile`) drives HF-cache interop (RFC-0006 MP11, LD4).

## decode

The bandwidth-bound generation phase: one token per sequence per step, with throughput
scaling almost linearly with memory bandwidth ([PRD §2.1](../../PRD.md#2-background-what-the-research-found-july-2026),
RFC-0004 FE21). The backend executes it step-granular — `decode(batch)` advances B
sequences by one token — keeping scheduling policy in Rust (RFC-0001 A6 —
[Architecture](../rfcs/RFC-0001-architecture.md#proposed-design)).

## dialect (API)

One of the two wire formats the server speaks: OpenAI (`/v1/chat/completions`) or Anthropic
(`/v1/messages`). Both normalize to one internal `GenerationRequest` and render back in the
caller's dialect, including its streaming envelope; unsupported dialect fields fail loud
with a `400` naming the field (RFC-0007 AS1-AS2 —
[API Server](../rfcs/RFC-0007-api-server.md#proposed-design)).

## draft model

A small model with the same tokenizer as the target (typically 0.5-1.7B at 4-bit) that
proposes k=4-8 tokens verified in a single target forward pass. The fit engine budgets its
memory explicitly, and `drakkar run --draft auto` selects one from a curated compatibility
table (RFC-0003 IC19 — [Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design)).
Draft and target share no KV state in v1 (LD9).

## engine actor

The dedicated OS thread that exclusively owns one loaded model's backend instance, KV block
pool, and Metal stream, processing a message loop (`Prefill`, `DecodeStep`, `Admit`,
`Evict`, `Snapshot`, `Unload`). Exactly one engine actor exists per resident model
(invariant I1); this confinement turns MLX's thread-safety constraint into lock-free model
state and deterministic memory accounting (RFC-0001 A2 —
[Architecture](../rfcs/RFC-0001-architecture.md#proposed-design)).

## feasibility engine / fit

The pure library (`drakkar-fit`) that answers, before any download and again at load time:
does this model fit this machine, at what quantization, with how much context, and how fast
will it feel. It is the single source of truth for memory math (invariant I3), consumed by
the CLI preflight (`drakkar fit`), `POST /fit`, and admission control
([RFC-0004](../rfcs/RFC-0004-feasibility-engine.md#summary)). "fit" is the product-facing
name for the same component; the two are used interchangeably.

## fragmentation margin

A 3% safety term added on top of weights, KV pool, activation watermark, and runtime
overhead in the total-memory model, absorbing allocator slack and accounting error
(RFC-0004 §memory model, FE16 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).

## GGUF

llama.cpp's single-file model format carrying its quantization zoo (K-quants, i-quants).
Served unmodified by the secondary `drakkar-gguf` backend (cargo feature, on by default in
release builds) as the coverage path for checkpoints with no MLX or safetensors route
(RFC-0002 D4, RFC-0003 IC7 —
[Stack Selection](../rfcs/RFC-0002-stack-selection.md#proposed-design)).

## GQA (grouped-query attention)

Attention with fewer KV heads than query heads, the dominant full-attention layout. KV cost
per token is `2 × n_layers × n_kv_heads × head_dim × bytes_elem`, the baseline formula of
architecture-aware KV accounting (RFC-0004 FE8 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).

## honest speed

The product principle: maximum performance the hardware allows, and no number shown to a
user that the engine cannot defend — every surfaced figure traces to a formula (RFC-0004)
or a benchmark (RFC-0009), carries a confidence tier, and regressions gate releases
([PRD §1](../../PRD.md#1-vision), RFC-0001 design principle 1 —
[Architecture](../rfcs/RFC-0001-architecture.md#proposed-design)).

## hybrid attention

Architectures interleaving sliding-window and global-attention layers (Gemma-family
SWA:global patterns, gpt-oss alternating layers). SWA layers contribute a fixed KV term
(window W), global layers scale with context; the fit engine MUST NOT bill such models at
the uniform rate (RFC-0004 FE9, RFC-0003 IC11, RFC-0005 KV5-KV6 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).

## ITL (inter-token latency)

The interval between consecutive streamed tokens during decode, reported as p50 and p95;
p95 is the agent-UX metric because means hide the jitter that breaks interactive feel
(RFC-0009 PB2 — [Performance](../rfcs/RFC-0009-performance.md#proposed-design)). See
[TPOT](#tpot-time-per-output-token) for the synonym.

## KV cache

The per-token attention keys and values retained across decode steps so each token's K/V is
computed once. DRAKKAR's implementation is a paged, quantization-aware, prefix-sharing
block pool with an optional SSD persistence tier
([RFC-0005](../rfcs/RFC-0005-kv-cache.md#summary)).

## KV quantization

Storing cached K/V at 8-bit or 4-bit group-quantized precision (group 64, per-head scales)
instead of fp16, applied at block granularity on write with dequantization fused into the
attention kernels — no resident fp16 shadow copy exists. Capacity effect on uniform-attention
models: roughly 2x tokens at 8-bit, 3.5x at 4-bit net of scale overhead; the final
full-attention layer stays fp16 by default on deep models (RFC-0005 KV13-KV16 —
[KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)).

## layout class

The KV storage strategy a layer belongs to: global attention (paged blocks), sliding-window
(fixed ring buffer outside the pool), MLA (paged latent vectors), or recurrent/SSM state
(constant per-sequence tensors). A model may mix classes; each is sized and cached by its
own rule (RFC-0005 KV5-KV8 — [KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)).

## memory contract

Invariant I2: `weights + kv_pool + activation_watermark + runtime_overhead <=
declared_budget` at all times, enforced by the up-front pool carve-out and admission
control, verified by `memory_report()` in debug soaks. A contract breach in benchmarking is
a hard failure, not a slow result (RFC-0001 I2, RFC-0009 PB4 —
[Architecture](../rfcs/RFC-0001-architecture.md#proposed-design)).

## metallib

A precompiled Metal shader library. MLX's shaders are embedded as metallib in the DRAKKAR
binary so no compiler toolchain is required on the user's machine at runtime (RFC-0002 D5 —
[Stack Selection](../rfcs/RFC-0002-stack-selection.md#proposed-design)).

## MLA (multi-head latent attention)

The DeepSeek-lineage attention family that stores, per layer per token, one shared latent
`(c_kv + d_rope)` vector (for example 512 + 64 = 576 elements) instead of per-head K and V.
It is a distinct cache layout with its own accounting formula, and the fit report flags it
(RFC-0004 FE10, RFC-0005 KV7 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).

## MLX

Apple's open-source, MIT-licensed array framework: lazy computation graphs,
unified-memory-native execution, hand-tuned Metal kernels, built-in quantization, and
first-to-silicon Neural Accelerator paths. DRAKKAR's primary compute substrate, vendored
and pinned per release behind the C++ shim's C ABI
([RFC-0002](../rfcs/RFC-0002-stack-selection.md#summary)).

## MoE (mixture of experts)

An architecture routing each token through a subset of expert feed-forward networks. Total
parameters determine memory; active parameters determine decode bandwidth cost — the two
are accounted separately (RFC-0003 IC22). SSD expert streaming is explicitly out of scope
for v1 (IC23 — [Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design)).

## MSRV (minimum supported Rust version)

The oldest Rust toolchain the workspace guarantees to build with, pinned per release
alongside the MLX pin (RFC-0002 D5 —
[Stack Selection](../rfcs/RFC-0002-stack-selection.md#proposed-design)).

## NAX (Neural Accelerator)

The dedicated matmul unit embedded in every GPU core of M5-family chips, programmed through
Metal 4 Tensor Operations on macOS 26.2+; it delivers the measured 3.3-4.1x prefill gain
over M4. Availability is established by a functional self-test at load, never version
sniffing; it gates the fit engine's prefill anchors and a non-waivable CI check (RFC-0003
IC26, RFC-0004 FE22, RFC-0009 PB14 —
[Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design)). Distinct from the
[ANE](#ane-apple-neural-engine).

## os_floor

The minimum un-wired system RAM every memory plan must leave for macOS: 4 GiB on ≤ 16 GB
machines, 6 GiB (24-36 GB), 8 GiB (48-64 GB), 12 GiB (96 GB+). Whichever binds first —
os_floor or the GPU budget — wins (RFC-0004 FE16 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)). An
"aggressive" profile trimming it is an explicit v1 non-goal (LD11).

## paged attention

Attention computed over KV stored in non-contiguous fixed-size blocks addressed through a
block table (vLLM lineage), eliminating contiguous-allocation fragmentation. v0.2 ships a
gather-based kernel fallback; a fused paged varlen Metal kernel is a named v0.2 performance
milestone with a prototype-both spike (LD20) (RFC-0003 IC10, RFC-0005 KV1-KV3 —
[Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design)).

## prefill

The compute-bound phase that processes prompt tokens to populate the KV cache; it
determines TTFT and is where the NAX 3-4x M5 gain lives
([PRD §2.1](../../PRD.md#2-background-what-the-research-found-july-2026)). Always executed
in chunks under scheduler control (RFC-0003 IC12 —
[Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design)).

## prefix sharing / prefix hash chain

Reuse of cached KV blocks across requests whose leading tokens are identical. Prefix
identity is a rolling hash chain over `(model_id, tokenizer_hash, chat_template_hash,
token_ids)` computed at block granularity; keys are content hashes, never names, so any
change in revision, tokenizer, template, KV precision, or rope scaling invalidates the
affected subtree (RFC-0005 KV9, KV12 —
[KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)).

## quantization group

The run of `g` consecutive weights (or KV elements) sharing one fp16 scale and bias.
Smaller groups improve quality and add `32/g` bits per weight of metadata; default g=64,
with g=32 recommended for models under ~4B (RFC-0003 IC6, RFC-0004 FE5 —
[Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design)).

## radix tree (prefix index)

The in-RAM index of cached prefixes, keyed by hash-chain segments and mapping to cached
block runs (SGLang RadixAttention lineage). On admission the scheduler queries it for the
longest cached prefix; prefill starts at the first uncached token, with partial-block
matches reused up to the block boundary (RFC-0005 KV9-KV10 —
[KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)).

## recipe (quantization)

A versioned per-model-family table naming per-tensor quantization treatment: embeddings and
lm_head bits/group, norms fp32, layers flagged sensitive held at 8-bit or bf16. The fit
estimator and the converter share the same recipe tables so predictions match produced
artifacts (RFC-0003 IC9, RFC-0004 FE6, RFC-0006 MP13 —
[Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design)).

## REPL

The interactive read-eval-print loop `drakkar run` enters: streaming output, `Ctrl-C`
cancels the current generation without exiting, and meta-commands (`/model`, `/context`,
`/stats`, `/system`, `/save`, `/load`, `/fit`, `/tools`) drive session state (RFC-0008
CLI3-CLI5 — [CLI and UX](../rfcs/RFC-0008-cli-ux.md#proposed-design)).

## roofline

A performance upper bound derived from a hardware limit rather than asserted. DRAKKAR uses
a [bandwidth roofline](#bandwidth-roofline) for decode and anchored, capability-scaled
estimates for prefill; every user-facing performance number traces to such a formula or to
a benchmark (RFC-0004 FE21-FE22, RFC-0001 design principle 1 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).

## runtime overhead

The memory floor of the engine process itself — Metal runtime, allocator, tokenizer,
process overhead — charged as a fixed term in the memory model. Shipped default 1.2 GiB,
replaced by the measured RSS floor of an empty engine on first run (RFC-0004 FE14 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).

## safetensors

The zero-copy, non-executable tensor serialization format used for HF-native weights and
for SSD-tier KV blocks. With GGUF, one of only two accepted weight formats: pickle
checkpoints are rejected as a security boundary (RFC-0001 A11, RFC-0006 MP6 —
[Model Pipeline](../rfcs/RFC-0006-model-pipeline.md#proposed-design)).

## speculative decoding (n-gram / draft)

Converting spare compute into tokens by proposing candidate continuations verified in one
target forward pass. Two v1 tiers: prompt-lookup (n-gram) speculation — zero extra memory,
default-on for agent workloads, auto-disabled per-request below an acceptance threshold —
and draft-model speculation (see [draft model](#draft-model)). The scheduler disables
per-sequence speculation above the calibrated batch-occupancy crossover (RFC-0003
IC18-IC21, RFC-0007 AS16 —
[Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design)).

## SSD KV tier

Optional disk persistence for evicted cached prefix runs, serialized as safetensors blocks
with an index sidecar under `~/.drakkar/kv-cache/`, restored after process restarts.
Eligibility requires restore to beat recompute by ≥ 3x under the calibrated cost model;
default budget 8 GiB with LRU, files mode 0600 and treated as sensitive (RFC-0005
KV17-KV19 — [KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)). Ships in v0.3.

## SWA (sliding-window attention)

Attention restricted to the most recent W tokens of context. SWA layers' KV cost is fixed
at `min(ctx, W)` per layer regardless of total context, and their state lives in
per-sequence ring buffers outside the paged pool (RFC-0004 FE9, RFC-0005 KV6 —
[KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)).

## TPOT (time per output token)

The per-token decode interval; in this corpus a synonym for
[ITL](#itl-inter-token-latency), which is the canonical term (RFC-0009 PB2 —
[Performance](../rfcs/RFC-0009-performance.md#proposed-design)).

## TTFT (time to first token)

Wall time from request submission to the first sampled token, reported separately for the
cold and warm conditions (see [cold / warm](#cold--warm-ttft)); model load-from-disk time
is never folded in (RFC-0009 PB1 —
[Performance](../rfcs/RFC-0009-performance.md#proposed-design)).

## unified memory

Apple Silicon's single RAM pool addressed zero-copy by both CPU and GPU. It is why a 64 GB
laptop runs models that exceed discrete-GPU VRAM, and why the wired-memory cap — not total
RAM — is the machine's true inference budget
([PRD §2.1](../../PRD.md#2-background-what-the-research-found-july-2026), RFC-0004 FE15).

## usable budget

The memory actually available to a plan:
`usable = budget − runtime_overhead − fragmentation_margin`, additionally constrained by
[os_floor](#os_floor); whichever constraint binds first wins. All verdicts and the context
solver compute against `usable` (RFC-0004 FE16, FE19-FE20 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).

## verdict tiers

The four feasibility outcomes: **Comfortable** (`total ≤ 0.85 × usable`), **Tight**
(`0.85 × usable < total ≤ usable`, works with a concurrent-apps warning), **Needs tuning**
(fails as requested but a remedy plan exists, ranked by expected quality impact), and
**Won't fit** (exceeds the machine even at the floor plan; the nearest fitting sibling is
suggested) (RFC-0004 FE19 —
[Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)). Won't fit
maps to CLI exit code 4 absent `--force` (RFC-0008 CLI8).

## wired memory / iogpu.wired_limit_mb

Wired memory is GPU-resident memory macOS pins un-swappably, capped near two thirds of RAM
on ≤ 36 GB machines and three quarters above; the `iogpu.wired_limit_mb` sysctl raises the
cap at runtime. The authoritative budget signal is the live
`MTLDevice.recommendedMaxWorkingSetSize` probe, never a hardcoded ratio; when a plan fits
only with a raised limit, DRAKKAR proposes the exact sysctl value with warnings, the
os_floor respected, and a revert command — and MUST NOT apply it automatically (RFC-0004
FE15, FE17 — [Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)).
