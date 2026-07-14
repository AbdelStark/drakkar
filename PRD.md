# DRAKKAR Product Requirements Document

**Status:** Draft v0.1
**Date:** 2026-07-14
**Owner:** Abdelhamid Bakhta
**Related:** RFC-0001 through RFC-0009

## 1. Vision

Every MacBook Pro sold since 2021 is a capable inference machine, and the M5 generation (Neural Accelerators in every GPU core, up to 128 GB unified memory at 614 GB/s) turned it into a serious one. Yet running a model locally still means choosing between friendly tools that hide the machine from you (Ollama, LM Studio) and expert tools that expose everything but automate nothing (mlx-lm, llama.cpp). None of them answers the first question every user actually has: **will this model run on my machine, at what context length, and how fast?**

DRAKKAR is a native, single-binary inference engine for Apple Silicon that makes feasibility a first-class product feature. You hand it a Hugging Face link. It computes, before downloading a byte, whether the model fits, what quantization it needs, what context window your RAM affords, and what TTFT and tokens/second to expect. Then it downloads, prepares, and serves the model at the hardware limit through OpenAI- and Anthropic-compatible endpoints, with a KV cache subsystem designed for the multi-request, long-context reality of agentic workloads.

The product principle: **honest speed**. Maximum performance the hardware allows, and no number shown to the user that the engine cannot defend.

## 2. Background: what the research found (July 2026)

### 2.1 Hardware

The M5 generation changed the performance envelope in two distinct ways, matching the two phases of LLM inference:

- **Prefill (compute-bound, determines TTFT).** M5-family chips embed a Neural Accelerator (dedicated matmul unit) in every GPU core, programmable through Metal 4 Tensor Operations. Apple's own MLX benchmarks measure 3.3x to 4.1x faster time-to-first-token on M5 vs M4 across Qwen3 1.7B-30B and gpt-oss-20b at a 4,096-token prompt. Exploiting this requires macOS 26.2+ and Metal 4 tensor paths; engines that miss the tensor API leave 2-3x prefill on the table (a real failure mode observed in LM Studio's bundled llama.cpp runtime on M5).
- **Decode (bandwidth-bound, determines tokens/second).** Bandwidth per chip: M4 120 GB/s, M4 Pro 273 GB/s, M4 Max 546 GB/s, M5 153.6 GB/s, M5 Pro 307 GB/s, M5 Max 460 GB/s (32-core GPU) or 614 GB/s (40-core GPU). Decode throughput scales almost linearly with this number. M5 Pro/Max ship a dual-die Fusion Architecture, up to 128 GB unified memory, and 14.5 GB/s SSDs (a 55 GB model loads in about 4 seconds).

Unified memory is the strategic asset: the GPU addresses the full RAM pool with zero copies, so a 64 GB laptop runs models that do not fit a 24-32 GB discrete GPU. macOS caps GPU-wired memory at roughly two thirds of RAM (machines up to 36 GB) or three quarters (above 36 GB); the `iogpu.wired_limit_mb` sysctl raises it at runtime. This cap, not total RAM, is the real budget, and DRAKKAR models it explicitly (RFC-0004).

### 2.2 Software landscape

The ecosystem consolidated on **MLX** (Apple's open-source, MIT-licensed array framework with hand-tuned Metal kernels) as the compute substrate for Apple Silicon:

- MLX decode throughput leads llama.cpp's Metal backend by roughly 15-40% at equal quantization in 2026 community benchmarks, and by 3-4x on M5 prefill via Neural Accelerator support (shipped with macOS 26.2).
- mlx-lm gained continuous batching in its server (late 2025), prompt caching, speculative decoding, and distributed inference over Thunderbolt; Apple dedicated a WWDC26 session to running agentic workloads against `mlx_lm.server`.
- Ollama 0.19 (March 2026) added an experimental MLX backend for Macs with 32 GB+ RAM. Docker built and contributed **vllm-metal** to the vLLM organization (March 2026): vLLM's engine and scheduler with MLX as the compute path, paged Metal attention kernels, and speculative decoding. LM Studio ships dual llama.cpp/MLX engines plus a headless mode.
- A wave of MLX-based servers appeared in the last year: vllm-mlx (continuous batching, paged KV, published benchmarks of 21-87% over llama.cpp on M4 Max), oMLX (tiered RAM/SSD KV cache, menu-bar app), mlx-serve (single Zig binary, MLX + GGUF engines, Ollama API compatibility), vMLX (adaptive quantization, SSD expert streaming for MoE), and mlx-vlm's server (KV quantization with TurboQuant, block prefix caching with a disk tier).
- On the Rust side: mistral.rs (Candle-based, PagedAttention on Metal, prebuilt binaries, 45+ architectures) is the most complete all-Rust engine, but Candle's Metal kernels measurably trail MLX on fused-kernel throughput. Rust bindings to the MLX C API exist and are active (mlx-rs, mlxrs with LM-level features).
- vLLM proper remains CUDA-first; its macOS CPU backend is 20-30x slower than Metal engines and irrelevant for this product.

**Implication.** The kernel race on Apple GPUs is effectively decided: MLX's Metal kernels, maintained by Apple and first to exploit new silicon, are the substrate to build on, not compete with. The open competitive space is one layer up: packaging (native binary vs Python environments), resource intelligence (nobody ships a real feasibility engine), agent-grade serving (KV reuse across tool-call loops), and a coherent one-command experience. That is where DRAKKAR plays. The full stack analysis and decision record is RFC-0002.

### 2.3 Where existing tools fall short

| Gap | Evidence |
| --- | -------- |
| No pre-download feasibility analysis | Users discover a model does not fit after a 40 GB download; LM Studio shows a coarse RAM range on GGUF rows only; nobody models the wired-memory cap, KV growth vs context, or predicts TTFT |
| Python packaging friction | mlx-lm, vllm-metal, vllm-mlx, vMLX all require a Python 3.12 arm64 environment; wrong-arch Python and venv drift are the top install failure class |
| KV cache amnesia across agent loops | Default mlx-lm and Ollama recompute long shared prefixes; block-level prefix reuse plus persistence exists only in newer niche servers (oMLX, mlx-vlm APC) |
| Opaque memory behavior | Tools report the model file size, not weights + KV + activation + runtime overhead against the actual GPU-wired budget; out-of-memory failures appear mid-generation |
| Fragmented API surfaces | Agent ecosystems now expect both OpenAI (`/v1/chat/completions`) and Anthropic (`/v1/messages`) shapes; support is inconsistent |

## 3. Target users

1. **The agent builder (primary).** Runs Claude Code-style coding agents, multi-agent research loops, or MCP tool servers against a local model for privacy, cost, or offline work. Needs: concurrent requests without ITL collapse, prefix reuse across tool-call turns, both API dialects, structured output that never breaks JSON.
2. **The AI engineer / power user.** Evaluates open-weight releases weekly. Needs: paste an HF link and get a verdict in seconds, correct quantization picked automatically, honest benchmark numbers, scriptable JSON output.
3. **The privacy-constrained professional.** Lawyer, clinician, security researcher on a 16-36 GB MacBook. Needs: zero-config, clear guidance on what their machine can and cannot do, graceful behavior at the memory edge, nothing leaves the device.
4. **The future desktop-app user (v1.0+).** Non-CLI users reached through a menu-bar app built on the same engine.

## 4. Goals and non-goals

### Goals (v1.0 horizon)

- G1. One-command run: `drakkar run <hf-link-or-alias>` from cold start to interactive chat, with a feasibility preflight before any download.
- G2. Best-in-class single-stream latency on Apple Silicon: match or beat mlx-lm on decode, exploit Metal 4 Neural Accelerators on M5 for prefill, and beat all incumbents on warm TTFT via prefix caching.
- G3. Agent-grade serving: OpenAI + Anthropic compatible endpoints, continuous batching, structured output, tool calling, streaming with usage accounting.
- G4. A feasibility engine that predicts memory within 7% and throughput within 20% of measured, exposed in the CLI, the API, and as machine-readable JSON.
- G5. KV cache subsystem with paged allocation, copy-on-write prefix sharing, KV quantization, and an SSD persistence tier that survives restarts.
- G6. Distribution as a signed, notarized, dependency-free universal binary (Homebrew tap plus direct download). No Python, no containers, no drivers.
- G7. Open source under MIT or Apache-2.0, developed in the open.

### Non-goals

- N1. Cross-platform support (Linux/Windows/CUDA) in v1. The architecture keeps a backend seam (RFC-0001) but Apple Silicon is the only target.
- N2. Training or fine-tuning.
- N3. Multi-tenant datacenter serving, authn/z beyond a local API key, or fairness scheduling across organizations.
- N4. A model marketplace or curation service; Hugging Face is the registry.
- N5. Image/video generation and full multimodal parity in v1 (vision input is a v1.x extension, RFC-0003 §9).
- N6. Intel Macs.

## 5. Product requirements

Detailed, testable requirements live in the RFCs; this section states the product-level contract.

### 5.1 Functional

- P1. The CLI MUST accept a model reference as a full HF URL, `org/repo`, `hf.co/org/repo`, or a curated short alias, and resolve it to a runnable artifact (RFC-0006).
- P2. Before downloading, DRAKKAR MUST display a fit report: required memory (weights, KV at requested context, overhead), the machine's GPU budget, a verdict (Comfortable / Tight / Needs tuning / Won't fit), the maximum context at the chosen KV precision, and estimated TTFT and decode speed (RFC-0004).
- P3. When a model does not fit as published, DRAKKAR MUST propose concrete remedies ranked by quality impact: a smaller official quant, on-device quantization, reduced context, KV quantization, or (opt-in, with explicit warnings) raising the wired-memory limit.
- P4. `drakkar serve` MUST expose `/v1/chat/completions`, `/v1/completions`, `/v1/models`, and `/v1/messages` (Anthropic dialect) with SSE streaming, tool calling, and JSON-schema constrained output (RFC-0007).
- P5. The server MUST support concurrent requests through continuous batching with chunked prefill so that a long prompt from one client does not stall another client's decode (RFC-0007).
- P6. The engine MUST reuse KV state across requests sharing a prefix (system prompts, conversation history, agent scaffolds) automatically, in RAM and optionally on SSD (RFC-0005).
- P7. Every command MUST offer `--json` machine-readable output with a stable schema and deterministic exit codes (RFC-0008).
- P8. `drakkar bench` MUST measure TTFT, ITL, prefill and decode throughput, peak memory, and energy per token, and MUST be able to calibrate the feasibility engine's per-chip constants (RFC-0009).
- P9. Model files MUST be stored content-addressed with resumable downloads, integrity verification, and compatibility with an existing Hugging Face cache to avoid re-downloads (RFC-0006).

### 5.2 Non-functional

- P10. Performance floors per chip class as defined in RFC-0009; headline target for the reference workload (8B dense, 4-bit, 2k prompt / 256 gen): cold TTFT under 1.0 s and decode at 85% or more of the bandwidth-derived roofline on M4 Pro and newer.
- P11. Memory safety: the engine process MUST NOT exceed its declared budget; admission control rejects requests that would, rather than letting Metal fail mid-generation.
- P12. Startup: binary launch to server-ready (model already resident) under 200 ms; model load bounded by SSD bandwidth (weights are memory-mapped).
- P13. Privacy: no telemetry by default; any future opt-in metrics are documented and off unless enabled.
- P14. Reliability: 24-hour soak at mixed load with zero leaks (RSS drift < 2% after warmup) and zero request failures.
- P15. Supported OS: macOS 15+ baseline; Neural Accelerator fast paths require macOS 26.2+ and are detected at runtime, never assumed.

## 6. Differentiation summary

| Capability | DRAKKAR | Ollama 0.19 | LM Studio | mlx-lm server | vllm-metal | mlx-serve |
| --- | --- | --- | --- | --- | --- | --- |
| Pre-download feasibility engine (memory + context + speed prediction) | Core feature | No | Coarse RAM hint | No | No | RAM range on GGUF |
| Single native binary, no Python | Yes | Yes (Go) | App bundle | No | No | Yes (Zig) |
| MLX-class kernels incl. M5 tensor ops | Yes (MLX core) | Yes (MLX backend, 32GB+ only) | Yes (MLX engine) | Yes | Yes | Yes |
| Continuous batching + chunked prefill | Yes | Partial | Yes (0.4.0+) | Yes | Yes | Yes |
| Paged KV + CoW prefix sharing + SSD tier | Yes | No | No | Prompt cache only | Paged (RAM) | Prefix cache |
| OpenAI + Anthropic APIs | Yes | OpenAI + native | Both (partial) | OpenAI | Both | Both + Ollama |
| Agent JSON contract on every CLI command | Yes | Partial | No | No | No | Partial |
| Calibrated perf prediction before download | Yes | No | No | No | No | No |

The moat is not any single row: it is the combination of native distribution, the feasibility engine, and agent-first serving on top of the fastest available kernels, with numbers the product can defend.

## 7. Success metrics

- M1. Time-to-first-token-ever: fresh machine, `brew install` to first generated token for an 8B model, under 5 minutes on a 300 Mbps connection (download-dominated).
- M2. Fit-report accuracy: predicted peak memory within 7% of measured on the RFC-0009 model matrix; predicted decode within 20%, TTFT within 30% (cold), tightening after `bench --calibrate`.
- M3. Performance: meet all Tier-1 targets in RFC-0009 §5 on M4 Pro, M4 Max, M5, M5 Max reference machines; never regress more than 3% release-over-release (CI gate).
- M4. Agent workload: 4 concurrent Claude-Code-style sessions against one 30B-A3B model on M4 Max sustain per-stream ITL under 45 ms with warm-prefix TTFT under 500 ms.
- M5. Adoption (12 months post v0.2): 10k GitHub stars, 3 documented integrations (an agent framework, an editor extension, an MCP client), and DRAKKAR cited as a supported backend by at least one major agent tool.
- M6. Quality: crash-free rate above 99.9% of sessions (from opt-in crash reports only).

## 8. Roadmap

| Phase | Target | Scope |
| ----- | ------ | ----- |
| v0.1 "First light" | +10 weeks | CLI (`run`, `pull`, `ls`, `rm`, `fit`, `doctor`), MLX engine shim, single-request generation, streaming OpenAI chat endpoint, MLX-community models, fit engine v1 (memory + max context) |
| v0.2 "Convoy" | +10 weeks | Continuous batching, paged KV with prefix CoW, KV quantization, Anthropic `/v1/messages`, tool calling, JSON-schema output, speculative decoding, `bench` + calibration, GGUF coverage backend |
| v0.3 "Fleet" | +8 weeks | Daemon mode (launchd), multi-model pool with LRU/TTL, SSD KV tier, embeddings endpoint, MCP server mode, `convert`/on-device quantization UX polish |
| v1.0 "Harbor" | +12 weeks | Menu-bar desktop app (SwiftUI shell over the engine's C ABI), auto-update, signed installers, hardware fleet CI, docs site |
| v1.x | Exploratory | Vision-language input, distributed inference over Thunderbolt 5, ANE offload for small/draft models, Linux backend seam |

## 9. Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
| ---- | ---------- | ------ | ---------- |
| MLX API or internals shift under us | Medium | High | Pin MLX per release; vendor via a thin C++ shim exposing our own stable C ABI (RFC-0002 §6); upstream fixes; the shim isolates 100% of MLX surface |
| mlx-c / binding gaps lag new MLX features (tensor-op paths, batch APIs) | Medium | Medium | Shim links MLX C++ directly, not mlx-c, so we consume features the day they land |
| Competitors close the feasibility gap | Medium | Medium | Ship calibration + prediction accuracy as a measurable, marketed number; depth of the memory model (RFC-0004) is months of work to replicate |
| Model architecture churn (MLA, hybrid SSM, new MoE routing) | High | Medium | Architecture-aware KV accounting from day one (RFC-0004 §4); GGUF coverage backend as escape hatch; model-def layer designed for weekly additions |
| macOS fragmentation (NAX needs 26.2+) | Certain | Low | Runtime capability detection; fit engine reports which fast paths are active; graceful non-NAX path is still MLX-fast |
| Wired-limit guidance harms a user's system | Low | High | Never auto-apply; explicit warnings, conservative floors, one-command revert, documented as unsupported-by-Apple (RFC-0004 §6) |
| Solo/small-team bandwidth vs scope | High | High | Phases are independently shippable; v0.1 is deliberately narrow; GGUF backend and desktop app are cut lines, not commitments |

## 10. Open questions

1. Final name and trademark clearance (DRAKKAR is a working codename; conflicts to screen include fragrance and gaming uses).
2. License: MIT (ecosystem-maximal) vs Apache-2.0 (patent grant). Leaning Apache-2.0 for the engine, MIT for client SDKs.
3. Should v0.2 include `/v1/responses` (OpenAI Responses API) given growing agent adoption, or defer to v0.3?
4. Default model alias set: curate 10-15 aliases (`qwen3:8b`, `gpt-oss:20b`, ...) or resolve everything through HF search?
5. Whether to share the Hugging Face hub cache by default (saves disk, couples us to HF layout) or link into it read-only (RFC-0006 §7).

## References

- Apple, "Exploring LLMs with MLX and the Neural Accelerators in the M5 GPU", machinelearning.apple.com/research/exploring-llms-mlx-m5 (Nov 2025)
- Apple Newsroom, "Apple debuts M5 Pro and M5 Max" and MacBook Pro tech specs (Mar 2026), apple.com
- Apple WWDC26 session 232, "Run local agentic AI on the Mac using MLX"
- vllm-project/vllm-metal (GitHub, v0.2.0 Apr 2026); Docker blog, "Docker Model Runner adds vLLM support on macOS" (Mar 2026)
- Barrios et al., "Native LLM and MLLM Inference at Scale on Apple Silicon", arXiv:2601.19139 (Jan 2026)
- Ollama 0.19 MLX backend coverage (Mar 2026); LM Studio 0.4.0 release notes (Jan 2026)
- jundot/omlx, ddalcu/mlx-serve, Blaizzy/mlx-vlm, EricLBuehler/mistral.rs, oxideai/mlx-rs (GitHub, 2026)
- llama.cpp discussion #2182 and community documentation of `iogpu.wired_limit_mb` and `recommendedMaxWorkingSetSize`
