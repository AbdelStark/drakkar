# RFC-0009: Performance Targets and Benchmark Methodology

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.2

## Summary

"Blazing fast" and "honest speed" are only meaningful if measured the same way every time. This RFC defines the metrics, the harness (`drakkar bench`), the reference hardware fleet, the per-chip Tier-1 targets, the calibration loop that feeds the feasibility engine, and the CI regression gate. It is the source of every performance number the product is allowed to claim: this document is the canonical home of the Tier-1 target table, and every other corpus document that quotes a number links here rather than restating it.

## Motivation

The PRD's product principle is **honest speed**: maximum performance the hardware allows, and no number shown to the user that the engine cannot defend ([PRD §1](../../PRD.md#1-vision)). That principle is unenforceable without a single methodology that produces the numbers, states their variance, and blocks releases that regress them.

Four PRD commitments depend directly on this RFC:

- **P8** ([PRD §5.1](../../PRD.md#51-functional)): `drakkar bench` MUST measure TTFT, ITL, prefill and decode throughput, peak memory, and energy per token, and MUST be able to calibrate the feasibility engine's per-chip constants.
- **P10** ([PRD §5.2](../../PRD.md#52-non-functional)): performance floors per chip class are defined here; the headline target (8B dense, 4-bit, 2k prompt / 256 gen: cold TTFT < 1.0 s, decode ≥ 85% of the bandwidth roofline on M4 Pro and newer) is PB13.
- **M2** ([PRD §7](../../PRD.md#7-success-metrics)): fit-report accuracy (memory within 7%, decode within 20%, TTFT within 30% cold, tightening after `bench --calibrate`) is measured on this RFC's model matrix and validated by this RFC's calibration loop against RFC-0004 FE24 ([Feasibility Engine](RFC-0004-feasibility-engine.md#proposed-design)).
- **M3** ([PRD §7](../../PRD.md#7-success-metrics)): all Tier-1 targets met on the reference machines, with a never-regress-more-than-3% release-over-release CI gate — PB16.

The 2025-2026 Apple Silicon benchmark discourse supplies the anti-patterns this RFC is built against: single-run numbers that do not reproduce, warm results published as cold, UI tokens/s that hide prefill overhead, and an engine shipping on M5 hardware with its tensor-op fast path silently disabled. Each has a named countermeasure below (PB8, PB1, PB14).

## Goals

- Define every performance metric DRAKKAR reports, precisely enough that two implementations of the harness would produce comparable numbers (PB1-PB7).
- Ship `drakkar bench` as a first-class subcommand with fixed workloads, warmup, multi-iteration variance reporting, and `--json` output (PB8-PB10).
- Establish a two-tier reference fleet and a per-chip Tier-1 target table that gates releases (PB11-PB14).
- Close the loop between measurement and prediction: `bench --calibrate` writes per-chip constants the feasibility engine prefers over shipped defaults (PB15, RFC-0004 FE24).
- Enforce a CI regression gate: > 3% regression on any Tier-1 metric blocks a release absent a reviewed waiver; memory-contract breaches and NAX-path self-test failures are non-waivable (PB16).
- Attach a reproducibility manifest (machine, macOS version, MLX pin, model hashes) to every published number (LD18).

## Non-Goals

- **Leaderboard marketing.** Cross-engine baselines exist for honesty and drift detection, not ranking theater; they are published, never gated on (PB17).
- **Battery-canonical numbers.** Plugged-in is the canonical benchmark condition; battery is an annotated secondary axis, reported when material (fanless M-class) but never the headline (LD19).
- **Cross-engine gating.** DRAKKAR's release gate compares DRAKKAR against its own last release, not against other engines. PB17 baselines are informational.
- **Benchmarking as a general-purpose profiling product.** `bench` measures the workloads defined here; it is not a flamegraph tool, a Metal shader profiler, or a substitute for `doctor`.
- **Tier-2 hardware gating.** M1/M2/M3-family and Ultra-class numbers validate the fit engine's fallback table (RFC-0004 FE2), best-effort only.

## Proposed Design

### Metric definitions

- PB1. **TTFT** (time to first token): wall time from request submission to first sampled token. Reported **cold** (model resident, cache empty for this prompt) and **warm** (prefix cached). Load-from-disk time is reported separately, never folded into TTFT.
- PB2. **ITL** (inter-token latency) and **TPOT** (time per output token): per-token decode intervals; report p50 and p95 (p95 is the agent-UX metric; means hide the jitter that breaks interactive feel).
- PB3. **Prefill throughput** (tokens/s) and **decode throughput** (tokens/s), measured separately; decode measured at several context lengths because it decays with KV read volume (RFC-0004 FE21, [Feasibility Engine](RFC-0004-feasibility-engine.md#proposed-design)).
- PB4. **Peak memory**: max resident engine footprint vs the declared contract (RFC-0001 I2, [Architecture](RFC-0001-architecture.md#proposed-design)); a breach is a hard failure, not a slow result.
- PB5. **Energy**: tokens/joule from `powermetrics` sampling; reported because a laptop metric that ignores battery and thermals is dishonest. `powermetrics` requires root; the fallback behavior when it is unavailable is specified under [Energy sampling](#energy-sampling) below.
- PB6. **Sustained vs burst**: a 30 s burst number and a 5 min sustained number; fanless/14-inch chassis throttle and MUST be reported as such, not hidden behind a best-case burst.
- PB7. **Concurrency**: aggregate throughput and per-stream p95 ITL at 1/2/4/8/16 concurrent streams, with and without shared prefix.

### Harness

- PB8. `drakkar bench <ref> [--workload W] [--calibrate] [--json]` runs fixed workloads with warmup, multiple iterations, and reported variance (median + IQR, never a single run: the ecosystem is full of single-run numbers that do not reproduce, and DRAKKAR explicitly does not add to them).
- PB9. Standard workloads:
  - **A (chat):** 512-token prompt, 256 gen, single stream. Latency baseline.
  - **B (long-context prefill):** 8k/32k/128k prompt, 128 gen. TTFT and prefill scaling; exercises the NAX path.
  - **C (agent concurrency):** shared 8k system/scaffold prefix, 4 and 8 streams, 512 gen each, staggered arrivals. The headline agent metric and the ITL-guard test (RFC-0007 AS13, [API Server](RFC-0007-api-server.md#proposed-design)).
  - **D (throughput):** batch of independent 1k-prompt/512-gen requests; aggregate tokens/s.
  - **E (structured output):** JSON-schema-constrained generation; ITL overhead vs unconstrained (RFC-0003 AC3, [Inference Core](RFC-0003-inference-core.md#proposed-design)).
- PB10. Golden fixtures include Apple's published M5/MLX anchors (RFC-0004 FE7/FE22) so a machine's numbers are checkable against a public reference.

Workload inputs are deterministic: prompts are fixed byte-for-byte in the harness fixture set, and models are pinned by content hash (RFC-0006 store addressing), so two runs of the same workload on the same machine and release differ only by runtime noise — which the variance reporting (PB8) then quantifies.

The harness schema for a single result record:

```json
{
  "schema": "drakkar.bench.result/1",
  "workload": "C",
  "model": { "ref": "qwen3-30b-a3b-4bit", "weights_sha256": "…", "tokenizer_sha256": "…" },
  "metrics": {
    "ttft_cold_ms":  { "median": 1580, "iqr": [1544, 1631], "n": 5 },
    "itl_p95_ms":    { "median": 43.2, "iqr": [42.1, 44.9], "n": 5 },
    "decode_tps":    { "median": 56.4, "iqr": [55.8, 57.1], "n": 5 },
    "peak_mem_gib":  18.9,
    "tokens_per_joule": { "median": 2.41, "iqr": [2.33, 2.48], "n": 5, "source": "powermetrics" }
  },
  "condition": { "power": "ac", "thermal_pressure_start": "nominal" },
  "manifest": {
    "machine": "Mac16,7 M4 Max 64GB 40c-GPU",
    "macos": "26.2 (25C101)",
    "mlx_pin": "<vendored MLX commit sha>",
    "drakkar": "0.2.0 (<git sha>)",
    "model_hashes": ["sha256:…"]
  }
}
```

The `manifest` block is mandatory on every published record (LD18): a result without a complete manifest MUST NOT be published or compared by the CI gate.

#### Energy sampling

`powermetrics` (the only supported source for PB5) requires root. Behavior:

1. **Fleet CI:** runners carry a scoped sudoers entry for `powermetrics` only; energy is always sampled and PB5 is a reported (not gated) metric.
2. **User machines, no root:** `bench` detects the permission failure, emits `tokens_per_joule: null` with `"source": "unavailable(no-root)"` in JSON, prints a one-line note with the sudo invocation to enable it, and completes the run. Energy absence never fails a bench run.
3. `bench` never invokes `sudo` itself and never prompts for elevation.

### Reference fleet

- PB11. Tier-1 (release-gating, physically owned or CI-attached): M4 (base, fanless-class behavior), M4 Pro, M4 Max, M5, M5 Max. These span the bandwidth range (120-614 GB/s) and both NAX-absent (M4) and NAX-present (M5) regimes.
- PB12. Tier-2 (best-effort, community/CI runners): M1/M2/M3 families, M5 Pro, Ultra-class. Used to validate the fit engine's fallback bandwidth table (RFC-0004 FE2), not to gate releases.

Fleet machines run with a controlled environment: AC power (LD19), lid open, display sleep disabled, no other GPU clients, thermal state confirmed `nominal` before each workload (a run starting under thermal pressure is retried after cooldown, and the retry is logged in the manifest).

### Tier-1 targets

Reference model set: Qwen3-8B (dense, 4-bit) and Qwen3-30B-A3B (MoE, 4-bit) as primary; Llama-3.3-70B-4bit on 64 GB+ machines. Targets are for the resident-model, warm-engine case. Values marked `est.` are modeled pending first-fleet measurement; the CI gate (PB16) locks them to measured values once established.

| Metric (workload) | M4 | M4 Pro | M4 Max | M5 | M5 Max |
| --- | --- | --- | --- | --- | --- |
| 8B cold TTFT, 2k prompt (A/B) | < 1.8 s | < 1.0 s | < 0.7 s | < 0.9 s est. | < 0.35 s est. |
| 8B decode t/s, short ctx (A) | ≥ 22 | ≥ 48 | ≥ 85 | ≥ 28 est. | ≥ 95 est. |
| 8B decode, % of bandwidth roofline | ≥ 80% | ≥ 85% | ≥ 85% | ≥ 85% est. | ≥ 85% est. |
| 30B-A3B cold TTFT, 4k prompt (B) | n/a (mem) | < 3.0 s | < 1.6 s | < 3.0 s (Apple) | < 0.9 s est. |
| 30B-A3B decode t/s (A) | n/a | ≥ 35 | ≥ 55 | ≥ 40 est. | ≥ 75 est. |
| Agent: 4-stream shared-prefix p95 ITL (C) | - | < 60 ms | < 45 ms | < 55 ms est. | < 30 ms est. |
| Warm TTFT, 8k cached + 64 new (C) | < 400 ms | < 250 ms | < 180 ms | < 220 ms est. | < 120 ms est. |

- PB13. Headline product claim (PRD P10): 8B/4-bit A-workload, cold TTFT < 1.0 s and decode ≥ 85% of roofline on M4 Pro and newer. Everything else in the table supports or extends it.
- PB14. M5-family prefill MUST demonstrate the NAX tensor-op advantage: B-workload prefill ≥ 3x the M4-class figure at equal model/quant (RFC-0003 AC2), and a self-test failure (NAX path silently off) fails the gate rather than quietly regressing (the LM Studio M5 incident is the anti-pattern).

### Calibration loop

- PB15. `drakkar bench --calibrate` writes `~/.drakkar/calibration/<chip>.json`: measured `η_d` (decode efficiency, RFC-0004 FE21), prefill anchors per arch-class with the NAX multiplier, `runtime_overhead` floor, activation watermarks per arch, and the speculation occupancy crossover (RFC-0003 IC21). The feasibility engine prefers calibrated values over shipped defaults and labels predictions `calibrated` (RFC-0004 FE24). Calibration is optional for users, mandatory for the fleet before targets are quoted as measured.

### CI regression gate

- PB16. Every release runs workloads A-E on the Tier-1 fleet; a metric regressing > 3% versus the last release blocks the release absent an explicit, reviewed waiver. Memory-contract breaches (PB4) and NAX-path self-test failures (PB14) are non-waivable.
- PB17. Cross-engine baselines are published alongside targets (not gated on): DRAKKAR vs mlx-lm (decode parity target within 5%, RFC-0003 AC1), vs llama.cpp Metal, vs Ollama, vs LM Studio, same model/quant/machine, same harness. The point is honesty and drift detection, not leaderboard theater.

### Named bench work items (v0.2)

Two locked decisions route their resolution through this harness:

- **KV block-size ablation (LD7):** 16-token vs 32-token KV blocks on workloads A/C/D across the owned Tier-1 machines; decides whether RFC-0005's 32-token default stands ([KV Cache](RFC-0005-kv-cache.md#proposed-design)).
- **Paged-attention kernel spike (LD20):** prototype-both comparison (in-house fused paged varlen kernel vs adopting vllm-metal's) on workloads C/D, resolving RFC-0003's build-vs-adopt question ([Inference Core](RFC-0003-inference-core.md#proposed-design)).

## Alternatives Considered

### Adopt llama-bench or a third-party harness as the gate

llama.cpp's `llama-bench` and the arXiv:2601.19139 harness are mature and give genuine cross-engine comparability. Rejected as the gating harness: neither can measure DRAKKAR-specific semantics — warm-prefix TTFT through the paged CoW cache (PB1 warm, workload C), the chunked-prefill ITL guard (RFC-0007 AS13), constrained-decoding overhead against DRAKKAR's grammar engine (workload E), or tokens/joule under DRAKKAR's admission control. A gate that cannot see the product's differentiating paths cannot protect them. Kept as a cross-check: PB17 runs external engines under the same conditions and publishes the comparison, and `llama-bench` remains a sanity reference for the shared metrics.

### Publish single-run numbers

The prevailing ecosystem practice, and rejected explicitly. Single runs conflate warm and cold state, catch thermal luck, and do not reproduce (the mlx-lm vs oMLX benchmark episode that motivates PB8). Every DRAKKAR number is a median with IQR over multiple iterations after warmup, and the harness refuses to emit a publishable record from `n = 1`.

### Synthetic microbenchmarks as targets

Kernel-level microbenchmarks (isolated GEMM, attention-only loops) are useful for optimization work but rejected as targets: they do not predict end-to-end behavior through the scheduler, sampler, cache, and HTTP path, and they invite optimizing the benchmark instead of the product. Workloads A-E are end-to-end request shapes that agents and users actually produce; microbenchmarks stay as internal engineering tools with no published numbers.

### Cloud CI runners for performance

Hosted macOS CI is fine for correctness but rejected for performance gating: virtualized Metal is unrepresentative (shared GPU scheduling, different thermal envelope, no control over the wired-memory state), and hosted fleets do not offer the exact Tier-1 chip/RAM matrix. The gate runs on physically owned, CI-attached machines (PB11); correctness CI stays on hosted runners.

## Drawbacks

- **Fleet cost and maintenance.** Five owned Tier-1 machines plus runner plumbing is real capital and ongoing toil (macOS updates change numbers; every fleet OS bump is a re-baseline event recorded in the manifest history). Accepted: it is the price of numbers the product can defend, and the roadmap staggers the spend (see Migration / Rollout).
- **`est.` targets are commitments made before measurement.** The M5 and M5 Max columns are modeled from published anchors until those machines join the fleet; a modeled target can be wrong in either direction and revising it publicly costs credibility. Mitigated by the `est.` marker discipline (never quoted without it) and the AC2 conversion step.
- **Energy sampling needs root.** `powermetrics` is the only honest source for PB5 and requires sudo; most users will run `bench` without energy data (the fallback in [Energy sampling](#energy-sampling)). Accepted: an inaccurate userspace estimate would be worse than an honest `null`.
- **The 3% gate has a variance floor.** On noisy metrics (p95 ITL under concurrency) a 3% threshold approaches run-to-run IQR; the gate compares medians and AC1 bounds the variance, but occasional false trips will require reruns. Accepted over widening the threshold, which would let real regressions accumulate.

## Migration / Rollout

- **v0.1 "First light":** no `bench` subcommand and no performance gate. Development uses informal harness prototypes to sanity-check the fit engine's estimates; no number from this phase is published.
- **v0.2 "Convoy"** (this RFC's target): `drakkar bench` ships with workloads A-E, `--json`, variance reporting, and `--calibrate` (PB8-PB10, PB15). The CI regression gate (PB16) activates on the owned M4 Pro and M4 Max; M4-base joins when acquired within the milestone. M5 and M5 Max columns remain `est.` until fleet hardware lands, and are excluded from the gate until converted to measured (AC2). LD7 block-size ablation and LD20 kernel spike run in this milestone. Result schema `drakkar.bench.result/1` is frozen; additive fields only within v0.x.
- **v0.3 "Fleet":** bench coverage extends to the SSD KV tier and multi-model pool (workload C variants with cache-tier eviction in play); Tier-2 community submissions accepted with mandatory manifests.
- **v1.0 "Harbor":** full Tier-1 fleet (all five chips) physically attached to CI; targets table fully measured, `est.` markers gone; the docs site publishes per-release benchmark reports generated from CI manifests.

## Testing Strategy

Acceptance criteria (release-gating for v0.2 unless noted):

- AC1. `bench` reproduces a metric within its stated variance across 3 back-to-back runs on the same machine (median stable within IQR).
- AC2. All Tier-1 targets met on owned M4 Pro and M4 Max at v0.2; M5/M5 Max targets confirmed or revised from `est.` to measured when hardware is in the fleet.
- AC3. Calibration improves fit-engine prediction accuracy to the FE24 post-calibration bounds on every Tier-1 machine.
- AC4. CI gate demonstrably blocks a seeded 5% decode regression and a seeded NAX-disable.

Named test cases:

- **Harness self-test / variance stability (AC1):** `bench_selftest_variance` runs workload A three times back-to-back on a fleet machine and asserts the medians of TTFT, decode t/s, and p95 ITL are mutually within each run's IQR. Runs nightly on the fleet; a failure marks the machine's results untrusted until investigated.
- **Seeded-regression drills (AC4):** `gate_drill_decode` builds the release candidate with an injected 5% decode-path slowdown (a hidden debug flag adding per-step stalls) and asserts PB16 blocks it; `gate_drill_nax` forces the tensor-op self-test off (RFC-0003 IC26 path) and asserts the non-waivable PB14 failure fires. Both drills run against the real gate pipeline, not a mock, before each release branch cut.
- **Calibration round-trip (AC3):** `calibrate_roundtrip` runs `bench --calibrate`, then re-runs `drakkar fit` for the reference model set and asserts predictions carry the `calibrated` tier and land within RFC-0004 FE24 post-calibration bounds (5/10/15%) against the just-measured values.
- **Manifest completeness (LD18):** `manifest_schema_check` validates every result record emitted by fleet CI against `drakkar.bench.result/1` and fails the pipeline if any published record lacks machine identity, macOS build, MLX pin, DRAKKAR version, or model hashes. Property test: no code path in `bench` can emit a publishable record with a partial manifest.
- **Workload determinism:** `workload_fixture_hash` asserts the prompt fixtures for A-E hash to their pinned values and the referenced models resolve to their pinned content hashes before any measurement starts; a mismatch aborts the run with a fixture-drift error rather than producing an incomparable number.
- **Golden anchors (PB10):** `anchor_apple_m5` compares fleet M5 results against Apple's published MLX envelope (via RFC-0004 FE7/FE22 fixtures) and flags divergence beyond the anchors' stated tolerance as a harness-or-regression investigation, not an auto-fail.
- **Energy fallback:** `energy_no_root` runs `bench` without root and asserts the run completes, JSON carries `tokens_per_joule: null` with the `unavailable(no-root)` source, and exit code is success.
- **Soak tie-in:** the PRD P14 24-hour soak runs a workload-C loop and feeds its RSS-drift and failure counters through the same result schema, so soak regressions surface in the same gate history.

## Open Questions

Resolved since draft:

- **Reproducibility manifest — resolved (LD18):** every published benchmark number carries a manifest (machine, macOS, MLX pin, model hashes); enforced by `manifest_schema_check` above.
- **Battery vs plugged — resolved (LD19):** plugged-in is the canonical condition; battery is an annotated secondary axis, reported where material (fanless M-class) and never the headline.

Kept open:

- OPEN QUESTION: exact M5-family target revision — the M5 and M5 Max columns are modeled (`est.`) from Apple's published anchors and must be confirmed or revised from real hardware. Owner: abdelstark. Resolution path: first fleet run on owned M5-class hardware, converting `est.` to measured per AC2. Target: v0.2 (gate exclusion lifts on conversion).

## References

- [PRD](../../PRD.md) — §1 honest speed, P8/P10, M2/M3/M4, §8 roadmap
- [RFC-0001: Architecture](RFC-0001-architecture.md) — I2 memory contract
- [RFC-0003: Inference Core](RFC-0003-inference-core.md) — AC1 mlx-lm parity, AC2 NAX prefill, AC3 constrained-output overhead, IC21 speculation crossover, IC26 tensor-op self-test
- [RFC-0004: Feasibility Engine](RFC-0004-feasibility-engine.md) — FE2 bandwidth table, FE7/FE22 anchors, FE21 decode roofline, FE24 accuracy tiers
- [RFC-0005: KV Cache](RFC-0005-kv-cache.md) — LD7 block-size ablation input
- [RFC-0007: API Server](RFC-0007-api-server.md) — AS13 ITL guard
- Apple MLR M5/MLX study (public TTFT/generation anchors, 4k prompt, 128 gen), Nov 2025
- Barrios et al., arXiv:2601.19139 (harness design, concurrency scaling, cross-engine methodology on M4 Max)
- Mac O'Clock mlx-lm vs oMLX benchmark (Jun 2026): warm-vs-cold and single-run pitfalls that motivate PB8
- famstack.dev LM Studio prefill-overhead finding (UI t/s vs effective t/s) motivating PB1's load/TTFT separation
- powermetrics (Apple) for PB5; llama.cpp llama-bench as a cross-engine reference harness (see Alternatives Considered)
