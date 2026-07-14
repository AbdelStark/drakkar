# RFC-0004: Feasibility Engine ("fit")

**Status:** Draft
**Author:** A. Bakhta
**Created:** 2026-07-14
**Requires:** RFC-0001
**Related:** RFC-0003, RFC-0005, RFC-0006, RFC-0008, RFC-0009

## 1. Summary

The feasibility engine answers, before any download and again at load time: does this model fit this machine, at what quantization, with how much context, and how fast will it feel. It is a pure library (`drakkar-fit`) with three consumers: the CLI preflight (`drakkar fit`, and automatically inside `run`/`pull`), the HTTP `/fit` endpoint, and the scheduler's admission control. One implementation, one truth (RFC-0001 I3).

Design stance: **model the machine as it is, not as the spec sheet says.** Runtime probes beat heuristics; heuristics exist only for offline simulation of other machines.

## 2. Inputs

- FE1. **Model descriptor** from HF metadata without downloading weights: `config.json` (architecture, layers, hidden, heads, kv_heads, head_dim, vocab, MoE topology, sliding window layout, MLA dims), safetensors index (exact per-tensor dtypes and byte sizes when present), quantization config, tokenizer config, and repo file sizes. When the safetensors index exists, weight sizing is exact, not estimated.
- FE2. **Hardware profile.** Runtime probe: chip identity and GPU core count (IOKit/sysctl), total RAM, `MTLDevice.recommendedMaxWorkingSetSize` (the authoritative GPU budget signal), current `iogpu.wired_limit_mb`, free memory and pressure, macOS version, Metal 4 tensor-op self-test result (RFC-0003 IC26), SSD throughput class. Shipped fallback table (for `fit --machine m4-max-64` simulations) carries per-chip bandwidth: M1 68, M1 Pro 200, M1 Max 400, M2 100, M2 Pro 200, M2 Max 400, M3 100, M3 Pro 150, M3 Max 300/400, M4 120, M4 Pro 273, M4 Max 410/546, M5 154, M5 Pro 307, M5 Max 460/614, Ultra-class 800+ (GB/s, Apple spec sheets).
- FE3. **Request shape:** target context (prompt + generation), concurrency, KV precision, draft-model choice. Defaults: model's advertised context capped at 32k for the preflight display, concurrency 1, KV fp16.
- FE4. **Calibration store** (RFC-0009 §6): measured per-(chip, model-class) throughput anchors and efficiency factors that override shipped defaults after `drakkar bench --calibrate`.

## 3. Memory model: weights

- FE5. Exact path: sum safetensors shard sizes (plus format overhead) when the repo is already quantized. Estimated path (for "what if we quantize" projections):

```
weight_bytes ≈ Σ_tensors  params(t) × bpw_eff(scheme, t) / 8
bpw_eff(MLX affine, b bits, group g) = b + 32/g          # fp16 scale+bias per group
   e.g. 4-bit g64 → 4.5 bpw ; 4-bit g32 → 5.0 ; 8-bit g64 → 8.5
bpw_eff(MXFP4) = 4.25 ; bpw_eff(bf16) = 16
GGUF families use a shipped per-type table (Q4_K_M ≈ 4.85, Q5_K_M ≈ 5.7, Q6_K ≈ 6.6, Q8_0 ≈ 8.5, ...)
```

- FE6. Per-tensor recipes (RFC-0003 IC9) are applied in the estimate: embeddings/lm_head at their recipe bits, norms fp32, sensitive layers at 8-bit; the estimator and the converter share the recipe tables so predictions match artifacts.
- FE7. Cross-check anchor: Apple's published MLX footprints (Qwen3-8B-4bit 5.61 GB total-inference, Qwen3-30B-A3B-4bit 17.31 GB, gpt-oss-20b MXFP4 12.08 GB at 4k context) are regression fixtures: the model MUST reproduce these within 7%.

## 4. Memory model: KV cache (architecture-aware)

- FE8. Uniform full-attention (GQA):

```
kv_bytes_per_token = 2 × n_layers × n_kv_heads × head_dim × bytes_elem × (1 + q_overhead)
```

where `bytes_elem` is 2 (fp16), 1 (8-bit), 0.5 (4-bit) and `q_overhead ≈ 32/(16×g)` for group-quantized KV (≈ 3% at g64). Reference values (fp16):

| Model | Layout | KiB/token | 32k context |
| --- | --- | --- | --- |
| Llama-3.1-8B | 32L × 8kv × 128 | 128 | 4.0 GiB |
| Qwen3-8B | 36L × 8kv × 128 | 144 | 4.5 GiB |
| Qwen3-30B-A3B | 48L × 4kv × 128 | 96 | 3.0 GiB |
| Llama-3.3-70B | 80L × 8kv × 128 | 320 | 10.0 GiB |

- FE9. Hybrid sliding-window models: SWA layers contribute a **fixed** term (window W, not context), global layers scale with context:

```
kv_bytes(ctx) = 2 × bytes × H×D × ( L_swa × min(ctx, W) + L_global × ctx )
```

This is why Gemma-class models hold long contexts cheaply and the engine MUST NOT bill them at the uniform rate.
- FE10. MLA (DeepSeek lineage): per layer per token stores one shared latent `(c_kv + d_rope)` vector (for example 512+64 = 576 elements), not per-head K and V; the engine uses the MLA formula and flags the layout in the report.
- FE11. Recurrent/SSM hybrid layers contribute constant state independent of context; accounted as fixed bytes per sequence.
- FE12. Paged-cache quantization granularity and block overhead (block tables, per-block scales) are charged per RFC-0005 §5; the fit engine adds the pool metadata term `≈ blocks × 96 B`.

## 5. Memory model: everything else

```
total = weights + kv_pool(ctx, concurrency, kv_precision)
      + activation_watermark(chunk, hidden, layers, batch)      # bounded by chunked prefill, IC13
      + runtime_overhead                                        # Metal + allocator + tokenizer + process
      + draft_model_bytes (if speculation)                       
      + fragmentation_margin (3% of the above)
```

- FE13. `activation_watermark` is computed from the compiled graph's peak for the configured chunk size; shipped defaults per architecture class, replaced by measured values on first load (and cached).
- FE14. `runtime_overhead` shipped default 1.2 GiB, calibrated per machine on first run (measured RSS floor of an empty engine).

## 6. The GPU budget

- FE15. **Budget primary source:** live `recommendedMaxWorkingSetSize` (observed ≈ 2/3 of RAM on ≤ 36 GB machines and ≈ 3/4 above, but macOS revisions move this; never hardcode when a probe is available).
- FE16. `usable = budget − runtime_overhead − fragmentation_margin`, and additionally the plan MUST leave `os_floor` of un-wired system RAM: 4 GiB (≤ 16 GB machines), 6 GiB (24-36 GB), 8 GiB (48-64 GB), 12 GiB (96 GB+). Whichever constraint binds first wins.
- FE17. **Wired-limit guidance** (`iogpu.wired_limit_mb`): when a plan fits only with a raised limit, the report proposes the exact value (`sudo sysctl iogpu.wired_limit_mb=N`), states that Apple does not support it, computes N to respect FE16's os_floor, offers the revert (`=0`), and links persistence options (sysctl.conf / LaunchDaemon). DRAKKAR MUST NOT apply it automatically, and `drakkar doctor` reports the current value and whether it is safe for the resident model.
- FE18. Admission control at serve time uses the same arithmetic against **live** occupancy: a request is admitted only if `kv_needed(prompt+max_tokens)` fits the pool's free blocks plus reclaimable cache (RFC-0005 §8); otherwise a structured rejection with the computable maximum (`max_tokens_admissible`) is returned.

## 7. Verdicts and the context solver

- FE19. Verdict tiers on `total(ctx_requested)` vs `usable`:
  - **Comfortable**: total ≤ 0.85 × usable
  - **Tight**: 0.85 < total ≤ usable (works; warns about concurrent apps)
  - **Needs tuning**: fails as requested but a remedy plan exists; remedies ranked by expected quality impact: (1) official smaller quant, (2) on-device quantization, (3) KV 8-bit, (4) reduced context, (5) KV lower than 8-bit, (6) wired-limit raise (opt-in)
  - **Won't fit**: even at the floor plan (lowest sane quant, 4k context, KV 8-bit, max safe wired limit) the model exceeds the machine; the report says so plainly and suggests the nearest sibling that fits.
- FE20. Context solver: `ctx_max(precision) = solve kv_bytes(ctx) = usable − weights − activation_watermark − fixed_terms`, using the architecture-correct kv function (FE8-FE11); reported for fp16/8-bit/4-bit KV side by side, and per concurrency level for serve planning.

## 8. Performance prediction

- FE21. **Decode (bandwidth roofline):**

```
decode_tps ≈ η_d × BW / ( active_weight_bytes + kv_read_bytes(ctx) )
```

`active_weight_bytes` uses active parameters for MoE; `η_d` (kernel efficiency) shipped 0.65, calibrated per chip/model-class (observed range 0.6-0.85). The model reproduces the known shape: throughput falls as context grows because kv_read grows.
- FE22. **Prefill (compute roofline with capability factor):** prefill_tps is anchored, not derived from raw TFLOPs: shipped anchors per (chip-class, arch-class) scaled by active-parameter ratio, with a NAX multiplier applied only when the tensor-op self-test passes. Published anchors used for shipping defaults include Apple's M5 results (4k prompt: 14B-4bit TTFT < 10 s ⇒ ≥ 410 t/s; 30B-A3B TTFT < 3 s ⇒ ≥ 1,365 t/s; 3.3-4.1x over M4) and the M5 generation-speed uplift of 19-27%. All anchors carry provenance and an `est.` flag until locally calibrated.
- FE23. `TTFT_cold ≈ prompt/prefill_tps + c0`; `TTFT_warm ≈ uncached_suffix/prefill_tps + c1` (prefix cache, RFC-0005). Load-from-disk time reported separately as `weights_bytes / ssd_bw`.
- FE24. Accuracy targets (product metric M2): memory within 7%, decode within 20%, TTFT within 30% pre-calibration; post-calibration 5/10/15%. Every prediction the CLI prints carries its confidence tier (`measured`, `calibrated`, `modeled`).

## 9. Worked examples (shipped as golden tests)

| Machine | Model | Plan | Outcome |
| --- | --- | --- | --- |
| M2, 16 GB (budget ≈ 10.7 GiB) | Qwen3-8B, MLX 4-bit g64 | weights ≈ 4.2 GiB, KV fp16 144 KiB/t | Comfortable to ~16k ctx; ~38k with 8-bit KV; decode est. ~14-18 t/s |
| M5, 24 GB (probe budget ≈ 18 GiB) | Qwen3-30B-A3B, 4-bit | ≈ 17.3 GiB total at 4k (Apple anchor) | Tight; 8-bit KV recommended beyond 8k; prefill NAX-class |
| M4 Pro, 48 GB (budget ≈ 36 GiB) | Llama-3.3-70B, 4-bit | weights ≈ 37 GiB | Needs tuning: wired raise to 42 GiB (leaves 6 GiB OS) ⇒ Tight, ctx ≈ 24k with 8-bit KV, decode est. 5-7 t/s; engine recommends 70B on this machine only for patient workloads and suggests 30B-A3B as the fast alternative |
| M5 Max, 128 GB (budget ≈ 96 GiB) | gpt-oss-120b, MXFP4 | weights ≈ 62 GB | Comfortable; 100k+ ctx with 8-bit KV; decode est. 45-60 t/s (est., 614 GB/s part) |

## 10. Interfaces

- FE25. CLI: `drakkar fit <model> [--ctx N] [--kv-bits B] [--concurrency C] [--quant Q] [--machine PROFILE] [--json]`. Human output is a compact report card; the same struct serializes to JSON.
- FE26. JSON schema (v1, abridged):

```json
{ "schema": "drakkar.fit/1",
  "model": {"id": "Qwen/Qwen3-8B", "arch": "qwen3", "params_total": 8.19e9, "params_active": 8.19e9,
             "quant": {"scheme": "mlx_affine", "bits": 4, "group": 64, "bpw_eff": 4.5}},
  "machine": {"chip": "Apple M4 Pro", "ram_gib": 48, "budget_gib": 36.0, "budget_source": "probe",
               "bandwidth_gbs": 273, "nax": false, "wired_limit_mb": 0},
  "memory": {"weights_gib": 4.21, "kv_per_token_kib": 144, "kv_at_ctx_gib": 4.5,
              "activation_gib": 0.4, "runtime_gib": 1.2, "total_gib": 10.4, "confidence": "modeled"},
  "verdict": "comfortable", "headroom_gib": 25.6,
  "context": {"requested": 32768, "max_fp16": 214000, "max_kv8": 468000, "advertised": 131072},
  "performance": {"decode_tps": {"value": 55, "confidence": "calibrated"},
                   "ttft_cold_s": {"value": 1.9, "prompt": 4096, "confidence": "modeled"},
                   "load_s": 1.4},
  "remedies": [] }
```

- FE27. The scheduler consumes the same library for admission (FE18); the server exposes `POST /fit` with the same schema (RFC-0007 §8).

## 11. Acceptance criteria

- AC1. FE7 anchor fixtures reproduce within 7% in CI.
- AC2. Architecture-aware KV: hybrid (Gemma-class), MLA, and uniform models each produce correct kv(ctx) curves against hand-computed fixtures.
- AC3. On the RFC-0009 fleet, post-calibration prediction accuracy meets FE24.
- AC4. No plan emitted by the engine, when executed, breaches the memory contract (I2) in the soak suite.

## Open questions

1. Should the preflight fetch and parse `model.safetensors.index.json` opportunistically for exact sizing even on huge repos (cheap, few KB), always, or only on `fit`? (Leaning always.)
2. Expose an "aggressive" profile that trims os_floor for headless Mac-mini-class servers? Deferred until demand.

## References

- Apple MLR M5/MLX study (footprint and TTFT anchors), Nov 2025; Apple MacBook Pro spec sheets (bandwidth table), 2024-2026
- MTLDevice.recommendedMaxWorkingSetSize (Apple Developer docs); llama.cpp discussion #2182; community documentation of `iogpu.wired_limit_mb` defaults (≈2/3 vs 3/4 split), persistence patterns, and safety floors (2023-2026)
- DeepSeek-V2/V3 MLA cache layout papers; Gemma/gpt-oss hybrid attention model cards
- LM Studio GGUF RAM-range hints and mlx-serve min-max estimates (prior art for coarse fit UX), 2026
