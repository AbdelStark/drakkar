# 11 — Decision Log

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14

Decisions locked at corpus acceptance. Each entry names the decision, the document that
records its rationale, and — where it resolved a previously open question — the question it
closed. Corpus documents cite these as `LDn`. A locked decision changes only through the
RFC supersession process ([SPEC.md](../../SPEC.md#change-process)); re-litigating one in an
implementation PR is out of order.

| # | Decision | Recorded in | Resolves |
|---|----------|-------------|----------|
| LD1 | License: Apache-2.0 for the engine repository | [00-overview](00-overview.md), [RFC-0012](../rfcs/RFC-0012-release-engineering.md) | PRD OQ2 |
| LD2 | DRAKKAR remains a working codename; trademark screening is a v1.0 work item, not a blocker | [00-overview](00-overview.md) | PRD OQ1 |
| LD3 | Alias table ships in the binary, user-extensible, refreshed only by explicit `drakkar alias update` | [RFC-0006](../rfcs/RFC-0006-model-pipeline.md) | RFC-0006 OQ1 |
| LD4 | HF cache interop default `clone` (APFS clonefile / hard link, read-only; DRAKKAR never mutates the HF cache) | [RFC-0006](../rfcs/RFC-0006-model-pipeline.md) | PRD OQ5 |
| LD5 | `/v1/responses` lands v0.3 behind a config flag | [RFC-0007](../rfcs/RFC-0007-api-server.md) | PRD OQ3 |
| LD6 | Determinism contract v1: "reproducible given identical batch schedule"; strict-determinism mode is v1.x | [RFC-0003](../rfcs/RFC-0003-inference-core.md) | RFC-0003 OQ2 |
| LD7 | KV block size 32 tokens default; 16-vs-32 decided by the RFC-0009 ablation in v0.2 | [RFC-0005](../rfcs/RFC-0005-kv-cache.md) | kept open with resolution path |
| LD8 | Per-request cache opt-out (`cache: false`) is honored, including in RAM | [RFC-0005](../rfcs/RFC-0005-kv-cache.md), [RFC-0007](../rfcs/RFC-0007-api-server.md) | RFC-0005 OQ2 |
| LD9 | No cross-model KV sharing in v1 (speculation draft/target share nothing) | [RFC-0005](../rfcs/RFC-0005-kv-cache.md) | RFC-0005 OQ3 |
| LD10 | The preflight always fetches `model.safetensors.index.json` when present | [RFC-0004](../rfcs/RFC-0004-feasibility-engine.md) | RFC-0004 OQ1 |
| LD11 | An "aggressive" os_floor profile is deferred; explicit v1 non-goal | [RFC-0004](../rfcs/RFC-0004-feasibility-engine.md) | RFC-0004 OQ2 |
| LD12 | Multi-model pool (v0.3) uses strict per-engine Metal residency isolation; revisited only with v0.3 profiling data | [RFC-0001](../rfcs/RFC-0001-architecture.md) | kept open with resolution path |
| LD13 | Crash isolation via an engine subprocess is revisited at v1.0 for the desktop app | [RFC-0001](../rfcs/RFC-0001-architecture.md) | kept open with resolution path |
| LD14 | `storage.path` (custom or external store volume) is supported from v0.1 | [RFC-0006](../rfcs/RFC-0006-model-pipeline.md) | RFC-0006 OQ2 |
| LD15 | The CLI stays line-oriented through v0.x; no TUI before the desktop app | [RFC-0008](../rfcs/RFC-0008-cli-ux.md) | RFC-0008 OQ1 |
| LD16 | User-defined aliases win over shipped aliases, with a warning | [RFC-0008](../rfcs/RFC-0008-cli-ux.md) | RFC-0008 OQ2 |
| LD17 | KV caching stays fully automatic; Anthropic `cache_control` is honored as hints | [RFC-0007](../rfcs/RFC-0007-api-server.md) | RFC-0007 OQ2 |
| LD18 | Every published benchmark number carries a reproducibility manifest (machine, macOS, MLX pin, model hashes) | [RFC-0009](../rfcs/RFC-0009-performance.md) | RFC-0009 OQ1 |
| LD19 | Plugged-in is the canonical benchmark condition; battery is an annotated secondary axis | [RFC-0009](../rfcs/RFC-0009-performance.md) | RFC-0009 OQ2 |
| LD20 | v0.2 ships gather-based paged attention; the fused paged varlen kernel is a v0.2 performance milestone, with a prototype-both spike deciding build-vs-adopt | [RFC-0003](../rfcs/RFC-0003-inference-core.md) | kept open with resolution path |
| LD21 | Distribution artifact is an arm64-only macOS binary; no universal binary | [RFC-0012](../rfcs/RFC-0012-release-engineering.md), [RFC-0002](../rfcs/RFC-0002-stack-selection.md) | PRD N6 consequence |
| LD22 | Server default bind `127.0.0.1:11711` | [RFC-0007](../rfcs/RFC-0007-api-server.md) | — |
| LD23 | Config at `~/.config/drakkar/config.toml`, state under `~/.drakkar/`; precedence flags > `DRAKKAR_*` env > file > defaults | [RFC-0008](../rfcs/RFC-0008-cli-ux.md) | — |
| LD24 | Workspace crates: drakkar-cli, drakkar-server, drakkar-sched, drakkar-fit, drakkar-models, drakkar-engine, drakkar-grammar, drakkar-core, drakkar-mlx-sys, drakkar-mlx, drakkar-gguf (feature) | [RFC-0002](../rfcs/RFC-0002-stack-selection.md), [01-architecture](01-architecture.md) | — |
| LD25 | Milestones: v0.1 "First light", v0.2 "Convoy", v0.3 "Fleet", v1.0 "Harbor" | [00-overview](00-overview.md) | PRD §8 |

Questions still open after corpus acceptance live in the `Open Questions` section of their
RFC, each with an owner and a resolution path; the implementation tracker mirrors them as
work items.
