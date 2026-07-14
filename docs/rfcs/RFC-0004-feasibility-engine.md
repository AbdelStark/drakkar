# RFC-0004: Feasibility Engine ("fit")

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.1 (memory model, context solver, verdicts); v0.2 (calibration-fed prediction hardening)

## Summary

The feasibility engine answers, before any download and again at load time: does this model fit this machine, at what quantization, with how much context, and how fast will it feel. It is a pure library (`drakkar-fit`) with three consumers: the CLI preflight (`drakkar fit`, and automatically inside `run`/`pull`), the HTTP `POST /fit` endpoint, and the scheduler's admission control. One implementation, one truth (RFC-0001 I3, [Architecture](RFC-0001-architecture.md#proposed-design)).

Design stance: **model the machine as it is, not as the spec sheet says.** Runtime probes beat heuristics; heuristics exist only for offline simulation of other machines.

## Motivation

The PRD opens with the question this engine exists to answer: "will this model run on my machine, at what context length, and how fast?" ([PRD §1](../../PRD.md#1-vision)). No shipping tool answers it. The gap table ([PRD §2.3](../../PRD.md#23-where-existing-tools-fall-short), row 1) records the observed failure mode: users discover a model does not fit after a 40 GB download; the closest prior art is a coarse RAM range shown on GGUF rows only; nobody models the wired-memory cap, KV growth versus context, or predicts TTFT.

Two product requirements bind this RFC directly:

- **P2** ([PRD §5.1](../../PRD.md#51-functional)): before downloading, DRAKKAR MUST display a fit report — required memory decomposed into weights, KV at requested context, and overhead; the machine's GPU budget; a verdict (Comfortable / Tight / Needs tuning / Won't fit); the maximum context at the chosen KV precision; and estimated TTFT and decode speed.
- **P3**: when a model does not fit as published, DRAKKAR MUST propose concrete remedies ranked by quality impact.

The feasibility engine is also load-bearing for memory safety (PRD P11): the same arithmetic that produces the preflight report drives serve-time admission control, so no admitted request can push the engine past its declared budget. Getting this wrong in either direction is a product failure — a false "won't fit" turns users away from models their machine handles; a false "comfortable" produces the mid-generation Metal OOM the product exists to eliminate.

## Goals

- Predict peak memory within 7% of measured, decode throughput within 20%, and cold TTFT within 30% with shipped constants; tighten to 5% / 10% / 15% after `drakkar bench --calibrate` (PRD M2, FE24).
- One library serving three consumers (CLI preflight, `/fit` endpoint, scheduler admission) with identical arithmetic and one JSON schema (`drakkar.fit/1`).
- Architecture-aware KV accounting for uniform GQA, hybrid sliding-window, MLA, and recurrent/SSM-hybrid layouts, correct against hand-computed fixtures (AC2).
- Verdicts computed against the probed GPU-wired budget, never against total RAM or model file size.
- A context solver reporting `ctx_max` per KV precision and per concurrency level.
- Remedy plans ranked by expected quality impact (PRD P3), including exact, revertible wired-limit guidance that is never applied automatically.
- Every printed prediction carries a confidence tier: `measured`, `calibrated`, or `modeled`.

## Non-Goals

- An "aggressive" `os_floor` profile that trims the reserved system-RAM floor for headless server use. Deferred; explicit non-goal for v1 (locked decision LD11).
- Cross-platform hardware profiles. The probe targets macOS on Apple Silicon only; the shipped fallback table covers M-series chips only. No Linux, Windows, discrete-GPU, or Intel profiles (PRD N1, N6).
- Measuring performance. The fit engine predicts; `drakkar bench` measures and calibrates ([RFC-0009 Performance](RFC-0009-performance.md#proposed-design)). This RFC consumes the calibration store, it does not define the harness.
- Guaranteeing throughput. Predictions are estimates with stated confidence tiers, not SLAs; the honest-speed principle (PRD §1) requires stating error bars, not hiding them.

## Proposed Design

### 1. Structure and consumers

`drakkar-fit` is a pure Rust library: no I/O beyond what its inputs hand it, deterministic given identical inputs, callable offline with a simulated machine profile. Consumers:

1. CLI: `drakkar fit`, and automatically as the preflight inside `run` and `pull` (RFC-0008 CLI contract, [CLI and UX](RFC-0008-cli-ux.md#proposed-design)).
2. Server: `POST /fit` (RFC-0007 §8, [API Server](RFC-0007-api-server.md#proposed-design)).
3. Scheduler: admission control at serve time (FE18), via the same library — one implementation, one truth (RFC-0001 I3).

### 2. Inputs

- FE1. **Model descriptor** from HF metadata without downloading weights: `config.json` (architecture, layers, hidden, heads, kv_heads, head_dim, vocab, MoE topology, sliding window layout, MLA dims), safetensors index (exact per-tensor dtypes and byte sizes when present), quantization config, tokenizer config, and repo file sizes. The preflight MUST always fetch `model.safetensors.index.json` when the repo publishes one (locked decision LD10): the file is a few KB, and when it exists weight sizing is exact, not estimated.
- FE2. **Hardware profile.** Runtime probe: chip identity and GPU core count (IOKit/sysctl), total RAM, `MTLDevice.recommendedMaxWorkingSetSize` (the authoritative GPU budget signal), current `iogpu.wired_limit_mb`, free memory and pressure, macOS version, Metal 4 tensor-op self-test result (RFC-0003 IC26, [Inference Core](RFC-0003-inference-core.md#proposed-design)), SSD throughput class. Shipped fallback table (for `fit --machine m4-max-64` simulations) carries per-chip bandwidth: M1 68, M1 Pro 200, M1 Max 400, M2 100, M2 Pro 200, M2 Max 400, M3 100, M3 Pro 150, M3 Max 300/400, M4 120, M4 Pro 273, M4 Max 410/546, M5 154, M5 Pro 307, M5 Max 460/614, Ultra-class 800+ (GB/s, Apple spec sheets).
- FE3. **Request shape:** target context (prompt + generation), concurrency, KV precision, draft-model choice. Defaults: model's advertised context capped at 32k for the preflight display, concurrency 1, KV fp16.
- FE4. **Calibration store** (RFC-0009 §6): measured per-(chip, model-class) throughput anchors and efficiency factors that override shipped defaults after `drakkar bench --calibrate`.

### 3. Memory model: weights

- FE5. Exact path: sum safetensors shard sizes (plus format overhead) when the repo is already quantized. Estimated path (for "what if we quantize" projections):

```
weight_bytes ≈ Σ_tensors  params(t) × bpw_eff(scheme, t) / 8
bpw_eff(MLX affine, b bits, group g) = b + 32/g          # fp16 scale+bias per group
   e.g. 4-bit g64 → 4.5 bpw ; 4-bit g32 → 5.0 ; 8-bit g64 → 8.5
bpw_eff(MXFP4) = 4.25 ; bpw_eff(bf16) = 16
GGUF families use a shipped per-type table (Q4_K_M ≈ 4.85, Q5_K_M ≈ 5.7, Q6_K ≈ 6.6, Q8_0 ≈ 8.5, ...)
```

- FE6. Per-tensor recipes (RFC-0003 IC9) are applied in the estimate: embeddings/lm_head at their recipe bits, norms fp32, sensitive layers at 8-bit; the estimator and the converter ([Model Pipeline](RFC-0006-model-pipeline.md#proposed-design)) share the recipe tables so predictions match artifacts.
- FE7. Cross-check anchor: Apple's published MLX footprints (Qwen3-8B-4bit 5.61 GB total-inference, Qwen3-30B-A3B-4bit 17.31 GB, gpt-oss-20b MXFP4 12.08 GB at 4k context) are regression fixtures: the model MUST reproduce these within 7%.

### 4. Memory model: KV cache (architecture-aware)

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
- FE12. Paged-cache quantization granularity and block overhead (block tables, per-block scales) are charged per RFC-0005 §5 ([KV Cache](RFC-0005-kv-cache.md#proposed-design)); the fit engine adds the pool metadata term `≈ blocks × 96 B`.

### 5. Memory model: everything else

```
total = weights + kv_pool(ctx, concurrency, kv_precision)
      + activation_watermark(chunk, hidden, layers, batch)      # bounded by chunked prefill, IC13
      + runtime_overhead                                        # Metal + allocator + tokenizer + process
      + draft_model_bytes (if speculation)
      + fragmentation_margin (3% of the above)
```

- FE13. `activation_watermark` is computed from the compiled graph's peak for the configured chunk size (chunked prefill bound: RFC-0003 IC13); shipped defaults per architecture class, replaced by measured values on first load (and cached).
- FE14. `runtime_overhead` shipped default 1.2 GiB, calibrated per machine on first run (measured RSS floor of an empty engine).

### 6. The GPU budget

- FE15. **Budget primary source:** live `recommendedMaxWorkingSetSize` (observed ≈ 2/3 of RAM on ≤ 36 GB machines and ≈ 3/4 above, but macOS revisions move this; never hardcode when a probe is available).
- FE16. `usable = budget − runtime_overhead − fragmentation_margin`, and additionally the plan MUST leave `os_floor` of un-wired system RAM: 4 GiB (≤ 16 GB machines), 6 GiB (24-36 GB), 8 GiB (48-64 GB), 12 GiB (96 GB+). Whichever constraint binds first wins.
- FE17. **Wired-limit guidance** (`iogpu.wired_limit_mb`): when a plan fits only with a raised limit, the report proposes the exact value (`sudo sysctl iogpu.wired_limit_mb=N`), states that Apple does not support it, computes N to respect FE16's os_floor, offers the revert (`=0`), and links persistence options (sysctl.conf / LaunchDaemon). DRAKKAR MUST NOT apply it automatically, and `drakkar doctor` reports the current value and whether it is safe for the resident model.
- FE18. Admission control at serve time uses the same arithmetic against **live** occupancy: a request is admitted only if `kv_needed(prompt+max_tokens)` fits the pool's free blocks plus reclaimable cache (RFC-0005 §8); otherwise a structured rejection with the computable maximum (`max_tokens_admissible`) is returned.

### 7. Verdicts and the context solver

- FE19. Verdict tiers on `total(ctx_requested)` vs `usable`:
  - **Comfortable**: total ≤ 0.85 × usable
  - **Tight**: 0.85 < total ≤ usable (works; warns about concurrent apps)
  - **Needs tuning**: fails as requested but a remedy plan exists; remedies ranked by expected quality impact: (1) official smaller quant, (2) on-device quantization, (3) KV 8-bit, (4) reduced context, (5) KV lower than 8-bit, (6) wired-limit raise (opt-in)
  - **Won't fit**: even at the floor plan (lowest sane quant, 4k context, KV 8-bit, max safe wired limit) the model exceeds the machine; the report says so plainly and suggests the nearest sibling that fits.
- FE20. Context solver: `ctx_max(precision) = solve kv_bytes(ctx) = usable − weights − activation_watermark − fixed_terms`, using the architecture-correct kv function (FE8-FE11); reported for fp16/8-bit/4-bit KV side by side, and per concurrency level for serve planning.

Named invariant — **verdict monotonicity**: for a fixed model and plan, increasing `usable` MUST NOT worsen the verdict tier, and increasing `ctx_requested` MUST NOT improve it. Any refactor of the memory model is validated against this property (see [Testing Strategy](#testing-strategy)).

### 8. Performance prediction

- FE21. **Decode (bandwidth roofline):**

```
decode_tps ≈ η_d × BW / ( active_weight_bytes + kv_read_bytes(ctx) )
```

`active_weight_bytes` uses active parameters for MoE; `η_d` (kernel efficiency) shipped 0.65, calibrated per chip/model-class (observed range 0.6-0.85). The model reproduces the known shape: throughput falls as context grows because kv_read grows.
- FE22. **Prefill (compute roofline with capability factor):** prefill_tps is anchored, not derived from raw TFLOPs: shipped anchors per (chip-class, arch-class) scaled by active-parameter ratio, with a NAX multiplier applied only when the tensor-op self-test passes (RFC-0003 IC26). Published anchors used for shipping defaults include Apple's M5 results (4k prompt: 14B-4bit TTFT < 10 s ⇒ ≥ 410 t/s; 30B-A3B TTFT < 3 s ⇒ ≥ 1,365 t/s; 3.3-4.1x over M4) and the M5 generation-speed uplift of 19-27%. All anchors carry provenance and an `est.` flag until locally calibrated.
- FE23. `TTFT_cold ≈ prompt/prefill_tps + c0`; `TTFT_warm ≈ uncached_suffix/prefill_tps + c1` (prefix cache, [RFC-0005](RFC-0005-kv-cache.md#proposed-design)). Load-from-disk time reported separately as `weights_bytes / ssd_bw`.
- FE24. Accuracy targets (product metric M2): memory within 7%, decode within 20%, TTFT within 30% pre-calibration; post-calibration 5/10/15%. Every prediction the CLI prints carries its confidence tier (`measured`, `calibrated`, `modeled`).

### 9. Worked examples (shipped as golden tests)

| Machine | Model | Plan | Outcome |
| --- | --- | --- | --- |
| M2, 16 GB (budget ≈ 10.7 GiB) | Qwen3-8B, MLX 4-bit g64 | weights ≈ 4.2 GiB, KV fp16 144 KiB/t | Comfortable to ~16k ctx; ~38k with 8-bit KV; decode est. ~14-18 t/s |
| M5, 24 GB (probe budget ≈ 18 GiB) | Qwen3-30B-A3B, 4-bit | ≈ 17.3 GiB total at 4k (Apple anchor) | Tight; 8-bit KV recommended beyond 8k; prefill NAX-class |
| M4 Pro, 48 GB (budget ≈ 36 GiB) | Llama-3.3-70B, 4-bit | weights ≈ 37 GiB | Needs tuning: wired raise to 40 GiB (leaves 8 GiB OS) ⇒ Tight, ctx ≈ 8-12k with 8-bit KV, decode est. 5-7 t/s; engine recommends 70B on this machine only for patient workloads and suggests 30B-A3B as the fast alternative |
| M5 Max, 128 GB (budget ≈ 96 GiB) | gpt-oss-120b, MXFP4 | weights ≈ 62 GB | Comfortable; 100k+ ctx with 8-bit KV; decode est. 45-60 t/s (est., 614 GB/s part) |

These rows are golden fixtures, not documentation examples: each is encoded as a test in `drakkar-fit` and CI fails if the engine's verdict, ctx_max band, or decode band drifts (see [Testing Strategy](#testing-strategy)).

### 10. Interfaces

- FE25. CLI: `drakkar fit <model> [--ctx N] [--kv-bits B] [--concurrency C] [--quant Q] [--machine PROFILE] [--json]`. Human output is a compact report card; the same struct serializes to JSON (PRD P7).
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

## Alternatives Considered

**File-size heuristics (RAM ranges).** The nearest prior art shows a coarse min–max RAM range derived from the artifact file size (GGUF rows only). Rejected: file size ignores KV growth with context — the dominant memory term for agentic long-context workloads (10 GiB at 32k for a 70B, FE8) — ignores the GPU-wired cap that is the real budget (FE15/FE16), and ignores architecture (a hybrid-SWA model and a uniform-GQA model of equal file size differ by GiB at long context, FE9). A range that cannot say "fits at 16k, not at 32k" does not answer the product question (PRD P2).

**Trial-loading at runtime.** Load the model, watch RSS, report what happened. Rejected: it requires the multi-gigabyte download the product exists to gate ([PRD §2.3](../../PRD.md#23-where-existing-tools-fall-short) row 1), it reports one point (the tried configuration) instead of the full ctx/precision/concurrency surface the solver produces (FE20), and a failed trial is exactly the mid-generation OOM the engine must prevent (PRD P11). Trial data does enter the system, but through the controlled calibration loop (FE4, FE13, FE14), never as the primary mechanism.

**Static spec-sheet tables only.** Ship a table of per-chip budgets and skip the Metal probe. Rejected by the design stance: macOS revisions move the wired cap (the observed 2/3 vs 3/4 split is not stable across releases, FE15), users change `iogpu.wired_limit_mb`, and live memory pressure matters at admission time. The probe beats the table wherever a probe is possible; the shipped table survives only for offline `--machine` simulation (FE2).

**ML-learned predictor.** Train a regression model over (machine, model, config) → (memory, tps). Rejected for v1: unexplainable predictions cannot be defended to the user (the honest-speed principle requires showing the decomposition in FE26), a learned model is unfalsifiable when it drifts, and it needs a fleet-scale training corpus that does not exist at v0.1. The chosen design — closed-form models plus auditable calibration anchors with provenance (FE22) — is inspectable term by term, and each anchor can be independently re-measured. Revisit only if calibrated closed-form accuracy misses M2 on the RFC-0009 fleet.

## Drawbacks

- **Per-architecture KV accounting is an ongoing maintenance tax.** New attention layouts (MLA variants, new SWA mixes, SSM hybrids, novel MoE routing) ship weekly; each needs its kv(ctx) function and a hand-computed fixture before the engine prices it correctly (PRD §9 names this risk). Mitigation: the architecture registry is data-plus-small-code designed for weekly additions, and an unknown layout degrades explicitly to the uniform formula with a `modeled, layout-unknown` confidence flag rather than failing silently.
- **Anchors go stale without fleet time.** Prefill anchors and η_d ranges (FE21, FE22) are only as good as their last measurement; MLX kernel releases and macOS updates shift them. Until the RFC-0009 hardware fleet runs in CI (v1.0) and users run `bench --calibrate` (v0.2), shipped constants carry `est.` flags and the wider pre-calibration error bands.
- **The probe is macOS-specific by construction.** `recommendedMaxWorkingSetSize`, `iogpu.wired_limit_mb`, and IOKit chip identification have no portable equivalents; a future backend seam (PRD v1.x) would need a per-platform probe layer. Accepted: Apple Silicon is the only target (PRD N1), and the probe-over-table stance is the product's accuracy advantage on that target.
- **Some inputs are unavailable for some repos.** Repos without a safetensors index fall back to estimated weight sizing (FE5); the report's confidence field makes the degradation visible rather than hiding it.

## Migration / Rollout

- **v0.1 "First light":** `drakkar-fit` library; memory model (FE5-FE14), GPU budget probe (FE15-FE17), verdicts and context solver (FE19-FE20), CLI `fit` command and preflight integration (FE25), JSON schema `drakkar.fit/1` (FE26). All performance predictions run on shipped constants and print `modeled` confidence. Admission control is the trivial single-request case (concurrency 1).
- **v0.2 "Convoy":** `drakkar bench --calibrate` populates the calibration store (FE4, RFC-0009 §6); predictions flip to `calibrated` where anchors exist; full admission control against live pool occupancy lands with the continuous-batching scheduler (FE18, FE27); paged-pool overhead term activates with paged KV (FE12). Post-calibration accuracy targets (FE24) become CI-tracked.
- **v0.3 "Fleet":** per-concurrency context solving feeds multi-model pool planning; `/fit` participates in daemon-mode admission.
- **v1.0 "Harbor":** hardware-fleet CI runs the accuracy harness continuously (AC3 as a release gate); the desktop app consumes the same fit structs over the C ABI.
- **Ongoing:** shipped anchors and efficiency factors are revisited at every MLX pin bump (the vendored-MLX update procedure in RFC-0002 includes re-running the FE7 anchor fixtures and the golden examples).
- **Schema versioning:** `drakkar.fit/1` is additive-only; any breaking change mints `drakkar.fit/2` and the server/CLI emit both during a deprecation window of one minor release.

## Testing Strategy

Folds acceptance criteria AC1-AC4 from the source spec.

- **Golden fixtures (AC1):** `fit_anchor_qwen3_8b_4bit`, `fit_anchor_qwen3_30b_a3b_4bit`, `fit_anchor_gpt_oss_20b_mxfp4` reproduce the FE7 Apple-published footprints within 7% in CI on every commit.
- **Golden fixtures (worked examples):** the four §9 rows ship as `fit_golden_m2_16_qwen3_8b`, `fit_golden_m5_24_30b_a3b`, `fit_golden_m4pro_48_llama70b`, `fit_golden_m5max_128_gptoss120b`, each asserting verdict tier, remedy list shape, ctx_max band, and decode band under the shipped constants and the `--machine` simulated profiles.
- **Hand-computed KV fixtures (AC2):** unit tests `kv_uniform_llama31_8b` (128 KiB/t), `kv_uniform_qwen3_*` per the FE8 table, `kv_hybrid_gemma_class` (verifies the fixed SWA term and the kink at ctx = W), `kv_mla_deepseek_lineage` (576-element latent, not per-head), `kv_ssm_hybrid_constant_state` — each checks the full kv(ctx) curve at ctx ∈ {1k, 4k, 8k, 32k, 128k} against values computed by hand in the fixture file.
- **Property tests (verdict monotonicity):** for randomized model descriptors and machine profiles, (a) increasing `usable` never worsens the verdict tier; (b) increasing `ctx_requested` never improves it; (c) `ctx_max(kv4) ≥ ctx_max(kv8) ≥ ctx_max(fp16)`; (d) `total` is monotone non-decreasing in ctx, concurrency, and KV element width.
- **Unit tests:** `bpw_eff` table values (FE5) including the GGUF per-type table; per-tensor recipe estimates match converter output byte counts on a small quantized fixture model (FE6); os_floor tier selection and binding-constraint choice (FE16); wired-limit proposal N respects os_floor and is revertible (FE17).
- **Fuzz:** the `config.json` / safetensors-index parsers run under a fuzzer; arbitrary and truncated inputs MUST produce a structured error from the error taxonomy ([Error Model](../spec/04-error-model.md#2-error-categories-and-the-total-mapping)), never a panic or a garbage estimate presented with confidence.
- **Integration:** `drakkar fit --json` output validates against the `drakkar.fit/1` schema; `POST /fit` returns byte-identical structs for identical inputs (FE27); on the developer machine, the probe path and `--machine` path agree when the profile matches the hardware.
- **Accuracy harness (AC3):** on the RFC-0009 fleet, the harness runs fit-then-measure for the model matrix and tracks prediction error against FE24 targets; post-calibration accuracy is a release gate from v1.0.
- **Soak (AC4):** in the 24-hour mixed-load soak (PRD P14), every plan the engine emitted and executed is checked against the memory contract (RFC-0001 I2): no engine process exceeds its declared budget; a single breach fails the suite.

## Open Questions

None kept open. Two questions from the draft are resolved:

1. Safetensors index fetching — **resolved (LD10):** the preflight always fetches `model.safetensors.index.json` when present, on every `fit`/`run`/`pull` preflight, regardless of repo size. The file is a few KB and converts weight sizing from estimated to exact (FE1).
2. "Aggressive" os_floor profile for headless servers — **resolved as deferred (LD11):** explicit non-goal for v1; the conservative FE16 floors stand. Revisit only on demonstrated demand, tracked as a post-v1.0 item.

## References

- [PRD](../../PRD.md) §1 (vision), §2.3 (gap table), §5.1 P2/P3, §7 M2, §9 (risks)
- [RFC-0001: Architecture](RFC-0001-architecture.md) — invariants I2 (memory contract), I3 (one fit implementation)
- [RFC-0003: Inference Core](RFC-0003-inference-core.md) — IC9 (quant recipes), IC13 (chunked prefill), IC26 (tensor-op self-test)
- [RFC-0005: KV Cache](RFC-0005-kv-cache.md) — §5 (paged-cache overhead), §8 (reclaimable cache at admission)
- [RFC-0006: Model Pipeline](RFC-0006-model-pipeline.md) — metadata acquisition, converter recipe sharing
- [RFC-0007: API Server](RFC-0007-api-server.md) — §8 (`POST /fit`)
- [RFC-0008: CLI and UX](RFC-0008-cli-ux.md) — `fit` command surface, `--json` contract
- [RFC-0009: Performance](RFC-0009-performance.md) — §6 (calibration store), fleet, accuracy harness
- Apple MLR M5/MLX study (footprint and TTFT anchors), Nov 2025; Apple MacBook Pro spec sheets (bandwidth table), 2024-2026
- `MTLDevice.recommendedMaxWorkingSetSize` (Apple Developer docs); llama.cpp discussion #2182; community documentation of `iogpu.wired_limit_mb` defaults (≈ 2/3 vs 3/4 split), persistence patterns, and safety floors (2023-2026)
- DeepSeek-V2/V3 MLA cache layout papers; Gemma/gpt-oss hybrid attention model cards
- LM Studio GGUF RAM-range hints and mlx-serve min-max estimates (prior art for coarse fit UX), 2026
