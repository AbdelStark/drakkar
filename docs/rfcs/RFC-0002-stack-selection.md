# RFC-0002: Technology Stack Selection

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.1

## Summary

DRAKKAR is built as **a Rust control plane over a vendored MLX compute core, joined by a thin C++ shim that exposes a stable C ABI (`dk_*`), with an optional llama.cpp backend for GGUF coverage**. This is the only combination that simultaneously satisfies the four hard constraints that define the product: peak kernel performance on Apple GPUs (including M5 Neural Accelerators via Metal 4 TensorOps), single-binary arm64-only native distribution, full ownership of scheduling and KV memory policy, and a maintenance burden a small team can carry. This RFC records the candidate analysis, the scoring, the decision (D1-D6), and the conditions under which the decision gets revisited.

## Motivation

The PRD's software-landscape analysis ([PRD §2.2](../../PRD.md#22-software-landscape)) concludes that the kernel race on Apple GPUs is effectively decided: MLX's Metal kernels, maintained by Apple and first to exploit new silicon, are the substrate to build on, not compete with. The open competitive space is one layer up — packaging, resource intelligence, agent-grade serving — and that layer is exactly where existing tools fall short ([PRD §2.3](../../PRD.md#23-where-existing-tools-fall-short)): Python packaging friction is the top install failure class (mlx-lm, vllm-metal, vllm-mlx, vMLX all require a Python 3.12 arm64 environment), and no incumbent owns its serving layers deeply enough to ship real KV reuse across agent loops or pre-download feasibility.

A stack decision therefore has to thread a needle: inherit Apple's kernels without inheriting Python distribution, and own the scheduler, admission control, and KV allocator without forking a framework that fights back. Every candidate stack fails at least one of these; the analysis below shows which and why. The decision here binds RFC-0001's component boundaries ([Architecture](RFC-0001-architecture.md#proposed-design)) to concrete languages, libraries, and build outputs, and everything downstream (RFC-0003 through RFC-0009, and the backend ABI in RFC-0010) assumes it.

## Goals

The stack MUST satisfy six hard constraints. These are requirements of this RFC and carry its S-prefix IDs:

- S1. **Kernel performance ceiling.** Must reach the best demonstrated decode and prefill throughput on Apple Silicon, including Metal 4 TensorOps on M5-family GPUs (3.3-4.1x prefill measured by Apple with MLX). A stack that structurally trails the leader by >15% fails the product's reason to exist (PRD G2).
- S2. **Distribution.** One signed, notarized arm64 binary. Anything requiring the user to have Python, a venv, or a container fails (this is the top failure class in the incumbent Python stacks and the core UX differentiator; PRD G6).
- S3. **Control.** We must own the scheduler, admission control, KV block allocator, prefix cache, and sampling pipeline; a framework that owns these itself (and fights us for them) is a liability (PRD G3, G5).
- S4. **Velocity and safety.** Memory-safe systems language for the 80% of the code that is not kernels; strong async/HTTP/serialization ecosystem; hiring/contributor pool.
- S5. **Model coverage.** New open-weight architectures land weekly (2026 cadence: hybrid attention, MLA, new MoE routers). Adding an architecture must be days, not weeks, and there must be an escape hatch for the long tail.
- S6. **License compatibility** with an Apache-2.0 product (license fixed per [RFC-0012](RFC-0012-release-engineering.md#proposed-design)).

Additional goals of this document:

- Record a scored, evidence-backed comparison of every credible candidate so the decision is auditable.
- Fix the Rust workspace layout, the FFI strategy, and the third-party dependency set for v0.1.
- Define explicit revisit triggers so the decision is falsifiable rather than permanent.

## Non-Goals

- Selecting the desktop-app UI stack. Swift/SwiftUI is fixed as the v1.0 shell language consuming the engine's C ABI (see [Alternatives](#swift--mlx-swift)), but the app itself is RFC-scoped at v1.0, not here.
- Cross-platform (Linux/CUDA/Windows) stack selection. PRD N1 excludes it from v1; the backend seam (RFC-0001 I5) preserves the option and R3 below defines the trigger.
- Choosing specific Metal kernel implementations (fused paged attention build-vs-adopt is RFC-0003's spike, per its kept-open question).
- Re-litigating this decision in downstream RFCs. Changes route through the revisit triggers in [Open Questions](#open-questions).

## Proposed Design

### The selected substrate: MLX core (C++/Metal), consumed natively

Apple's array framework: lazy computation graphs, unified-memory-native, hand-tuned Metal kernels (steel GEMM, fused SDPA, quantized matmul), quantization built in (affine 4/5/6/8-bit, MXFP4), `mx.compile` graph fusion, and, decisively, first-to-silicon support: Neural Accelerator paths via Metal 4 TensorOps and Metal Performance Primitives shipped with macOS 26.2, delivering the measured 3.3-4.1x TTFT gain on M5. C and C++ APIs are official; Swift and Python fronts share the same core. MIT licensed (S6-compatible). Ecosystem gravity is overwhelming in 2026: Ollama's new backend, vllm-metal, LM Studio's MLX engine, and roughly 4,800 pre-quantized models under mlx-community all sit on this core. Community benchmarks put MLX 15-40% ahead of llama.cpp Metal decode at equal quantization, with the gap widening on M5.

Known weaknesses, accepted with mitigations: the polished LM tooling (mlx-lm) is Python; consuming the core natively means implementing model graph definitions, weight loading, and generation loops ourselves in C++/Rust (mlx-lm is the executable reference for each port, and the existing Rust-over-MLX crates demonstrate the port is tractable). The official C API (mlx-c) historically lags core features by weeks — which is why D2 links the C++ core directly and does not depend on mlx-c.

### Decision

- D1. **Control plane: Rust.** Workspace crates: `drakkar-core` (shared types, config schema, and the error taxonomy of RFC-0011), `drakkar-cli` (clap), `drakkar-server` (axum, tokio), `drakkar-sched`, `drakkar-fit`, `drakkar-models` (hf-hub, safetensors, tokenizers), `drakkar-engine` (backend trait + actors), `drakkar-grammar` (llguidance for JSON-schema/grammar-constrained decoding), `drakkar-mlx-sys` (bindgen-generated raw `dk_*` bindings + build orchestration for the vendored shim) and `drakkar-mlx` (safe Rust wrapper implementing `InferenceBackend`), `drakkar-gguf` (secondary backend, cargo feature). Every crate above `drakkar-mlx-sys`/`drakkar-gguf` MUST be free of Metal, MLX, and llama.cpp types (RFC-0001 invariant I5).
- D2. **Primary backend: `drakkar-mlx`.** A C++17 shim (~3-6 kLoC) statically linking a pinned MLX core. The shim exposes our own C ABI (`dk_*`): array lifecycle, model-graph construction per supported architecture, quantized matmul, fused SDPA, KV block ops, fused sampling. Rust binds this ABI via bindgen in `drakkar-mlx-sys`. We link MLX C++ directly rather than depending on mlx-c, so new core features (tensor-op paths, batch primitives) are consumable the day they land, and our ABI stays stable regardless of upstream churn. The ABI surface, versioning, and conformance rules are normative in [RFC-0010](RFC-0010-backend-abi.md#proposed-design).
- D3. **Model definitions in the shim, config-driven.** Architectures implemented natively (launch set: Llama-family, Qwen3/3.5 dense + MoE, Gemma-family with hybrid SWA, gpt-oss, Mistral-family, MLA-style DeepSeek-lineage), parameterized by HF `config.json`. mlx-lm's Python model zoo is the executable reference for each port ([RFC-0003](RFC-0003-inference-core.md#proposed-design) specifies the model-definition layer).
- D4. **Secondary backend: `drakkar-gguf`** (cargo feature, on by default in release builds): llama.cpp embedded via FFI for GGUF-only checkpoints, implementing the same `InferenceBackend` trait with reduced `Capabilities`. Its Metal 4 tensor path is kept enabled per upstream defaults on M5 + macOS 26.2+. Ships in v0.2 per the roadmap (PRD §8).
- D5. **Build/distribution:** a single arm64-only binary (Apple Silicon is the only target, PRD N6; no universal/x86_64 slice is built). Metal shaders from MLX are embedded (metallib) so no compiler toolchain is required at runtime; the binary is codesigned and notarized; distributed via Homebrew tap and GitHub releases ([RFC-0012](RFC-0012-release-engineering.md#proposed-design)). MSRV pinned via `rust-toolchain.toml`; MLX pinned to an exact tag per DRAKKAR release with a documented upgrade cadence (target: track upstream within two MLX releases; see [Migration / Rollout](#migration--rollout)).
- D6. **Key third-party dependencies**, each with its constraint and reason:

| Dependency | Constraint | Reason |
| --- | --- | --- |
| MLX (vendored C++ core) | exact tag per release; 0.31.x line at v0.1 | compute substrate (D2); pin is the churn firewall |
| llama.cpp (vendored, `drakkar-gguf` only) | exact tag per release | GGUF long-tail coverage (D4) |
| Rust toolchain | MSRV 1.85, pinned in `rust-toolchain.toml` | reproducible builds; 2024-edition features |
| C++ | C++17, Apple Clang from pinned Xcode CLT | MLX core's language level; macOS 26.2 SDK for Metal 4 headers (runtime baseline stays macOS 15+, PRD P15) |
| tokio | 1.x | async runtime for server, scheduler channels, downloads |
| axum + tower | axum 0.8.x, tower 0.5.x | HTTP server and middleware for RFC-0007 endpoints |
| clap | 4.x | CLI parsing, RFC-0008 surface |
| hf-hub | 0.4.x | HF resolution/downloads, RFC-0006 pipeline |
| tokenizers | 0.21.x | tokenizer parity with the HF ecosystem |
| safetensors | 0.5.x | zero-copy mmap weight loading (PRD P12) |
| serde / serde_json | 1.x | config, API schemas, `--json` contracts (PRD P7) |
| llguidance | 0.7.x | JSON-schema/grammar-constrained decoding (`drakkar-grammar`) |
| sysinfo + IOKit bindings | sysinfo 0.33.x | hardware probe feeding the fit engine (RFC-0004) |
| tracing | 0.1.x | structured logging/diagnostics |
| criterion | 0.5.x (dev) | microbenchmarks feeding RFC-0009 |
| bindgen | 0.71.x (build) | `dk_*` ABI bindings in `drakkar-mlx-sys` |

Version bumps within a pinned line follow normal dependency review; MLX and llama.cpp pins move only via the cadence in [Migration / Rollout](#migration--rollout).

### Why this combination holds

- Fastest available substrate, with Apple maintaining the kernels and shipping silicon support first (S1).
- The shim isolates 100% of the MLX API surface behind ~40 C functions at v0.1 (growing per milestone, see [Migration / Rollout](#migration--rollout)), so upstream churn is absorbed at one seam (S4, PRD risk table).
- Single-binary story intact: static linking, embedded metallib, no runtime toolchain (S2).
- Scheduler, admission control, KV policy, prefix cache, and sampling are fully ours in Rust (S3).
- The Swift desktop app (v1.0) and third-party embedders get the same C ABI for free — one engine, two frontends (RFC-0010).
- llama.cpp as a trait-level secondary backend takes GGUF's coverage strength without inheriting its performance ceiling or its server (S5).
- Dependence on MLX's roadmap is de-risked structurally: Ollama, vLLM (vllm-metal), and LM Studio now share the same dependency, so MLX abandonment would be an ecosystem-level event, not a DRAKKAR-level one; the pin plus the backend seam bound the blast radius regardless.

## Alternatives Considered

Each candidate was evaluated against S1-S6. None is a straw man; several contribute design inputs even where rejected.

### Python stack: fork mlx-lm or vllm-metal

Fastest possible MVP: mlx-lm already has continuous batching, prompt caching, speculative decoding, and every architecture. vllm-metal (contributed by Docker to the vLLM org, March 2026) adds vLLM's scheduler, paged Metal attention (v0.2.0), and MTP/draft speculative decoding on MLX.

Fails S2 outright (Python 3.12 arm64 environment; vllm-metal compiles vLLM core from source via clang++ at install). Fails the differentiation test: we would be the fifth Python MLX server of 2026 (mlx-lm, vllm-metal, vllm-mlx, vMLX, oMLX), competing on their terms. The GIL and Python object overhead also tax the scheduler exactly where agentic concurrency needs headroom (S3). **Verdict: reference material, not a base.** vllm-metal's paged varlen Metal kernel and vllm-mlx's published batching results are design inputs for [RFC-0005](RFC-0005-kv-cache.md#proposed-design) and [RFC-0007](RFC-0007-api-server.md#proposed-design).

### llama.cpp (C/C++), as the base

Mature Metal backend, upstream Metal 4 tensor-API support on M5 (2-3x prefill, PR #16634), GGUF's unmatched quantization zoo (K-quants, i-quants) and model coverage, a capable server with continuous batching, grammars, and slot save/restore. MIT. Single-binary friendly.

But: decode trails MLX by 15-40% on Apple Silicon in 2026 measurements and Apple will keep tuning MLX for silicon we have not seen (S1); GGML's scheduler and server own the layers we need to control (S3), so we would fork-and-fight; and GGUF-only intake adds a conversion step against the HF-native flow. **Verdict: wrong primary, ideal secondary.** As an embedded library behind our backend trait, it costs little and buys the long tail of GGUF-only checkpoints (D4).

### mistral.rs / Candle (all-Rust)

The strongest all-Rust option: mistral.rs ships PagedAttention on Metal, prefix caching, in-situ quantization, an OpenAI server, MCP client, 45+ architectures, prebuilt Metal binaries. One language end to end, memory safety everywhere, aligned with our control plane (S4).

The blocker is S1: Candle's Metal kernels measurably trail MLX where it counts. Public head-to-heads attribute the gap to kernel-launch overhead and missing fusion (multiple launches where MLX runs one fused kernel), and no Candle Neural Accelerator path exists as of mid-2026, forfeiting the 3-4x M5 prefill. Closing that gap means us maintaining a Metal kernel library against Apple's own team. **Verdict: rejected as base; tracked as the standing revisit candidate (R1 in [Open Questions](#open-questions)); its ISQ and API-surface ideas inform [RFC-0006](RFC-0006-model-pipeline.md#proposed-design) and [RFC-0007](RFC-0007-api-server.md#proposed-design).**

### Burn (Rust, WGPU/CubeCL)

Elegant multi-backend design, but its Metal path goes through wgpu/CubeCL abstraction layers; no fused paged attention or quantized-matmul story competitive with MLX on Apple GPUs, and no tensor-op path (S1). Inference-serving maturity for LLMs is well behind every option above. **Verdict: rejected for v1; strategically interesting only if a future Linux port materializes (R3).**

### ONNX Runtime / Core ML

Static-graph frameworks fight the dynamic shapes of paged KV and continuous batching (S3); CoreML's ANE targeting shines for small encoder-style models, not 8-70B autoregressive decode; quantization format support diverges from the HF ecosystem (S5). onnxruntime-genai does not target Metal seriously. Apple's own Foundation Models framework serves its ~3B system model to apps, which is a competitor context, not a substrate. **Verdict: rejected. ANE via Core ML remains a v1.x candidate for draft models only ([RFC-0003](RFC-0003-inference-core.md#proposed-design), extensions section).**

### Swift + mlx-swift

Same MLX core, first-party bindings, natural for the eventual desktop app. But the CLI/server ecosystem (async HTTP, HF hub clients, tokenizers, grammar engines, CLI frameworks) is thinner than Rust's (S4), and a Swift-core engine complicates the C-ABI reuse story for non-Apple frontends. **Verdict: Swift is the v1.0 desktop shell language, consuming the engine's C ABI; not the engine language.**

### Scoring matrix

Weights reflect the S1-S6 constraints in [Goals](#goals). Scores 1-5.

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

## Drawbacks

Accepted costs of the decision, with mitigations:

- **We re-implement the model zoo** instead of importing mlx-lm's. Mitigated by config-driven model definitions (D3), mlx-lm as the executable reference for each port, and the `drakkar-gguf` escape hatch (D4) for architectures not yet ported.
- **C++ in the codebase, with the usual FFI safety burden.** Mitigated by keeping the ABI small (~40 functions at v0.1), fuzzed, and AddressSanitizer/UndefinedBehaviorSanitizer-covered in CI; the conformance regime is normative in [RFC-0010](RFC-0010-backend-abi.md#testing-strategy).
- **Dependence on MLX's roadmap.** Mitigated by pinning per release plus the backend seam (RFC-0001 I5), and de-risked by shared ecosystem dependency (Ollama, vLLM, LM Studio all sit on the same core).
- **Slower path to v0.1 than a Python fork** (velocity score 3 vs 5). Accepted: the differentiators (S2, S3) are unreachable from a fork, so the extra weeks buy the product's reason to exist.
- **mlx-c is bypassed**, so we own binding maintenance against MLX's C++ headers across pin bumps. Accepted: the shim's compile-time breakage on an MLX upgrade is the designed early-warning signal, and the cost is bounded by the small `dk_*` surface.

## Migration / Rollout

The stack lands incrementally; the C ABI and the vendored pins have an explicit growth and upgrade plan.

**Per-milestone shim and workspace growth:**

- **v0.1 "First light":** workspace crates `drakkar-core`, `drakkar-cli`, `drakkar-server`, `drakkar-fit`, `drakkar-models`, `drakkar-engine`, `drakkar-grammar` (skeleton), `drakkar-mlx-sys`, `drakkar-mlx`. Shim covers array lifecycle, weight loading, graph construction for the D3 launch architectures, quantized matmul, fused SDPA, and fused sampling — approximately 40 `dk_*` functions. `drakkar-sched` exists as a single-request pass-through. `drakkar-gguf` is scaffolded (trait + feature flag) but does not ship.
- **v0.2 "Convoy":** shim adds batch primitives, paged KV block ops, KV-quantization ops, and speculative-decoding hooks (draft/target coordination per RFC-0003). `drakkar-gguf` ships, feature-flagged, on by default in release builds (D4); a release build with the feature disabled remains a supported CI configuration. `drakkar-sched` becomes the continuous-batching scheduler.
- **v0.3 "Fleet":** shim adds multi-model residency controls (per-engine Metal residency isolation, RFC-0001) and embedding-pooling ops; SSD KV tier needs no new ABI surface (block serialization lives Rust-side, RFC-0005).
- **v1.0 "Harbor":** the `dk_*` ABI is frozen at 1.0 and semantically versioned thereafter (RFC-0010 governs compatibility); the SwiftUI desktop shell consumes it unchanged.

**MLX pin and upgrade cadence:** MLX is vendored at an exact tag, recorded in the repository and in every release's reproducibility manifest (RFC-0009, LD18 regime). Target cadence: DRAKKAR tracks upstream within two MLX releases. Each pin bump is a dedicated PR that MUST pass the full parity and conformance suites ([Testing Strategy](#testing-strategy)) before merge; a bump that regresses the RFC-0009 CI gate (>3% release-over-release) is reverted, not waived. llama.cpp follows the same pin-bump discipline once `drakkar-gguf` ships.

**Toolchain rollout:** MSRV and Xcode CLT versions are pinned from v0.1 and bumped only in minor releases with a changelog entry (RFC-0012). Runtime OS baseline stays macOS 15+; Metal 4 tensor paths are runtime-detected, never assumed (PRD P15).

**No schema migration applies** — this RFC ships no persisted formats; store and config schemas belong to RFC-0006 and RFC-0008.

## Testing Strategy

The stack decision is validated continuously, not once. Three test families guard it:

**1. Decode parity harness vs mlx-lm** (`parity/decode-mlxlm`). The claim "MLX core consumed natively costs nothing vs Python MLX" is testable: RFC-0003 AC1 requires decode throughput within 5% of mlx-lm on the RFC-0009 model matrix (same model, quantization, machine, single stream) ([Inference Core](RFC-0003-inference-core.md#testing-strategy)). The harness additionally checks greedy-decode token-sequence equality against mlx-lm for each launch architecture on fixed prompts (golden fixtures), catching graph-construction bugs in the shim, not just speed regressions. Runs on every MLX pin bump and nightly on the RFC-0009 fleet.

**2. Shim ABI conformance and sanitizers.** Every `dk_*` function carries a conformance test (argument validation, error-code contract, ownership/lifetime rules) as specified by the AB* requirements in [RFC-0010](RFC-0010-backend-abi.md#testing-strategy). CI runs the conformance suite and the Rust-side integration tests under AddressSanitizer and UndefinedBehaviorSanitizer on every PR touching the shim or `drakkar-mlx-sys`; fuzz targets cover every ABI entry point that parses external data (config JSON, tensor headers). Feature-matrix builds: release with and without `drakkar-gguf` MUST both build and pass tests from v0.2 on.

**3. Quarterly stack-assumption review** (checklist, tied to R1-R3; ownership in [Open Questions](#open-questions)). Each quarter, run and record:
- R1 check: mistral.rs/Candle decode and prefill vs MLX on one M-series reference machine (RFC-0009 harness); note Neural Accelerator path status.
- R2 check: diff the current MLX C API (mlx-c and any batch-serving API) against the `dk_*` surface; list shim functions an official API could replace.
- R3 check: status of MLX's CUDA backend and any Linux demand signal; no action unless the R3 trigger fires.
- Pin health: current MLX pin age in upstream releases (alert if more than two behind).

Unit tests additionally cover crate-boundary invariants: a CI lint (`deny-backend-types`) fails the build if any crate above `drakkar-mlx-sys`/`drakkar-gguf` names MLX, Metal, or llama.cpp types (RFC-0001 I5).

## Open Questions

The decision stands; these are its standing falsification conditions, kept open deliberately.

- **R1 — All-Rust backend revisit.** If Candle/mistral.rs ships fused attention + quantized matmul within 10% of MLX on M-series decode **and** a Neural Accelerator path, re-evaluate an all-Rust backend. Owner: abdelstark. Resolution path: annual benchmark check (each January, using the RFC-0009 harness), fed by the quarterly review data; outcome recorded as an addendum to this RFC.
- **R2 — Official MLX serving C API.** If Apple ships an official MLX batch-serving C API that covers RFC-0005/RFC-0007 needs, shrink the shim to the uncovered remainder. Owner: abdelstark. Resolution path: quarterly stack-assumption review ([Testing Strategy](#testing-strategy)); acted on in the next minor release after a covering API is confirmed stable.
- **R3 — Linux/CUDA.** If Linux/CUDA becomes a goal (PRD v1.x exploratory), the backend seam (RFC-0001) admits a third backend; MLX's own CUDA backend (landed 2025-2026) is the first candidate to evaluate, before Burn or Candle. Owner: abdelstark. Resolution path: triggered only by an explicit roadmap change at or after v1.0; evaluation would be a new RFC.

## References

- [PRD](../../PRD.md) §2.2 (software landscape), §2.3 (gaps), §4 (G2, G6, N1, N6), §9 (risk table)
- [RFC-0001: Architecture](RFC-0001-architecture.md) — component boundaries, backend seam, invariant I5
- [RFC-0003: Inference Core](RFC-0003-inference-core.md) — model-definition layer, AC1 parity criterion
- [RFC-0005: KV Cache](RFC-0005-kv-cache.md), [RFC-0007: API Server](RFC-0007-api-server.md) — consumers of the batching/paged-KV design inputs noted above
- [RFC-0006: Model Pipeline](RFC-0006-model-pipeline.md) — HF-native intake that GGUF-only stacks would compromise
- [RFC-0009: Performance](RFC-0009-performance.md) — model matrix, fleet, CI regression gate, reproducibility manifests
- [RFC-0010: Backend ABI](RFC-0010-backend-abi.md) — normative `dk_*` surface, versioning, conformance (AB*)
- [RFC-0012: Release Engineering](RFC-0012-release-engineering.md) — signing, notarization, Homebrew tap, license
- Apple MLR, "Exploring LLMs with MLX and the Neural Accelerators in the M5 GPU" (Nov 2025): TensorOps/MPP usage, 3.33-4.06x TTFT table, macOS 26.2 requirement
- ml-explore/mlx: C/C++ API docs; MLX 0.31.x release notes; mlx-lm continuous batching announcement (Dec 2025); WWDC26 session 232
- ggml-org/llama.cpp PR #16634 (Metal tensor API on M5, prefill gains); LM Studio bug tracker issue #2040 (cost of missing the tensor path)
- vllm-project/vllm-metal README and docs (v0.2.0, Apr 2026); engineering blog announcing the vLLM-org contribution (Mar 2026)
- EricLBuehler/mistral.rs (Metal PagedAttention, ISQ) and tracking issue #903 (Metal vs MLX/llama.cpp); metal-candle BENCHMARKS.md (kernel-fusion gap analysis)
- oxideai/mlx-rs and the mlxrs crate (2026): evidence Rust-over-MLX-C is tractable; threading constraints documented
- Barrios et al., arXiv:2601.19139: vllm-mlx vs mlx-lm vs llama.cpp throughput study on M4 Max
- Community benchmark roundups (Apr-Jun 2026): MLX vs llama.cpp decode deltas on M4/M5
