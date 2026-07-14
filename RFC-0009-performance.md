# RFC-0009: Performance Targets and Benchmark Methodology

**Status:** Draft
**Author:** A. Bakhta
**Created:** 2026-07-14
**Requires:** RFC-0003, RFC-0004, RFC-0007

## 1. Summary

"Blazing fast" and "honest speed" are only meaningful if measured the same way every time. This RFC defines the metrics, the harness (`drakkar bench`), the reference hardware fleet, the per-chip Tier-1 targets, the calibration loop that feeds the feasibility engine, and the CI regression gate. It is the source of every performance number the product is allowed to claim.

## 2. Metric definitions

- PB1. **TTFT** (time to first token): wall time from request submission to first sampled token. Reported **cold** (model resident, cache empty for this prompt) and **warm** (prefix cached). Load-from-disk time is reported separately, never folded into TTFT.
- PB2. **ITL** (inter-token latency) and **TPOT** (time per output token): per-token decode intervals; report p50 and p95 (p95 is the agent-UX metric; means hide the jitter that breaks interactive feel).
- PB3. **Prefill throughput** (tokens/s) and **decode throughput** (tokens/s), measured separately; decode measured at several context lengths because it decays with KV read volume (RFC-0004 FE21).
- PB4. **Peak memory**: max resident engine footprint vs the declared contract (RFC-0001 I2); a breach is a hard failure, not a slow result.
- PB5. **Energy**: tokens/joule from powermetrics sampling; reported because a laptop metric that ignores battery and thermals is dishonest.
- PB6. **Sustained vs burst**: a 30 s burst number and a 5 min sustained number; fanless/14-inch chassis throttle and MUST be reported as such, not hidden behind a best-case burst.
- PB7. **Concurrency**: aggregate throughput and per-stream p95 ITL at 1/2/4/8/16 concurrent streams, with and without shared prefix.

## 3. Harness

- PB8. `drakkar bench <ref> [--workload W] [--calibrate] [--json]` runs fixed workloads with warmup, multiple iterations, and reported variance (median + IQR, never a single run: the ecosystem is full of single-run numbers that do not reproduce, and DRAKKAR explicitly does not add to them).
- PB9. Standard workloads:
  - **A (chat):** 512-token prompt, 256 gen, single stream. Latency baseline.
  - **B (long-context prefill):** 8k/32k/128k prompt, 128 gen. TTFT and prefill scaling; exercises the NAX path.
  - **C (agent concurrency):** shared 8k system/scaffold prefix, 4 and 8 streams, 512 gen each, staggered arrivals. The headline agent metric and the ITL-guard test (RFC-0007 AS13).
  - **D (throughput):** batch of independent 1k-prompt/512-gen requests; aggregate tokens/s.
  - **E (structured output):** JSON-schema-constrained generation; ITL overhead vs unconstrained (RFC-0003 AC3).
- PB10. Golden fixtures include Apple's published M5/MLX anchors (RFC-0004 FE7/FE22) so a machine's numbers are checkable against a public reference.

## 4. Reference fleet

- PB11. Tier-1 (release-gating, physically owned or CI-attached): M4 (base, fanless-class behavior), M4 Pro, M4 Max, M5, M5 Max. These span the bandwidth range (120-614 GB/s) and both NAX-absent (M4) and NAX-present (M5) regimes.
- PB12. Tier-2 (best-effort, community/CI runners): M1/M2/M3 families, M5 Pro, Ultra-class. Used to validate the fit engine's fallback bandwidth table (FE2), not to gate releases.

## 5. Tier-1 targets

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

## 6. Calibration loop

- PB15. `drakkar bench --calibrate` writes `~/.drakkar/calibration/<chip>.json`: measured `η_d` (decode efficiency, RFC-0004 FE21), prefill anchors per arch-class with the NAX multiplier, `runtime_overhead` floor, activation watermarks per arch, and the speculation occupancy crossover (IC21). The feasibility engine prefers calibrated values over shipped defaults and labels predictions `calibrated` (FE24). Calibration is optional for users, mandatory for the fleet before targets are quoted as measured.

## 7. CI regression gate

- PB16. Every release runs workloads A-E on the Tier-1 fleet; a metric regressing > 3% versus the last release blocks the release absent an explicit, reviewed waiver. Memory-contract breaches (PB4) and NAX-path self-test failures (PB14) are non-waivable.
- PB17. Cross-engine baselines are published alongside targets (not gated on): DRAKKAR vs mlx-lm (decode parity target within 5%, RFC-0003 AC1), vs llama.cpp Metal, vs Ollama, vs LM Studio, same model/quant/machine, same harness. The point is honesty and drift detection, not leaderboard theater.

## 8. Acceptance criteria

- AC1. `bench` reproduces a metric within its stated variance across 3 back-to-back runs on the same machine (median stable within IQR).
- AC2. All Tier-1 targets met on owned M4 Pro and M4 Max at v0.2; M5/M5 Max targets confirmed or revised from `est.` to measured when hardware is in the fleet.
- AC3. Calibration improves fit-engine prediction accuracy to the FE24 post-calibration bounds on every Tier-1 machine.
- AC4. CI gate demonstrably blocks a seeded 5% decode regression and a seeded NAX-disable.

## Open questions

1. Add a public, reproducible benchmark manifest (machine, macOS, MLX pin, model hashes) with every published number so third parties can reproduce, following the transparency the M5-era benchmark discourse showed is necessary?
2. Battery-vs-plugged as a reported axis: material on fanless M-class, but doubles the matrix. Report plugged-in as canonical and battery as an annotated note? (Leaning yes.)

## References

- Apple MLR M5/MLX study (public TTFT/generation anchors, 4k prompt, 128 gen), Nov 2025
- Barrios et al., arXiv:2601.19139 (harness design, concurrency scaling, cross-engine methodology on M4 Max)
- Mac O'Clock mlx-lm vs oMLX benchmark (Jun 2026): warm-vs-cold and single-run pitfalls that motivate PB8
- famstack.dev LM Studio prefill-overhead finding (UI t/s vs effective t/s) motivating PB1's load/TTFT separation
- powermetrics (Apple) for PB5; llama.cpp llama-bench as a cross-engine reference harness
