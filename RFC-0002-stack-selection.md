# RFC-0002: Technology Stack Selection

**Status:** Draft (decision proposed)
**Author:** A. Bakhta
**Created:** 2026-07-14
**Requires:** RFC-0001

## 1. Summary

Decision: **a Rust control plane over a vendored MLX compute core, joined by a thin C++ shim that exposes a stable C ABI, with an optional llama.cpp backend for GGUF coverage.** This is the only combination that simultaneously satisfies the four hard constraints: peak kernel performance on Apple GPUs (including M5 Neural Accelerators), single-binary native distribution, full control over scheduling and KV memory policy, and a maintenance burden a small team can carry.

## 2. Constraints the stack must satisfy

- S1. **Kernel performance ceiling.** Must reach the best demonstrated decode and prefill throughput on Apple Silicon, including Metal 4 TensorOps on M5-family GPUs (3.3-4.1x prefill measured by Apple with MLX). A stack that structurally trails the leader by >15% fails the product's reason to exist.
- S2. **Distribution.** One signed, notarized arm64 binary. Anything requiring the user to have Python, a venv, or a container fails (this is the top failure class in the incumbent Python stacks and the core UX differentiator).
- S3. **Control.** We must own the scheduler, admission control, KV block allocator, prefix cache, and sampling pipeline; a framework that owns these itself (and fights us for them) is a liability.
- S4. **Velocity and safety.** Memory-safe systems language for the 80% of the code that is not kernels; strong async/HTTP/serialization ecosystem; hiring/contributor pool.
- S5. **Model coverage.** New open-weight architectures land weekly (2026 cadence: hybrid attention, MLA, new MoE routers). Adding an architecture must be days, not weeks, and there must be an escape hatch for the long tail.
- S6. **License compatibility** with an MIT/Apache-2.0 product.

## 3. Candidate analysis

### 3.1 MLX core (C++/Metal), consumed natively

Apple's array framework: lazy computation graphs, unified-memory-native, hand-tuned Metal kernels (steel GEMM, fused SDPA, quantized matmul), quantization built in (affine 4/5/6/8-bit, MXFP4), `mx.compile` graph fusion, and, decisively, first-to-silicon support: Neural Accelerator paths via Metal 4 TensorOps and Metal Performance Primitives shipped with macOS 26.2, delivering the measured 3.3-4.1x TTFT gain on M5. C and C++ APIs are official; Swift and Python fronts share the same core. MIT licensed. Ecosystem gravity is overwhelming in 2026: Ollama's new backend, vllm-metal, LM Studio's MLX engine, and roughly 4,800 pre-quantized models under mlx-community all sit on this core. Community benchmarks put MLX 15-40% ahead of llama.cpp Metal decode at equal quantization, with the gap widening on M5.

Weaknesses: the polished LM tooling (mlx-lm) is Python; consuming the core natively means implementing model graph definitions, weight loading, and generation loops ourselves in C++/Rust (mlx-lm is the reference, and the mlxrs crate demonstrates the port is tractable). The C API (mlx-c) historically lags core features by weeks.

### 3.2 Python stack: fork mlx-lm or vllm-metal

Fastest possible MVP: mlx-lm already has continuous batching, prompt caching, speculative decoding, and every architecture. vllm-metal (contributed by Docker to the vLLM org, March 2026) adds vLLM's scheduler, paged Metal attention (v0.2.0), and MTP/draft speculative decoding on MLX.

Fails S2 outright (Python 3.12 arm64 environment; vllm-metal compiles vLLM core from source via clang++ at install). Fails the differentiation test: we would be the fifth Python MLX server of 2026 (mlx-lm, vllm-metal, vllm-mlx, vMLX, oMLX), competing on their terms. The GIL and Python object overhead also tax the scheduler exactly where agentic concurrency needs headroom. **Verdict: reference material, not a base.** vllm-metal's paged varlen Metal kernel and vllm-mlx's published batching results are design inputs for RFC-0005/0007.

### 3.3 llama.cpp (C/C++), as the base

Mature Metal backend, upstream Metal 4 tensor-API support on M5 (2-3x prefill, PR #16634), GGUF's unmatched quantization zoo (K-quants, i-quants) and model coverage, a capable server with continuous batching, grammars, and slot save/restore. MIT. Single-binary friendly.

But: decode trails MLX by 15-40% on Apple Silicon in 2026 measurements and Apple will keep tuning MLX for silicon we have not seen; GGML's scheduler and server own the layers we need to control (S3), so we would fork-and-fight; and GGUF-only intake adds a conversion step against the HF-native flow. **Verdict: wrong primary, ideal secondary.** As an embedded library behind our backend trait, it costs little and buys the long tail of GGUF-only checkpoints.

### 3.4 mistral.rs / Candle (all-Rust)

The strongest all-Rust option: mistral.rs ships PagedAttention on Metal, prefix caching, in-situ quantization, an OpenAI server, MCP client, 45+ architectures, prebuilt Metal binaries. One language end to end, memory safety everywhere, aligned with our control plane.

The blocker is S1: Candle's Metal kernels measurably trail MLX where it counts. Public head-to-heads attribute the gap to kernel-launch overhead and missing fusion (multiple launches where MLX runs one fused kernel), and no Candle Neural Accelerator path exists as of mid-2026, forfeiting the 3-4x M5 prefill. Closing that gap means us maintaining a Metal kernel library against Apple's own team. **Verdict: rejected as base; tracked as the revisit candidate (see §7), and its ISQ and API-surface ideas inform RFC-0006/0007.**

### 3.5 Burn (Rust, WGPU/CubeCL)

Elegant multi-backend design, but its Metal path goes through wgpu/CubeCL abstraction layers; no fused paged attention or quantized-matmul story competitive with MLX on Apple GPUs, and no tensor-op path. Inference-serving maturity for LLMs is well behind every option above. **Verdict: rejected for v1; strategically interesting if a future Linux port materializes.**

### 3.6 ONNX Runtime / Core ML

Static-graph frameworks fight the dynamic shapes of paged KV and continuous batching; CoreML's ANE targeting shines for small encoder-style models, not 8-70B autoregressive decode; quantization format support diverges from the HF ecosystem. onnxruntime-genai does not target Metal seriously. Apple's own Foundation Models framework serves its ~3B system model to apps, which is a competitor context, not a substrate. **Verdict: rejected. ANE via Core ML remains a v1.x candidate for draft models only (RFC-0003 §8).**

### 3.7 Swift + mlx-swift

Same MLX core, first-party bindings, natural for the eventual desktop app. But the CLI/server ecosystem (async HTTP, HF hub clients, tokenizers, grammar engines, CLI frameworks) is thinner than Rust's, and a Swift-core engine complicates the C-ABI reuse story for non-Apple frontends. **Verdict: Swift is the v1.0 desktop shell language, consuming the engine's C ABI; not the engine language.**

## 4. Scoring matrix

Weights reflect §2. Scores 1-5.

| Criterion (weight) | MLX core + Rust | Python (mlx-lm/vllm-metal fork) | llama.cpp base | mistral.rs/Candle | Burn | ONNX/CoreML |
| --- | --- | --- | --- | --- | --- | --- |
| Kernel perf ceiling incl. M5 NAX (x3) | 5 | 5 | 4 | 2 | 1 | 1 |
| Single-binary distribution (x3) | 5 | 1 | 5 | 5 | 5 | 3 |
| Control over sched/KV/memory (x2) | 5 | 2 | 2 | 4 | 4 | 1 |
| Velocity to v0.1 (x2) | 3 | 5 | 4 | 4 | 2 | 1 |
| Model coverage path (x2) | 4 | 5 | 5 | 3 | 1 | 1 |
| Maintenance risk (x2) | 4 | 3 | 3 | 2 | 2 | 2 |
| **Weighted total (/70)** | **62** | **47** | **53** | **46** | **34** | **20** |

llama.cpp-as-base scores closest; it loses on the two criteria that define the product (peak Apple-GPU performance trajectory, and owning the serving layers). The winning combination additionally absorbs llama.cpp as a secondary backend, taking its coverage strength without its ceiling.

## 5. Decision

- D1. **Control plane: Rust.** Workspace crates: `drakkar-cli` (clap), `drakkar-server` (axum, tokio), `drakkar-sched`, `drakkar-fit`, `drakkar-models` (hf-hub, safetensors, tokenizers), `drakkar-engine` (backend trait + actors), `drakkar-grammar` (llguidance for JSON-schema/grammar-constrained decoding).
- D2. **Primary backend: `drakkar-mlx`.** A C++17 shim (~3-6 kLoC) statically linking a pinned MLX core. The shim exposes our own C ABI (`dk_*`): array lifecycle, model-graph construction per supported architecture, quantized matmul, fused SDPA, KV block ops, fused sampling. Rust binds this ABI via bindgen. We link MLX C++ directly rather than depending on mlx-c, so new core features (tensor-op paths, batch primitives) are consumable the day they land, and our ABI stays stable regardless of upstream churn.
- D3. **Model definitions in the shim, config-driven.** Architectures implemented natively (launch set: Llama-family, Qwen3/3.5 dense + MoE, Gemma-family with hybrid SWA, gpt-oss, Mistral-family, MLA-style DeepSeek-lineage), parameterized by HF `config.json`. mlx-lm's Python model zoo is the executable reference for each port.
- D4. **Secondary backend: `drakkar-gguf`** (cargo feature, on by default in release): llama.cpp embedded via FFI for GGUF-only checkpoints, implementing the same `InferenceBackend` trait with reduced `Capabilities`. Its Metal 4 tensor path is kept enabled per upstream defaults on M5 + macOS 26.2+.
- D5. **Build/distribution:** universal arm64 binary; Metal shaders from MLX are embedded (metallib) so no compiler toolchain is required at runtime; codesigned and notarized; distributed via Homebrew tap and GitHub releases. MSRV pinned; MLX pinned per DRAKKAR release with a documented upgrade cadence (target: within two MLX releases).
- D6. **Key third-party Rust crates:** tokio, axum, tower, hf-hub, tokenizers, safetensors, serde, llguidance (structured output), sysinfo + IOKit bindings (hardware probe), tracing, criterion (benches).

## 6. Consequences

Positive: fastest available substrate with Apple maintaining the kernels; the shim isolates 100% of MLX API surface behind ~40 C functions; single-binary story intact; scheduler/KV policy fully ours; Swift desktop app and third-party embedders get the same C ABI for free.

Negative and accepted: we re-implement the model zoo instead of importing mlx-lm's (mitigated by config-driven definitions and D4's escape hatch); C++ in the codebase with the usual FFI safety burden (mitigated by the ABI being small, fuzzed, and valgrind/ASan-covered in CI); dependence on MLX's roadmap (mitigated by pinning plus the backend seam, and de-risked by the fact that Ollama, vLLM, and LM Studio now share the same dependency, making MLX abandonment an ecosystem-level, not DRAKKAR-level, event).

## 7. Revisit triggers

- R1. If Candle/mistral.rs ships fused attention + quantized matmul within 10% of MLX on M-series decode **and** a Neural Accelerator path, re-evaluate an all-Rust backend (annual check).
- R2. If Apple ships an official MLX batch-serving C API that covers RFC-0005/0007 needs, shrink the shim accordingly.
- R3. If Linux/CUDA becomes a goal, the seam (RFC-0001 §5) admits a third backend; MLX's own CUDA backend (landed 2025-2026) is the first candidate to evaluate before Burn or Candle.

## References

- Apple MLR, "Exploring LLMs with MLX and the Neural Accelerators in the M5 GPU" (Nov 2025): TensorOps/MPP usage, 3.33-4.06x TTFT table, macOS 26.2 requirement
- ml-explore/mlx: C/C++ API docs; MLX 0.31.x release notes; mlx-lm continuous batching announcement (Dec 2025); WWDC26 session 232
- ggml-org/llama.cpp PR #16634 (Metal tensor API on M5, prefill gains); LM Studio bug tracker issue #2040 (cost of missing the tensor path)
- vllm-project/vllm-metal README and docs (v0.2.0, Apr 2026); Docker engineering blog (Mar 2026)
- EricLBuehler/mistral.rs (Metal PagedAttention, ISQ) and tracking issue #903 (Metal vs MLX/llama.cpp); GarthDB/metal-candle BENCHMARKS.md (kernel-fusion gap analysis)
- oxideai/mlx-rs and the mlxrs crate (2026): evidence Rust-over-MLX-C is tractable; threading constraints documented
- Barrios et al., arXiv:2601.19139: vllm-mlx vs mlx-lm vs llama.cpp throughput study on M4 Max
- Community benchmark roundups (willitrunai.com, promptquorum.com, codersera.com, Apr-Jun 2026): MLX vs llama.cpp decode deltas on M4/M5
