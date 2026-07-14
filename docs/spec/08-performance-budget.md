# 08 — Performance Budget

- Part of the DRAKKAR specification corpus. Index: [SPEC.md](../../SPEC.md).
- Sources of authority: [PRD](../../PRD.md) §5.2 (P10–P12), §7 (M2–M4); [RFC-0009](../rfcs/RFC-0009-performance.md) (canonical metric definitions, harness, fleet, full Tier-1 matrix); [RFC-0003](../rfcs/RFC-0003-inference-core.md) AC1–AC2; [RFC-0007](../rfcs/RFC-0007-api-server.md) AS5/AS13; [RFC-0004](../rfcs/RFC-0004-feasibility-engine.md) FE13–FE24.
- Status: Accepted. Author: abdelstark. Date: 2026-07-14.

This document is the product performance contract in one place: the budgets DRAKKAR must meet, how each end-to-end budget decomposes into per-stage allowances, how the memory budget is computed, how and when the system is profiled, and the regression policy that keeps every number defensible release over release. Methodology (metric definitions PB1–PB7, workloads PB9, fleet PB11–PB12, the full per-chip target matrix, calibration PB15) is defined once in RFC-0009 and is not duplicated here; this document links to it and adds the budget decompositions and engineering process that the RFC delegates to the spec.

Terminology: TTFT, ITL, TPOT, prefill/decode throughput, peak memory, energy, sustained-vs-burst, and concurrency metrics are defined in RFC-0009 PB1–PB7 ([Metric definitions](../rfcs/RFC-0009-performance.md#metric-definitions)). In particular, TTFT ends at the **first sampled token** (PB1); transport of the first SSE frame is budgeted separately (AS5). Load-from-disk time is never folded into TTFT (PB1).

## 1. Headline budgets

**PBU1.** The table below is the binding product performance contract. Each row is a release gate at the milestone shown, measured with the RFC-0009 harness under its canonical conditions (resident model, warm engine, plugged in per LD19, median + IQR over ≥ 3 runs per RFC-0009 AC1). Values marked `est.` are modeled pending first-fleet measurement and convert to measured gates per RFC-0009 AC2. The full per-chip matrix (M4 through M5 Max, both reference models) lives in RFC-0009 ([Tier-1 targets](../rfcs/RFC-0009-performance.md#tier-1-targets)); this table is the distilled contract.

| # | Budget | Value | Condition | Gate from | Source |
| --- | --- | --- | --- | --- | --- |
| H1 | Cold TTFT, headline | < 1.0 s | 8B dense 4-bit, 2k prompt, workload A, M4 Pro and newer (M4 base: < 1.8 s) | v0.2 | PRD P10; RFC-0009 PB13 |
| H2 | Decode vs roofline | ≥ 85% of bandwidth-derived roofline | same as H1 (M4 base: ≥ 80%) | v0.2 | PRD P10; RFC-0009 PB13; FE21 |
| H3 | Decode parity | within 5% of mlx-lm decode | same model/quant/machine, single stream, RFC-0009 matrix | v0.1 | RFC-0003 AC1; RFC-0009 PB17 |
| H4 | NAX prefill uplift | ≥ 3x M4-class prefill t/s | M5-family, 4k prompt, 8B reference, workload B; functional self-test MUST pass | v0.2 (when M5 in fleet) | RFC-0003 AC2; RFC-0009 PB14 |
| H5 | Warm-prefix TTFT | ≤ 250 ms (M4 Pro) / ≤ 180 ms (M4 Max) / ≤ 400 ms (M4); product ceiling < 500 ms | 8k cached prefix + 64 new tokens, workload C | v0.2 | RFC-0009 §5; PRD M4; RFC-0005 AC2 |
| H6 | Agent concurrency ITL | per-stream p95 ITL < 45 ms | 4 streams, shared 8k prefix, 30B-A3B 4-bit, M4 Max, workload C (M4 Pro: < 60 ms) | v0.2 | PRD M4; RFC-0007 AC5; RFC-0009 §5 |
| H7 | ITL guard | p95 ITL inflation ≤ 25% vs solo decode | one 32k prefill admitted mid-stream against a running decode, reference concurrency | v0.2 | RFC-0007 AS13/AC2 |
| H8 | First streamed frame | ≤ 50 ms after first sampled token; chunk coalescing ≤ 10 ms | any streaming request | v0.1 | RFC-0007 AS5 |
| H9 | Startup | < 200 ms binary launch to server-ready | model already resident | v0.1 | PRD P12 |
| H10 | Model load | wall time ≤ 1.25 × `weights_bytes / ssd_bw_measured` | mmap load from content store, `ssd_bw_measured` from the doctor probe (FE2) | v0.1 | PRD P12; RFC-0003 IC24; derived, see PBU2 |
| H11 | Structured-output overhead | ≤ 8% ITL overhead vs unconstrained, batch 1 | workload E | v0.2 | RFC-0003 AC3; RFC-0009 PB9 |
| H12 | Memory contract | peak resident footprint ≤ declared budget, always | all workloads; a breach is a hard failure, not a slow result | v0.1 | PRD P11; RFC-0009 PB4; RFC-0001 I2 |
| H13 | Soak | 24 h mixed load: zero request failures, RSS drift < 2% post-warmup | soak suite | v1.0 | PRD P14; RFC-0003 AC5 |
| H14 | Release-over-release | no Tier-1 metric regresses > 3% | workloads A–E, Tier-1 fleet | every release from v0.2 | PRD M3; RFC-0009 PB16 |

**PBU2** (derivation of H10). PRD P12 states "model load bounded by SSD bandwidth"; the enforceable form is: load wall time MUST NOT exceed 1.25 × the ideal transfer time of the weight bytes at the machine's measured sequential-read bandwidth, i.e. the loader achieves ≥ 80% of SSD bandwidth end-to-end (mmap fault-in, Metal residency wiring, tokenizer and metadata load included). Reference point: 55 GB at 14.5 GB/s is ~3.8 s ideal, ≤ 4.75 s budgeted (RFC-0003 IC24). The 25% allowance covers filesystem and residency-wiring overheads measured on APFS; if v0.1 profiling shows the real overhead is materially lower, the allowance tightens to match measurement (regression gate H14 then holds the improved number).

Energy (PB5) and sustained-vs-burst (PB6) are **reported, not gated** in v1: tokens/joule and the 30 s burst / 5 min sustained pair appear in every published benchmark, with fanless-chassis throttling disclosed as such. Gating them requires fleet thermal baselines that do not exist before v1.0 hardware CI; the honesty obligation (publish, never hide) applies from the first `drakkar bench` release.

## 2. Memory budget arithmetic

The memory budget is owned by RFC-0004; this section states the identities the rest of the corpus (and this document's H12) depends on. The fit engine is the single implementation (RFC-0001 I3); nothing in the server or scheduler re-derives these numbers.

Master identity (RFC-0004 §5, [Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)):

```
total = weights                                   # FE5-FE7: exact from safetensors index, else bpw_eff estimate
      + kv_pool(ctx, concurrency, kv_precision)   # FE8-FE12: architecture-aware (GQA / SWA-hybrid / MLA / SSM)
      + activation_watermark(chunk, hidden, L, B) # FE13: bounded by chunked prefill (IC13), measured on first load
      + runtime_overhead                          # FE14: shipped 1.2 GiB, calibrated to the machine's RSS floor
      + draft_model_bytes                         # if speculation (IC19)
      + fragmentation_margin                      # 3% of the sum above
```

Budget identity (FE15–FE16):

```
budget = live MTLDevice.recommendedMaxWorkingSetSize        # probe, never hardcoded
usable = budget − runtime_overhead − fragmentation_margin
```

subject additionally to the **os_floor** constraint: the plan MUST leave the following un-wired system RAM, whichever constraint (usable or os_floor) binds first winning:

| Machine RAM | os_floor |
| --- | --- |
| ≤ 16 GB | 4 GiB |
| 24–36 GB | 6 GiB |
| 48–64 GB | 8 GiB |
| ≥ 96 GB | 12 GiB |

An "aggressive" os_floor profile for headless servers is an explicit v1 non-goal (LD11). Wired-limit raises (`iogpu.wired_limit_mb`) are proposed with exact values and never auto-applied (FE17).

Consequences enforced elsewhere but budgeted here:

- The KV pool is carved up-front from `usable − weights − activation_watermark − fixed` at load (RFC-0005 KV2); growth beyond it is impossible by construction. Pool metadata (block tables, per-block scales) is charged at ≈ 96 B/block (FE12).
- Admission control runs the same arithmetic against live occupancy (FE18): a request that does not fit is rejected with `max_tokens_admissible`, never admitted to die of memory (PRD P11).
- Verdict thresholds (Comfortable ≤ 0.85 × usable, Tight ≤ usable, then remedies) are FE19; the 0.85 factor is the product's standing headroom policy and MUST NOT be tuned per-machine.
- H12 test form: peak RSS and peak Metal-wired bytes are sampled by the harness in every workload run and compared against the declared contract; any breach fails the run regardless of speed (PB4), and no plan emitted by the fit engine may breach it in the soak suite (RFC-0004 AC4).

## 3. Latency decomposition budgets

End-to-end targets are useless for debugging unless each stage has an allowance. The decompositions below are engineering budgets **derived** from H1/H5: the fixed (non-prefill) overheads get an explicit ceiling, and prefill receives the entire remainder, because prefill is the only stage whose cost legitimately scales with the workload. Stage timings are captured as `tracing` spans (RFC-0007 AS22) and reported by `drakkar bench --json` so a blown budget names its stage.

**PBU3** (cold TTFT decomposition, workload A: 8B 4-bit, 2k prompt, single stream; budget column shown for the M4 Pro 1.0 s gate). The fixed-overhead constant `c0` (FE23) MUST NOT exceed 30 ms; every stage budget below is `est.` until v0.1 profiling locks it.

| Stage | Budget | Notes |
| --- | --- | --- |
| HTTP read, parse, dialect normalization | ≤ 3 ms | either dialect → one `GenerationRequest` (AS1) |
| Tokenization + chat-template render (2k tokens) | ≤ 10 ms | linear in prompt length; long-context (B) budgets scale pro rata |
| Prefix-cache lookup (radix over hash chain) | ≤ 2 ms | KV9–KV10; cold path = miss |
| Admission + first prefill chunk dispatch | ≤ 10 ms | FE18 arithmetic + one scheduler tick |
| First-token sample + readback | ≤ 5 ms | sampled ids only, never full logits (IC4) |
| **Fixed total (`c0`)** | **≤ 30 ms** | |
| Prefill (2,048 tokens) | remainder: ≥ 970 ms available | the only scaling term; NAX path where capable (IC12) |

**PBU4** (warm TTFT decomposition, workload C warm hit: 8k cached prefix + 64 new tokens; budget column for the M4 Pro 250 ms gate). The warm constant `c1` (FE23) MUST NOT exceed 40 ms. Note the full prompt is tokenized and hashed even on a hit — prefix identity is content-addressed (KV9, KV12) — so tokenization does not shrink on the warm path.

| Stage | Budget | Notes |
| --- | --- | --- |
| HTTP read, parse, normalization | ≤ 3 ms | |
| Tokenization + hash-chain computation (8k tokens) | ≤ 15 ms | |
| Radix lookup + block attach (refcount, CoW arm) | ≤ 5 ms | KV4, KV10; no data copy on attach |
| Admission + dispatch | ≤ 10 ms | |
| First-token sample + readback | ≤ 5 ms | |
| **Fixed total (`c1`)** | **≤ 40 ms** | |
| Prefill (64 uncached tokens) | remainder: ≥ 210 ms available | one chunk; RFC-0005 AC2 bounds the whole warm TTFT at ≤ 1.15 × (64-token prefill + c1) |

Post-TTFT transport: the first streamed frame MUST render and flush within 50 ms of the first sampled token, with chunk coalescing capped at 10 ms (H8, AS5). This sits outside TTFT by definition (PB1) but inside the user's perceived latency; the harness reports both numbers.

Startup (H9) decomposes as: binary launch + config parse + state-dir open ≤ 120 ms, socket bind + route table ≤ 30 ms, engine attach to the already-resident model ≤ 50 ms. These sub-budgets are `est.`; the v0.1 flamegraph pass (§4) locks them.

## 4. Profiling plan

**PBU5.** Every optimization PR (area labels `bench`, or any PR whose description claims a performance effect) MUST cite a before/after `drakkar bench --json` run pair from the RFC-0009 harness on at least one Tier-1 machine: same workload(s), median + IQR (never a single run, PB8), and the LD18 reproducibility manifest (machine, macOS build, MLX pin, model content hashes) for both runs. A claimed win that does not reproduce under the harness does not merge. Micro-benchmarks and profiler screenshots are supporting evidence, not a substitute.

**PBU6** (what gets profiled when). Tools: Instruments with the Metal System Trace template (GPU occupancy, kernel gaps, chunk scheduling), `powermetrics` (tokens/joule, thermal/sustained behavior, PB5–PB6), `cargo flamegraph` (Rust control-plane CPU: tokenizer, scheduler tick, SSE hot path, startup), MLX built-in timing and compile-cache metadata (per-graph eval time, compilation churn against IC2's bucket policy), and `drakkar bench` itself as the end-to-end source of truth.

| Milestone | Profiled | Primary instruments |
| --- | --- | --- |
| v0.1 "First light" | Startup path vs H9 sub-budgets; loader vs PBU2 (mmap fault-in, residency wiring); single-stream A/B; PBU3 stage spans; decode parity vs mlx-lm (H3) | cargo flamegraph, MLX timing, bench A/B, Metal System Trace |
| v0.2 "Convoy" | Full A–E Tier-1 matrix; ITL-guard behavior under C (H6/H7); chunked-prefill kernel occupancy incl. NAX path (H4); KV block-size 16-vs-32 ablation (LD7); gather-based vs fused paged-attention spike (LD20); speculation occupancy crossover (IC21); first fleet calibration (PB15) | Metal System Trace, bench --calibrate, powermetrics |
| v0.3 "Fleet" | SSD KV-tier restore bandwidth and the ≥ 3x-vs-recompute eligibility rule (KV18); daemon idle footprint; multi-model pool residency transitions (LD12 data) | bench, powermetrics, Instruments (allocations) |
| v1.0 "Harbor" | Hardware-fleet CI running the full gate on every release candidate; sustained/energy reporting on fanless chassis; 24 h soak (H13) | fleet CI harness, powermetrics, soak suite |

Profiling condition discipline: plugged in is canonical, battery is an annotated secondary axis (LD19); resident-model warm-engine unless the metric is load or startup; M5-family runs MUST record the tensor-op self-test result (IC26) in the manifest so a silently-disabled NAX path can never masquerade as a regression elsewhere.

## 5. Regression policy

**PBU7.** The CI gate is RFC-0009 PB16 ([CI regression gate](../rfcs/RFC-0009-performance.md#ci-regression-gate)); its operational rules are:

- Every release candidate runs workloads A–E on the Tier-1 fleet. The comparison base is the stored bench manifest set of the last released tag, matched per (machine, model, workload, metric).
- A metric regressing **> 3%** on medians (with non-overlapping IQRs; an overlap within stated variance is a re-run, not a failure) blocks the release.
- Waivers exist but are loud: an explicit, reviewed waiver names the metric, the cause, the plan to recover, and appears in the release notes. A waiver never carries over to the next release automatically.
- **Non-waivable classes:** memory-contract breaches (H12/PB4) and NAX-path self-test failures (H4/PB14). These are correctness failures wearing a performance costume; no review can accept them.
- The gate itself is tested: CI demonstrably blocks a seeded 5% decode regression and a seeded NAX-disable (RFC-0009 AC4). A gate that has never caught a planted fault is decoration.
- Cross-engine baselines (vs mlx-lm, llama.cpp Metal, and the incumbent servers, same model/quant/machine/harness) are published alongside every release for drift detection but are not gated on, except H3 which is a DRAKKAR acceptance criterion in its own right (PB17, RFC-0003 AC1).

Every published number — release notes, README, docs site — carries its LD18 manifest. A number without a manifest is not published.

## 6. Fit-engine accuracy budget

**PBU8.** The feasibility engine's predictions are product claims and carry their own budget (PRD M2; RFC-0004 FE24; gated fleet-wide by RFC-0009 AC3):

| Prediction | Pre-calibration bound | Post-`bench --calibrate` bound |
| --- | --- | --- |
| Peak memory | within 7% of measured | within 5% |
| Decode throughput | within 20% | within 10% |
| Cold TTFT | within 30% | within 15% |

Enforcement:

- The FE7 anchor fixtures (published MLX footprints for the reference models) MUST reproduce within 7% in CI on every commit (RFC-0004 AC1); they are the pre-calibration floor that ships in the binary.
- `drakkar bench --calibrate` (PB15) writes the per-chip calibration store — measured `η_d`, prefill anchors with the NAX multiplier, `runtime_overhead` floor, activation watermarks, speculation crossover — and the fit engine prefers it over shipped defaults. Calibration is optional for users, mandatory for the fleet before any target is quoted as measured.
- Every prediction surfaced in the CLI, `/fit`, or JSON carries its confidence tier (`measured` / `calibrated` / `modeled`, FE24); the accuracy budget applies per tier, and the harness verifies the post-calibration column on every Tier-1 machine at v0.2 (RFC-0009 AC3).
- Prediction accuracy is itself under the H14 regression gate: a model-def or estimator change that pushes any Tier-1 machine outside its bound blocks the release.

## 7. Requirement index

| ID | One-line statement |
| --- | --- |
| PBU1 | The §1 table is the binding, milestone-gated product performance contract; canonical conditions per RFC-0009. |
| PBU2 | Model load achieves ≥ 80% of measured SSD sequential bandwidth end-to-end (wall ≤ 1.25 × ideal). |
| PBU3 | Cold TTFT fixed overhead `c0` ≤ 30 ms with per-stage budgets; prefill gets the remainder. |
| PBU4 | Warm TTFT fixed overhead `c1` ≤ 40 ms with per-stage budgets; full-prompt tokenize+hash stays on the warm path. |
| PBU5 | Every optimization PR cites a reproducible before/after harness run pair with LD18 manifests; no manifest, no merge. |
| PBU6 | Milestone-scoped profiling plan (tools × targets) as tabulated in §4. |
| PBU7 | 3% release-over-release gate on medians with IQR discipline; memory-contract and NAX-self-test failures are non-waivable. |
| PBU8 | Fit-engine accuracy bounds 7/20/30% pre-calibration and 5/10/15% post, enforced per confidence tier and fleet-gated. |

## Open questions

None. The two open questions of the source RFC are resolved by the locked decisions LD18 (reproducibility manifests on every published number) and LD19 (plugged-in canonical, battery annotated); the KV block-size ablation (LD7) and the paged-kernel build-vs-adopt spike (LD20) are named v0.2 bench work items tracked in §4, owner abdelstark.
