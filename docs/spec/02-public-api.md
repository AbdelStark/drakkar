# 02. Public API Surfaces and Compatibility Policy

- Part of: DRAKKAR specification corpus (see [00-overview](00-overview.md))
- Sources: [RFC-0007](../rfcs/RFC-0007-api-server.md), [RFC-0008](../rfcs/RFC-0008-cli-ux.md), [RFC-0004](../rfcs/RFC-0004-feasibility-engine.md), [RFC-0010](../rfcs/RFC-0010-backend-abi.md), [PRD](../../PRD.md)
- Author: abdelstark
- Date: 2026-07-14

This document enumerates every surface DRAKKAR exposes to the outside world, states the
compatibility promise each surface carries, and defines the stability tiers those promises
are expressed in. It is the single place where "is this public?" and "can this change?"
are answered. Requirement IDs minted here use the `API*` prefix; requirements owned by the
source RFCs (`AS*`, `CLI*`, `FE*`, `AB*`) are cited, not restated.

## 1. The closed-surface rule

- API1. DRAKKAR has exactly **four** public surfaces: (1) the CLI (commands, flags, exit
  codes, stdout/stderr contract), (2) the HTTP API (OpenAI and Anthropic dialects plus the
  DRAKKAR-native ops endpoints), (3) the machine JSON contract (versioned `drakkar.<cmd>/N`
  schemas and JSON Lines streaming events), and (4) the embedder C ABI (`dk_*`, RFC-0010).
  Everything else — Rust crate APIs, the `InferenceBackend` trait, the engine actor message
  set, on-disk store layout under `~/.drakkar/` (RFC-0001 A8: reconstructible, deletable),
  log line formats, and human-readable CLI rendering — is **internal** and carries no
  compatibility promise. A consumer that scrapes human CLI output instead of `--json` has
  no contract; PRD P7 and RFC-0001 I4 make the JSON representation the only stable one.
- API2. No feature ships on a public surface without appearing in this document's tables
  (or a superseding revision of them). An endpoint, flag, schema field, or ABI symbol that
  is reachable but undocumented is a release-blocking defect, not an accidental extension.

## 2. Surface 1: CLI

Normative source: RFC-0008 ([CLI and UX](../rfcs/RFC-0008-cli-ux.md#proposed-design)).
The CLI has two co-equal users — a human at a TTY and an agent driving a subprocess
(RFC-0008 §1) — and every command renders both views from the same internal structs.

### 2.1 Command surface by milestone

| Command | Purpose | Ships | Tier at ship |
| --- | --- | --- | --- |
| `drakkar run <ref> [prompt]` | Fit-check, acquire, load, REPL or one-shot | v0.1 | stable |
| `drakkar pull <ref>` | Acquire + prepare without running | v0.1 | stable |
| `drakkar fit <ref>` | Feasibility report, no download (FE25) | v0.1 | stable |
| `drakkar ls` | Installed models | v0.1 | stable |
| `drakkar rm <ref>` / `drakkar prune` | Remove model / GC unreferenced blobs | v0.1 | stable |
| `drakkar doctor` | Environment report | v0.1 | stable |
| `drakkar serve [<ref>]` | HTTP server, foreground | v0.1 | stable |
| `drakkar config get\|set\|path` | Config read/write (CLI10-CLI11) | v0.1 | stable |
| `drakkar completions <shell>` | Shell completions | v0.1 | stable |
| `drakkar ps` | Resident models, pool occupancy | v0.2 | stable |
| `drakkar bench <ref> [--calibrate]` | Benchmark + calibration (RFC-0009) | v0.2 | experimental → stable v0.3 |
| `drakkar convert <ref> --bits B` | On-device quantization (RFC-0006 §6) | v0.2 | experimental → stable v0.3 |
| `drakkar cache ls\|clear` | SSD KV tier management (RFC-0005 §7) | v0.3 | experimental → stable v1.0 |
| `drakkar serve --daemon\|--stop\|--status\|--logs` | launchd lifecycle (CLI12) | v0.3 | stable |
| `drakkar alias update` | Refresh the shipped alias table (LD3, RFC-0006) | v0.2 | stable |

### 2.2 Global flags and environment

Per RFC-0008 CLI6-CLI9 and CLI2: `--json`, `--stream-json` (streaming commands),
`--quiet`, `--verbose`/`-v` (repeatable), `--yes` (suppress confirmations), `--force`
(override a `Won't fit` verdict, exit 4 path), `NO_COLOR` and non-TTY detection disable
ANSI. Configuration precedence is fixed by LD23: flags > `DRAKKAR_*` env > `~/.config/drakkar/config.toml` > defaults. stdout carries machine output only when `--json`
is given; logs and progress always go to stderr (CLI6), so `drakkar fit <ref> --json | jq .verdict` works with nothing else on stdout (RFC-0008 AC2).

### 2.3 Exit codes

The exit-code table is owned by RFC-0008 CLI8 and reproduced here because it is a frozen
part of the public surface:

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 2 | Usage error (bad flags/args) |
| 3 | Model or reference not found |
| 4 | Won't fit (feasibility failure without `--force`) |
| 5 | Download/network failure |
| 6 | Engine/runtime failure (load, Metal, inference); also the top-level panic wrapper (CLI15) |
| 7 | Disk/space failure |

- API3. Exit codes are **append-only and never renumbered**, from v0.1 onward. New failure
  classes get new codes (8+); an existing code never changes meaning. Code 1 is
  deliberately unassigned (it is what an uncaught runtime abort would produce); DRAKKAR
  never emits it intentionally, so observing exit 1 indicates a defect in the panic
  wrapper, reportable as a bug.

## 3. Surface 2: HTTP API

Normative source: RFC-0007 ([API Server](../rfcs/RFC-0007-api-server.md#proposed-design)).
Default bind is `127.0.0.1:11711` (LD22, AS18); non-loopback binding requires `--api-key`.

### 3.1 Endpoint surface by milestone

| Endpoint | Dialect | Ships | Tier at ship |
| --- | --- | --- | --- |
| `POST /v1/chat/completions` | OpenAI | v0.1 | stable (streaming + non-streaming; tools and `response_format: json_schema` from v0.2) |
| `GET /v1/models` | OpenAI | v0.1 | stable (with `x_drakkar` per-model fields, AS20) |
| `POST /fit` | DRAKKAR | v0.1 | stable (FE26 schema; AS20) |
| `GET /health` | ops | v0.1 | stable |
| `GET /metrics` | ops (Prometheus) | v0.1 | experimental → stable v1.0 (metric names may still move; AS21) |
| `POST /v1/completions` | OpenAI legacy | v0.2 | stable |
| `POST /v1/messages` | Anthropic | v0.2 | stable |
| `POST /v1/embeddings` | OpenAI | v0.3 | experimental → stable v1.0 |
| `POST /v1/responses` | OpenAI Responses | v0.3, behind a config flag (LD5) | experimental |

Ollama-compatible `/api/*` shims are explicitly **not** part of the surface in v1
(RFC-0007 AS3; revisit v0.3). Multi-model routing via the `model` field on one port lands
in v0.3 with the engine pool; the v0.1-v0.2 API shape already carries the `model` field so
no breaking change is required (RFC-0007 AS3).

### 3.2 Streaming envelopes

Per RFC-0007 AS1 and AS4-AS6: responses render in the caller's dialect, including its
streaming envelope — OpenAI SSE `chat.completion.chunk` frames with a final `usage` frame;
Anthropic `message_start` / `content_block_delta` / `message_delta` / `message_stop`
events. SSE heartbeats fire after 10 s of silence; client disconnect cancels within one
decode step. Usage accounting appears on every completion and final stream frame: prompt
tokens, cached prompt tokens (`prompt_tokens_details.cached_tokens` / Anthropic
`cache_read_input_tokens`), completion tokens, plus DRAKKAR extensions (§3.4).

### 3.3 Error envelopes

Errors are structured, remediable, and rendered in the caller's dialect envelope
(RFC-0007 AS8): `413 context_exceeded` carries `max_admissible_tokens`; `429
kv_pool_exhausted` carries `retry_after_ms` and current occupancy; `503 model_loading`
carries progress. The full taxonomy and its stable machine codes live in
[04-error-model](04-error-model.md) and RFC-0011.

### 3.4 The `x_drakkar` extension namespace

- API4. Every DRAKKAR-specific field on an upstream-dialect response lives under an
  `x_drakkar` object (or an `x_drakkar`-prefixed key where the envelope position forces a
  flat key). DRAKKAR never adds bare fields to OpenAI or Anthropic envelope positions that
  upstream could later claim. Current occupants: `x_drakkar.ttft_ms`,
  `x_drakkar.itl_ms_p50`, `x_drakkar.kv_cache_hit_ratio` on usage frames (AS6), and the
  per-model `x_drakkar` block on `GET /v1/models` (format, quant, `ctx_max` at current KV
  precision, resident state, pool stats summary; AS20). Fields inside `x_drakkar` follow
  the same additive-only rule as the JSON schemas (API7).

### 3.5 Per-request cache control

`cache: false` (or the equivalent header) opts a request out of donate-to-cache even in
RAM (LD8, RFC-0005/RFC-0007). Anthropic `cache_control` blocks are honored as hints on top
of fully automatic caching (LD17); they never degrade a request that omits them.

## 4. Surface 3: Machine JSON contract

Normative sources: RFC-0008 CLI6-CLI7, RFC-0004 FE26.

### 4.1 Schema registry

Every `--json` output is a single JSON object on stdout whose first-class `schema` field
names its contract as `drakkar.<cmd>/<major>` (RFC-0008 CLI6). The v0.x registry:

| Schema | Emitted by | Ships |
| --- | --- | --- |
| `drakkar.fit/1` | `drakkar fit --json`, `POST /fit` (FE26) | v0.1 |
| `drakkar.run/1` | `drakkar run --json` (one-shot result) | v0.1 |
| `drakkar.pull/1` | `drakkar pull --json` | v0.1 |
| `drakkar.ls/1` | `drakkar ls --json` | v0.1 |
| `drakkar.rm/1` | `drakkar rm --json`, `drakkar prune --json` | v0.1 |
| `drakkar.doctor/1` | `drakkar doctor --json` | v0.1 |
| `drakkar.config/1` | `drakkar config get --json` | v0.1 |
| `drakkar.error/1` | error object on any `--json` failure / `--stream-json` `error` event (owner RFC-0011; [04](04-error-model.md)) | v0.1 |
| `drakkar.ps/1` | `drakkar ps --json` | v0.2 |
| `drakkar.bench/1` | `drakkar bench --json` (RFC-0009 report + reproducibility manifest, LD18) | v0.2 |
| `drakkar.convert/1` | `drakkar convert --json` | v0.2 |
| `drakkar.cache/1` | `drakkar cache ls --json` | v0.3 |

- API5. The `schema` field is mandatory on every machine JSON object, including error
  objects and every JSON Lines event stream header. A consumer MUST be able to dispatch on
  `schema` alone.

### 4.2 The fit schema

`drakkar.fit/1` is defined by RFC-0004 FE26
([Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)) and served
identically by the CLI (FE25) and `POST /fit` (AS20, FE27): model descriptor with
quantization (`scheme`, `bits`, `group`, `bpw_eff`), machine profile with `budget_source`
(`probe` vs profile table), memory breakdown in GiB with a `confidence` tier, verdict
(`comfortable` / `tight` / `needs_tuning` / `wont_fit`), context ceilings per KV precision,
performance predictions each carrying `confidence` (`measured` / `calibrated` / `modeled`),
and a ranked `remedies` array. The human report card and the JSON serialize from the same
struct; they cannot drift.

### 4.3 JSON Lines streaming events

Streaming commands (`run`, `bench`) support `--stream-json` (RFC-0008 CLI7): one JSON
object per line on stdout, discriminated by `event`. The v1 event set:

```jsonc
{"schema": "drakkar.run-stream/1", "event": "start", "model": "qwen3:8b", "ctx_max": 32768}
{"event": "token", "text": " fjord", "token_id": 48231}
{"event": "stats", "ttft_ms": 412, "itl_ms_p50": 21.3, "prompt_tokens": 2048, "completion_tokens": 96}
{"event": "done",  "finish_reason": "stop", "usage": {"prompt_tokens": 2048, "cached_prompt_tokens": 1792, "completion_tokens": 128}}
{"event": "error", "code": "engine.inference_failed", "message": "generation failed mid-flight; the sequence was aborted and its blocks freed", "remedy": "retry the request; recurrence on the same input is a bug — report it", "exit_code": 6}
```

- API6. The first line of every `--stream-json` stream is a `start` event carrying the
  `schema` field; subsequent lines inherit that schema. The `event` discriminator set
  (`start`, `token`, `stats`, `done`, `error`) is append-only within a schema major; a
  consumer MUST ignore event types it does not recognize (this is the forward-compatibility
  contract that lets new event types ship additively). Exactly one terminal event (`done`
  or `error`) ends every stream; `error` carries the machine error code from
  [04-error-model](04-error-model.md) and the process exit code the CLI will return.

## 5. Surface 4: Embedder C ABI

Normative source: RFC-0010 ([Backend ABI](../rfcs/RFC-0010-backend-abi.md)), which
specifies the `dk_*` symbol family and the `DK_ABI_VERSION` handshake.

The same ABI serves two roles on one definition:

1. **Internal seam (v0.1+).** The Rust core (`drakkar-mlx-sys`, LD24) binds the C++ MLX
   shim through `dk_*` (RFC-0002 D2: array lifecycle, model-graph construction, quantized
   matmul, fused SDPA, KV block ops, fused sampling — on the order of 40 functions). In
   v0.1-v0.3 this seam is compiled into one binary; it is versioned from day one but not
   yet a public promise.
2. **Embedder surface (v1.0).** The v1.0 desktop app (SwiftUI shell, PRD §8) and
   third-party embedders consume the engine through the same C ABI (RFC-0002 §6). At v1.0
   the embedder-facing subset graduates to a stable public surface with a published header
   and semantic documentation in RFC-0010.

- API7. **Handshake before anything else.** `DK_ABI_VERSION` is a monotonically increasing
  integer. The first call any consumer makes is the version query defined in RFC-0010; in
  v0.x-v1.0 the policy is exact match — a consumer compiled against version N MUST refuse
  to proceed against any other version, and the engine loader MUST refuse a shim whose
  compiled version differs from its own, failing at load time with a named error (never a
  symbol-resolution crash or silent misbehavior mid-inference). Compatibility ranges
  (minimum-version semantics) are a post-v1.0 extension and require an RFC-0010 revision.
- API8. Nothing above the ABI names Metal, MLX, or llama.cpp types (RFC-0001 I5); nothing
  crosses the ABI except C types declared in the RFC-0010 header. Any symbol outside the
  `dk_` prefix in the shim's export table is a defect.

## 6. Versioning policy per surface

| Surface | Version carrier | Change discipline |
| --- | --- | --- |
| CLI | command/flag names, exit codes | deprecate-warn-remove cycle (API9); exit codes append-only (API3) |
| HTTP | dialect fidelity vs recorded traces | trace suites are the contract (API10); extensions confined to `x_drakkar` (API4) |
| Machine JSON | `drakkar.<cmd>/<major>` string | additive-only within a major (API11) |
| C ABI | `DK_ABI_VERSION` integer | exact-match load-time handshake (API7) |

- API9. **CLI deprecation = warn one milestone, then remove.** A flag or command deprecated
  in milestone M emits a one-line stderr warning (never on stdout, never altering `--json`
  output) for the whole of M, and is removed in M+1. Command names are never reused with
  different semantics, even after removal. Renames ship as an alias for the warning
  milestone. Within a milestone, flags and commands only accrete.
- API10. **HTTP compatibility is defined operationally**: the recorded-trace suites for
  reference clients (openai-python, the Anthropic SDK, and the two reference agent CLIs of
  RFC-0007 AS1/AC1) MUST pass unmodified against localhost on every release. A change that
  breaks a recorded trace is a breaking change regardless of intent, and is forbidden
  within a DRAKKAR major version. Upstream dialect evolution is absorbed by refreshing the
  trace corpus once per milestone and adopting new upstream fields additively; fields
  DRAKKAR cannot honor fall under the honesty rule (§8), never under silent acceptance.
- API11. **JSON schemas are additive-only within a major.** Within `drakkar.<cmd>/N`,
  permitted changes: new optional fields, new enum values on fields documented as open
  enums (`event` per API6; `confidence`; remedy kinds), and widened numeric ranges.
  Forbidden without a major bump: removing or renaming a field, changing a field's type or
  units, changing the meaning of an existing enum value, or making an optional field
  required for consumers. A major bump (`/N+1`) is announced one milestone ahead via a
  stderr deprecation warning whenever the old schema is emitted, mirroring API9; during
  v0.x, majors may bump at milestone boundaries only, and from v1.0 only at a DRAKKAR
  major version. Units never change silently: memory is GiB, time is ms or s as suffixed
  in the field name (`_gib`, `_ms`, `_s`, per FE26).

## 7. Stability tiers

| Tier | Promise |
| --- | --- |
| **stable** | Governed by §6 in full. Breaking changes only at a DRAKKAR major version (post-1.0) or with the one-milestone deprecation cycle (0.x), always in release notes. Covered by conformance tests in CI (RFC-0008 AC1, RFC-0007 AC1). |
| **experimental** | Shipped and documented, may change or disappear at any milestone without a deprecation cycle. Marked in docs; where a flag gates it (e.g. `/v1/responses`, LD5), the flag name states it. Honesty rule (§8) still applies in full. |
| **internal** | No promise. Not enumerated in this document's tables. Consumers have no recourse. |

Per-surface promises over the roadmap (LD25 milestones):

| Surface | v0.1 "First light" | v0.2 "Convoy" | v0.3 "Fleet" | v1.0 "Harbor" |
| --- | --- | --- | --- | --- |
| CLI commands + exit codes | v0.1 command set and the exit-code table stable; codes frozen (API3) | `ps`, `alias update` stable; `bench`, `convert` experimental | daemon lifecycle stable; `cache` experimental | entire shipped surface stable |
| HTTP | OpenAI chat, `/v1/models`, `/fit`, `/health` stable; `/metrics` experimental | `/v1/completions`, `/v1/messages` stable; tools + structured output stable | `/v1/embeddings` experimental; `/v1/responses` experimental behind flag | everything unflagged stable, incl. `/metrics` names |
| Machine JSON | `drakkar.fit/1` and all v0.1 schemas stable at major 1 | v0.2 schemas stable; stream-JSON event set frozen per API6 | `drakkar.cache/1` experimental one milestone | all schemas stable; major bumps only at v2.0 |
| C ABI | internal (versioned seam, exact-match handshake active) | internal | internal; header review in RFC-0010 for v1.0 publication | embedder subset **stable public**, exact-match `DK_ABI_VERSION` |

## 8. The honesty rule

RFC-0007 AS2 is the load-bearing compatibility principle and it generalizes to every
surface:

> Unsupported dialect fields fail loud (`400` naming the field) unless explicitly
> ignorable; silent acceptance of ignored parameters is forbidden.

- API12. The set of "explicitly ignorable" fields is a **closed, per-endpoint allowlist**
  maintained in the RFC-0007 conformance fixtures; adding a field to it requires a fixture
  demonstrating that ignoring it cannot change the semantics the caller asked for (e.g. a
  pure client-side bookkeeping field). Everything else that DRAKKAR does not implement is
  rejected with an error naming the field and, where one exists, the milestone or flag
  that will support it.
- API13. The same rule binds the other surfaces: unknown CLI flags are usage errors (exit
  2), never ignored; unknown config keys are flagged by `drakkar doctor` and rejected by
  `drakkar config set` (RFC-0008 CLI11); unknown fields in a request to `POST /fit` are
  rejected naming the field; the C ABI rejects version mismatches at load (API7) rather
  than best-effort continuing. The product-level rationale is PRD §1 "honest speed": the
  engine never lets a caller believe a parameter took effect when it did not — a sampler
  setting silently dropped is a wrong benchmark result and a broken agent, not a
  convenience.

## 9. Requirement index

| ID | One-line statement |
| --- | --- |
| API1 | Exactly four public surfaces; all else internal |
| API2 | Reachable-but-undocumented surface area is a release blocker |
| API3 | Exit codes append-only, never renumbered; code 1 unassigned |
| API4 | All HTTP extensions live under `x_drakkar` |
| API5 | Every machine JSON object carries a dispatchable `schema` field |
| API6 | Stream-JSON: `start` first, one terminal event, unknown events ignorable, event set append-only |
| API7 | `DK_ABI_VERSION` integer, exact-match handshake before any other call, fail loud at load |
| API8 | Only `dk_`-prefixed C symbols cross the ABI; no backend types leak above it |
| API9 | CLI deprecation: warn one full milestone on stderr, remove the next; names never reused |
| API10 | HTTP compatibility = recorded-trace suites pass unmodified; traces refreshed once per milestone |
| API11 | JSON schemas additive-only within a major; unit suffixes never change silently |
| API12 | Ignorable-field allowlist is closed and fixture-backed |
| API13 | Honesty rule applies to CLI flags, config keys, `/fit` bodies, and the ABI handshake alike |
