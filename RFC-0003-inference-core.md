# RFC-0003: Inference Core

**Status:** Draft
**Author:** A. Bakhta
**Created:** 2026-07-14
**Requires:** RFC-0001, RFC-0002
**Related:** RFC-0005 (KV), RFC-0007 (scheduler), RFC-0009 (targets)

## 1. Summary

The inference core is the `drakkar-mlx` backend: model graph construction, Metal execution, quantized compute, sampling, and speculative decoding, built on the MLX core per RFC-0002. This RFC specifies execution semantics, the precision/quantization matrix, the token pipeline, and the Metal-level memory and capability handling. The KV pool it plugs into is RFC-0005; who gets scheduled when is RFC-0007.

## 2. Execution model

- IC1. Computation uses MLX's lazy graphs: a decode step builds the full forward + sampling graph and evaluates once, minimizing kernel-launch and synchronization overhead (the documented weakness of un-fused Metal stacks).
- IC2. The hot loops (decode step per batch shape bucket, prefill chunk) are wrapped in MLX graph compilation to fuse elementwise chains (norm/rope/activation epilogues). Compiled variants are cached per (architecture, batch-bucket, dtype) key; shape buckets are powers of two up to `max_concurrency` to bound compilation churn.
- IC3. All GPU work is issued from the engine actor thread (RFC-0001 A2) on a single MLX stream; a second low-priority stream MAY be used for KV block quantize/dequantize and SSD-tier staging where overlap is measured to help (guarded by a flag until proven).
- IC4. Async evaluation: the actor issues step N+1's graph while draining step N's sampled tokens, keeping the GPU fed. Token readback is a single small transfer per step (sampled ids + optional logprobs), never full logits, except when the API requests top-k logprobs (then top-k is computed on-GPU and only k values cross).

## 3. Numerics and quantization

- IC5. Compute dtype: bf16 activations by default (fp16 fallback pre-M2 if measured faster); fp32 accumulation inside matmul/attention kernels as provided by MLX; RMSNorm epsilon and rope theta honored per model config.
- IC6. Weight formats supported natively (Backend A):

| Format | bits/weight (effective) | Source | Notes |
| --- | --- | --- | --- |
| MLX affine, group 64 | 4.5 / 5.5 / 6.5 / 8.5 | mlx-community repos or on-device convert | default serving format; group scale+bias fp16 adds 0.5 bpw at g=64 |
| MLX affine, group 32 | +0.5 bpw vs above | convert flag | quality bump for small models (< 4B) where 4-bit g64 degrades |
| MXFP4 | 4.25 | native checkpoints (gpt-oss) | pass-through, no conversion |
| bf16 / fp16 | 16 | HF safetensors | direct serve when it fits |
| NVFP4 | 4.x | upstream MLX support tracking | adopt when MLX lands it stably (Ollama 0.19 precedent) |

- IC7. Backend B (llama.cpp) serves the GGUF quant zoo (K-quants, i-quants) unmodified; the fit engine carries per-family bpw tables for both formats (RFC-0004 §3).
- IC8. On-device quantization ("fit-to-machine"): when only fp16/bf16 weights exist, the model manager quantizes at pull time to the fit engine's recommended bits/group (RFC-0006 §6). Quantization is per-tensor streaming (load shard, quantize, write, drop) so peak memory stays ~1 shard above the output size, and MUST be feasible on machines that cannot hold the fp16 model.
- IC9. Sensitivity guardrails: embeddings and lm_head follow the model-family recipe (default: quantize at the same bits, group 32); layers flagged sensitive by the recipe (for example the final full-attention block in deep hybrids) stay at 8-bit or bf16. Recipes live in the model-def layer and are versioned.

## 4. Attention and prefill

- IC10. Attention uses MLX fused scaled-dot-product attention with GQA support; the paged variant reads K/V through the block table supplied by RFC-0005 (gather-based fallback first, fused paged kernel as the optimization milestone; vllm-metal's unified paged varlen kernel is the design reference and upstreaming target).
- IC11. Sliding-window and hybrid-attention models (Gemma-family interleaved SWA:global, gpt-oss alternating) are first-class: SWA layers use fixed ring buffers, global layers use paged cache, per RFC-0005 §6. MLA-style latent KV (DeepSeek lineage) is a distinct cache layout with its own accounting (RFC-0004 §4).
- IC12. Chunked prefill: prompts are processed in scheduler-controlled chunks (default 512 tokens, adaptive 256-2048 by ITL pressure, RFC-0007 §6). The kernel path for chunks MUST hit the tensor-op (NAX) matmul route on capable hardware; this is where the 3-4x M5 gain lives, and a regression test pins it (RFC-0009).
- IC13. Long-context prefill activation watermark is bounded by chunk size, not prompt length: peak extra memory ≈ f(chunk, hidden, layers) and is charged against the budget as `activation_watermark` (RFC-0004 §5).

## 5. Sampling pipeline

- IC14. Fully on-GPU per batch step: temperature, top-k, top-p, min-p, repetition/frequency/presence penalties (penalty state per sequence on-device), seed-per-request via counter-based RNG for reproducibility, then sample. Greedy short-circuits.
- IC15. Logit processors are a small composable set evaluated in fixed order: penalties -> logit_bias -> grammar mask -> temperature -> truncation (top-k/p/min-p) -> sample. Adding a processor requires a benchmark showing < 2% ITL cost at batch 8.
- IC16. Structured output: llguidance compiles JSON Schema / regex / lark grammars to token masks in Rust; the mask (bitset over vocab) uploads per step only for constrained requests and applies as the grammar-mask stage. Constrained requests MUST guarantee valid output (mask, not retry).
- IC17. Stop handling in Rust on the streamed tokens: stop strings across token boundaries, max_tokens, EOS per template; tool-call and reasoning-content markers parsed incrementally per model family (RFC-0007 §5).

## 6. Speculative decoding

Decode is bandwidth-bound on this hardware, so speculation converts spare compute into tokens. Three tiers, all behind `Capabilities` and off unless they win in calibration:

- IC18. **Prompt-lookup (n-gram) speculation**, default-on for agent/RAG workloads: propose continuations matching recent context n-grams; zero extra memory; verified in the same batch step. Typical 1.2-1.8x on repetitive/tool-heavy text; never slower than 3% off-path (auto-disables per-request if acceptance < threshold).
- IC19. **Draft-model speculation**: a small same-tokenizer model (0.5-1.7B at 4-bit) drafts k=4-8 tokens verified in one target forward. The fit engine budgets the draft explicitly; `drakkar run --draft auto` picks a compatible draft from a curated table. Expected 1.5-2.2x single-stream on 30B+ targets (est., calibrate per RFC-0009).
- IC20. Self-speculative methods (MTP heads, EAGLE-style) are v1.x research items; the verification path in IC19 is built generic (verify B draft streams) so they slot in.
- IC21. Speculation composes with batching: the scheduler disables per-sequence speculation when batch occupancy already saturates bandwidth (crossover measured, stored in calibration).

## 7. MoE execution

- IC22. MoE layers use MLX grouped/gathered expert matmul; router in bf16. Active-parameter accounting (for bandwidth-derived speed estimates) and total-parameter accounting (for memory) are separate figures fed to RFC-0004.
- IC23. Expert-offload ("stream experts from SSD") is explicitly out of scope for v1: it trades the product's latency promise for capacity (vMLX Smelt / Anemll flash-moe demonstrate feasibility and its throughput cost). The fit engine instead recommends models that fit. Revisit if user demand is strong.

## 8. Metal and platform specifics

- IC24. Weights load via mmap from the content store; MLX/Metal residency keeps them wired for the model lifetime. Load time budget is SSD-bandwidth-bound (14.5 GB/s on M5-gen MBP: ~4 s for 55 GB).
- IC25. The backend sets MLX's memory and cache limits from the declared budget at load: hard memory limit = budget, allocator cache limit sized so steady-state RSS stays within contract (I2). `memory_report()` exposes actual vs contract, plus Metal's `recommendedMaxWorkingSetSize` and the active wired limit for the doctor/fit surfaces.
- IC26. Capability probe at load: chip identity, GPU core count, bandwidth class, macOS version, Metal 4 tensor-op availability (functional self-test, not version sniffing, per the LM Studio M5 incident where shipped binaries failed the tensor check silently). Results flow into `Capabilities` and the fit engine's constants.
- IC27. Thermal/power: `drakkar bench` samples powermetrics for tokens/joule; the engine itself does not throttle (macOS does), but sustained-vs-burst throughput is reported separately (RFC-0009 §4) because fanless/14-inch chassis diverge.

## 9. Extension points (v1.x)

Vision input (image embeddings feeding the same paged cache, vision-encoder output caching keyed by image hash per vllm-mlx's 28x finding); embeddings models (mean-pool path, batch-oriented); ANE-hosted draft models via Core ML for IC19 on low-power targets.

## 10. Acceptance criteria

- AC1. Decode throughput within 5% of mlx-lm on the RFC-0009 matrix (same model, quant, machine), single stream.
- AC2. M5-family prefill demonstrates the tensor-op path: ≥ 3x M4-class prefill tokens/s at 4k prompt on the 8B reference (matching Apple's published envelope).
- AC3. Constrained JSON output: 100% schema-valid across the structured-output test corpus, ≤ 8% ITL overhead at batch 1.
- AC4. Speculation: n-gram path ≥ 1.2x on the agent-trace corpus with no workload regressing > 3%.
- AC5. Soak: 24 h mixed load, zero engine-thread panics, RSS drift < 2% post-warmup.

## Open questions

1. Fused paged-attention Metal kernel: write in-house against MLX's custom-kernel API, or upstream/adopt vllm-metal's unified varlen kernel? (Prototype both; decide on maintenance cost.)
2. Per-request seeds under continuous batching change batch composition nondeterministically; document "reproducible given same batch schedule" or add a strict-determinism mode at reduced throughput?

## References

- Apple MLR M5/MLX study (TTFT and generation tables, TensorOps + Metal Performance Primitives), Nov 2025
- mlx-lm: batch generation and server batching (Dec 2025), prompt cache, speculative decoding; WWDC26 session 232
- vllm-metal v0.2.0 release notes (unified paged varlen Metal kernel; MTP/draft speculation)
- mlx-vlm server docs: KV quantization schemes incl. TurboQuant, last-layer sensitivity note (2026)
- llguidance (grammar-constrained decoding); mistral.rs ISQ design
- vMLX Smelt / Anemll flash-moe (SSD expert streaming trade-offs), 2026
