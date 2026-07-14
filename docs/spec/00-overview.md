# 00 — Overview

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Applies to: all milestones (v0.1 "First light" through v1.0 "Harbor")
- Source of truth: [PRD](../../PRD.md); this document is the corpus entry point

## Thesis

DRAKKAR is a native, single-binary LLM inference engine for Apple Silicon that makes
**feasibility a first-class product feature**. Given a Hugging Face model reference, it
computes — before downloading a byte — whether the model fits the machine, what quantization
it needs, what context window the GPU-wired memory budget affords, and what time-to-first-token
(TTFT) and decode throughput to expect. It then downloads, prepares, and serves the model at
the hardware limit through OpenAI- and Anthropic-compatible endpoints, with a KV cache
subsystem built for the multi-request, long-context reality of agentic workloads.

Architecturally, DRAKKAR is a Rust control plane over a vendored MLX compute core reached
through a thin C++ shim behind a stable C ABI (RFC-0001, RFC-0002, RFC-0010). The kernel race
on Apple GPUs is effectively decided: MLX's Metal kernels, maintained by Apple and first to
exploit new silicon (M5 Neural Accelerators via Metal 4 Tensor Operations), are the substrate
to build on, not compete with. DRAKKAR competes one layer up: packaging, resource
intelligence, agent-grade serving, and a coherent one-command experience
([PRD §2.2](../../PRD.md#22-software-landscape)).

Two decisions are locked at the product level and MUST be reflected consistently across the
corpus:

- **LD1 — License.** The engine repository is licensed **Apache-2.0** (patent grant; PRD open
  question 2 is resolved). Release mechanics live in
  [09 — Release](09-release-and-versioning.md) and [RFC-0012](../rfcs/RFC-0012-release-engineering.md).
- **LD2 — Name.** DRAKKAR is a **working codename**. Trademark screening (known conflict
  surfaces: fragrance and gaming uses) is a filed v1.0 work item, not a blocker for any
  earlier milestone. No document in this corpus may treat the name as final.

## Problem statement

Every MacBook Pro sold since 2021 is a capable inference machine; the M5 generation (Neural
Accelerators in every GPU core, up to 128 GiB unified memory at 614 GB/s) made it a serious
one. Yet running a model locally still forces a choice between friendly tools that hide the
machine (coarse or absent memory guidance, opaque quantization choices) and expert tools that
expose everything but automate nothing (Python environments, manual quant selection, no
feasibility answer). None of them answers the first question every user actually has: **will
this model run on my machine, at what context length, and how fast?**

Concretely, the field as of 2026-07 ([PRD §2.3](../../PRD.md#23-where-existing-tools-fall-short)):

1. **No pre-download feasibility analysis.** Users discover a model does not fit after a
   40 GB download. Nobody models the macOS GPU-wired memory cap (`iogpu.wired_limit_mb`,
   roughly 2/3 of RAM at ≤36 GiB, 3/4 above), KV growth versus context, or predicts TTFT.
2. **Python packaging friction.** The leading MLX servers require a Python 3.12 arm64
   environment; wrong-arch Python and venv drift are the top install failure class.
3. **KV cache amnesia across agent loops.** Default servers recompute long shared prefixes
   on every tool-call turn; block-level prefix reuse with persistence exists only in niche
   servers.
4. **Opaque memory behavior.** Tools report the model file size, not weights + KV +
   activations + runtime overhead against the actual wired budget; out-of-memory failures
   surface mid-generation instead of at admission.
5. **Fragmented API surfaces.** Agent ecosystems expect both OpenAI
   (`/v1/chat/completions`) and Anthropic (`/v1/messages`) dialects; support is inconsistent.

DRAKKAR exists to close all five gaps in one binary.

## Target users

Ordered by priority ([PRD §3](../../PRD.md#3-target-users)):

1. **The agent builder (primary).** Runs coding agents, multi-agent research loops, or MCP
   tool servers against a local model for privacy, cost, or offline work. Needs concurrent
   requests without inter-token-latency collapse, prefix reuse across tool-call turns, both
   API dialects, and structured output that never breaks JSON.
2. **The AI engineer / power user.** Evaluates open-weight releases weekly. Needs: paste an
   HF link, get a verdict in seconds, correct quantization picked automatically, honest
   benchmark numbers, scriptable `--json` output.
3. **The privacy-constrained professional.** Lawyer, clinician, security researcher on a
   16–36 GiB MacBook. Needs zero-config operation, clear guidance on what the machine can
   and cannot do, graceful behavior at the memory edge, and nothing leaving the device.
4. **The future desktop-app user (v1.0+).** Non-CLI users reached through a menu-bar app
   built on the same engine's C ABI.

## Goals (v1.0 horizon)

These are the product-level goals; testable requirements derive from them in the RFCs.

- **G1.** One-command run: `drakkar run <hf-link-or-alias>` from cold start to interactive
  chat, with a feasibility preflight before any download
  ([RFC-0008](../rfcs/RFC-0008-cli-ux.md), [RFC-0004](../rfcs/RFC-0004-feasibility-engine.md)).
- **G2.** Best-in-class single-stream latency on Apple Silicon: match or beat mlx-lm on
  decode, exploit Metal 4 Neural Accelerators on M5 for prefill, beat all incumbents on warm
  TTFT via prefix caching ([RFC-0003](../rfcs/RFC-0003-inference-core.md),
  [RFC-0009](../rfcs/RFC-0009-performance.md)).
- **G3.** Agent-grade serving: OpenAI + Anthropic compatible endpoints, continuous batching,
  structured output, tool calling, streaming with usage accounting
  ([RFC-0007](../rfcs/RFC-0007-api-server.md)).
- **G4.** A feasibility engine that predicts memory within 7% and throughput within 20% of
  measured, exposed in the CLI, the API, and as machine-readable JSON
  ([RFC-0004](../rfcs/RFC-0004-feasibility-engine.md)).
- **G5.** KV cache subsystem with paged allocation, copy-on-write prefix sharing, KV
  quantization, and an SSD persistence tier that survives restarts
  ([RFC-0005](../rfcs/RFC-0005-kv-cache.md)).
- **G6.** Distribution as a signed, notarized, dependency-free **arm64** binary (Homebrew
  tap plus direct download). No Python, no containers, no drivers. Per LD21 the artifact is
  Apple Silicon only; "universal binary" language from earlier drafts is dropped
  ([RFC-0012](../rfcs/RFC-0012-release-engineering.md)).
- **G7.** Open source under **Apache-2.0** (LD1), developed in the open.

## Non-goals

What DRAKKAR refuses to be, at least through v1.0:

- **N1.** Cross-platform support (Linux/Windows/CUDA). The architecture keeps a backend seam
  ([RFC-0001](../rfcs/RFC-0001-architecture.md)) but Apple Silicon is the only target.
- **N2.** Training or fine-tuning.
- **N3.** Multi-tenant datacenter serving, authn/z beyond a local API key, or fairness
  scheduling across organizations.
- **N4.** A model marketplace or curation service; Hugging Face is the registry.
- **N5.** Image/video generation and full multimodal parity in v1 (vision input is a v1.x
  extension, [RFC-0003](../rfcs/RFC-0003-inference-core.md)).
- **N6.** Intel Macs.

Two adjacent deferrals are locked for clarity: the "aggressive" wired-memory floor profile
is an explicit v1 non-goal (LD11, [RFC-0004](../rfcs/RFC-0004-feasibility-engine.md)), and
the CLI stays line-oriented through v0.x — no TUI before the desktop app (LD15,
[RFC-0008](../rfcs/RFC-0008-cli-ux.md)).

## The honest-speed principle

The product principle governing every subsystem: **honest speed** — maximum performance the
hardware allows, and **no number shown to the user that the engine cannot defend**.

Operationally this means:

- Every predicted figure (memory, max context, TTFT, decode t/s) carries an accuracy
  contract (G4, M2) and tightens after on-device calibration via `drakkar bench --calibrate`
  ([RFC-0009](../rfcs/RFC-0009-performance.md)).
- Numbers not yet measured on the RFC-0009 benchmark fleet are marked `est.` throughout the
  corpus and in product output.
- Every published benchmark number carries a reproducibility manifest — machine, macOS
  version, MLX pin, model hashes (LD18). Plugged-in is the canonical benchmark condition;
  battery is an annotated secondary axis (LD19).
- Admission control rejects a request that would exceed the declared memory budget rather
  than letting Metal fail mid-generation (PRD P11).
- Determinism claims are stated exactly: v1 documents "reproducible given identical batch
  schedule"; a strict-determinism mode is v1.x (LD6,
  [RFC-0003](../rfcs/RFC-0003-inference-core.md)).

Any corpus document or product surface that violates this principle is defective by
definition.

## Success criteria

Measured, not aspirational ([PRD §7](../../PRD.md#7-success-metrics)):

| ID | Criterion |
| -- | --------- |
| M1 | Time-to-first-token-ever: fresh machine, `brew install` to first generated token for an 8B model, under 5 minutes on a 300 Mbps connection (download-dominated) |
| M2 | Fit-report accuracy: predicted peak memory within 7% of measured on the RFC-0009 model matrix; predicted decode within 20%, TTFT within 30% (cold), tightening after `bench --calibrate` |
| M3 | Performance: all Tier-1 targets in [RFC-0009](../rfcs/RFC-0009-performance.md) met on M4 Pro, M4 Max, M5, M5 Max reference machines; never regress more than 3% release-over-release (CI gate) |
| M4 | Agent workload: 4 concurrent coding-agent sessions against one 30B-A3B model on M4 Max sustain per-stream ITL under 45 ms with warm-prefix TTFT under 500 ms |
| M5 | Adoption (12 months post v0.2): 10k GitHub stars, 3 documented integrations (an agent framework, an editor extension, an MCP client), and DRAKKAR cited as a supported backend by at least one major agent tool |
| M6 | Quality: crash-free rate above 99.9% of sessions (from opt-in crash reports only; no telemetry by default, PRD P13) |

## Differentiation summary

The moat is not any single capability; it is the combination of native distribution, the
feasibility engine, and agent-first serving on top of the fastest available kernels, with
numbers the product can defend. Compact view (full competitive table with per-tool evidence:
[PRD §6](../../PRD.md#6-differentiation-summary)):

- **Only DRAKKAR** ships a pre-download feasibility engine (memory + max context + speed
  prediction against the real wired budget) and calibrated performance prediction. No
  incumbent has either.
- **Native single binary** — shared with only two incumbents; the dominant MLX servers all
  require a Python environment.
- **Paged KV + CoW prefix sharing + SSD persistence tier** — incumbents offer at most one of
  the three.
- **OpenAI + Anthropic dialects plus a stable `--json` contract on every CLI command** — the
  agent-integration surface no incumbent covers completely.
- **MLX-class kernels including M5 tensor-op prefill** — table stakes DRAKKAR shares with
  the MLX-based field; the differentiation is everything above this row.

## Roadmap

Milestone codenames are locked (LD25) and used everywhere in the corpus. Scope is the
PRD §8 contract; durations are sequential estimates from project start.

| Phase | Codename | Target | Scope |
| ----- | -------- | ------ | ----- |
| v0.1 | "First light" | +10 weeks | CLI (`run`, `pull`, `ls`, `rm`, `fit`, `doctor`), MLX engine shim, single-request generation, streaming OpenAI chat endpoint, MLX-community models, fit engine v1 (memory + max context) |
| v0.2 | "Convoy" | +10 weeks | Continuous batching, paged KV with prefix CoW, KV quantization, Anthropic `/v1/messages`, tool calling, JSON-schema output, speculative decoding, `bench` + calibration, GGUF coverage backend |
| v0.3 | "Fleet" | +8 weeks | Daemon mode (launchd), multi-model pool with LRU/TTL, SSD KV tier, embeddings endpoint, MCP server mode, `convert`/on-device quantization UX polish, `/v1/responses` behind a config flag (LD5) |
| v1.0 | "Harbor" | +12 weeks | Menu-bar desktop app (SwiftUI shell over the engine's C ABI), auto-update, signed installers, hardware fleet CI, docs site |
| v1.x | — | Exploratory | Vision-language input, distributed inference over Thunderbolt 5, ANE offload for small/draft models, Linux backend seam, strict-determinism mode (LD6) |

Phases are independently shippable; the GGUF backend and the desktop app are cut lines, not
commitments ([PRD §9](../../PRD.md#9-risks-and-mitigations)). Solo/small-team bandwidth is
the top schedule risk and is mitigated by that shippability, not by optimism.

## How to read the corpus

Read this document, then [PRD](../../PRD.md) for the full evidence base, then the sections
below in order. RFCs are the deep design records; spec sections are the normative
cross-cutting contracts. A new contributor should be able to pick any subsystem after
reading 00–01 plus that subsystem's spec section and RFC.

### Spec sections

| Section | Covers |
| ------- | ------ |
| [01 — Architecture](01-architecture.md) | System decomposition, process model, workspace crates (LD24), named invariants |
| [02 — Public API](02-public-api.md) | CLI, HTTP, JSON-schema, and C-ABI surfaces; versioning and stability tiers |
| [03 — Data Model](03-data-model.md) | Core types, on-disk schemas, schema versioning, type invariants |
| [04 — Error Model](04-error-model.md) | Error taxonomy, exit codes, HTTP error shapes, failure-mode enumeration |
| [05 — Observability](05-observability.md) | Logging, metrics catalog, tracing, redaction rules |
| [06 — Security](06-security.md) | Threat model, trust boundaries, secrets handling |
| [07 — Testing Strategy](07-testing-strategy.md) | Test pyramid, conformance suites, CI gates |
| [08 — Performance Budget](08-performance-budget.md) | Latency/throughput/memory budgets, profiling plan |
| [09 — Release and Versioning](09-release-and-versioning.md) | Versioning, signing/notarization, arm64 artifact (LD21), licensing (LD1) |
| [10 — Glossary](10-glossary.md) | Canonical terms, units, ID prefixes, RFC 2119 usage |
| [11 — Decision Log](11-decision-log.md) | Locked decisions (`LDn`) and the open questions they resolved |

### RFC index

| RFC | Title | Requirement IDs |
| --- | ----- | --------------- |
| [RFC-0001](../rfcs/RFC-0001-architecture.md) | Architecture Overview and Design Principles | A* |
| [RFC-0002](../rfcs/RFC-0002-stack-selection.md) | Technology Stack Selection | S*/D*/R* |
| [RFC-0003](../rfcs/RFC-0003-inference-core.md) | Inference Core | IC* |
| [RFC-0004](../rfcs/RFC-0004-feasibility-engine.md) | Feasibility Engine | FE* |
| [RFC-0005](../rfcs/RFC-0005-kv-cache.md) | KV Cache Subsystem | KV* |
| [RFC-0006](../rfcs/RFC-0006-model-pipeline.md) | Model Acquisition and Format Pipeline | MP* |
| [RFC-0007](../rfcs/RFC-0007-api-server.md) | API Server and Scheduler | AS* |
| [RFC-0008](../rfcs/RFC-0008-cli-ux.md) | CLI and UX Specification | CLI* |
| [RFC-0009](../rfcs/RFC-0009-performance.md) | Performance Targets and Benchmark Methodology | PB* |
| [RFC-0010](../rfcs/RFC-0010-backend-abi.md) | Backend C ABI | AB* |
| [RFC-0011](../rfcs/RFC-0011-error-taxonomy.md) | Error Taxonomy | ER* |
| [RFC-0012](../rfcs/RFC-0012-release-engineering.md) | Release Engineering | RE* |

### Conventions (binding on every corpus document)

- RFC 2119 keywords (MUST/SHOULD/MAY) carry their normative meaning.
- Memory figures in GiB unless noted; dates ISO 8601; numbers marked `est.` are modeled
  estimates pending measurement on the RFC-0009 fleet.
- Requirement IDs are per-document and stable; cross-references use relative links with
  anchors, never positional references.
- Genuinely open items appear as `OPEN QUESTION` with an owner (abdelstark) and a resolution
  path (a milestone, an RFC, or a named benchmark/ablation) — never as bare `TBD`.
