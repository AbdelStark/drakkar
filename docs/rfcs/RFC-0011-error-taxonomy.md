# RFC-0011: Error Taxonomy and Failure Semantics

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.1

## Summary

DRAKKAR presents the same underlying failure on three surfaces: human CLI output, machine
`--json` / `--stream-json` output, and two HTTP dialects (OpenAI and Anthropic error
envelopes). This RFC locks the cross-cutting error design those surfaces share: one error
type (`DkError` in `drakkar-core`, LD24), a stable namespaced code registry published as a
versioned contract (`drakkar.errors/1`), a single total mapping from error category to CLI
exit code and HTTP status, explicit retryability, typed remedy templates, a panic policy,
and the FFI status mapping from the backend ABI. The normative code registry lives in
[04 — Error Model](../spec/04-error-model.md); this RFC is the decision record that fixes
its shape and the rules every crate must follow.

It is the concrete realization of RFC-0001 design principle 6 — fail legibly
([Architecture](RFC-0001-architecture.md#proposed-design)) — and the shared foundation
under RFC-0008 CLI8/CLI15 ([CLI and UX](RFC-0008-cli-ux.md#proposed-design)) and RFC-0007
AS8 ([API Server](RFC-0007-api-server.md#proposed-design)).

## Motivation

The PRD makes agents first-class users ([PRD §3](../../PRD.md#3-target-users)) and requires
deterministic exit codes and stable JSON on every command (P7) plus structured, remediable
API errors (RFC-0007 AS8). Three independent presentation surfaces already exist in the
spec corpus:

1. **CLI human output** (RFC-0008 CLI15): what failed, why in domain terms, and the single
   most useful next action.
2. **CLI machine output** (RFC-0008 CLI6-CLI8): schema-versioned JSON with deterministic
   exit codes 0/2-7.
3. **HTTP** (RFC-0007 AS8): `413 context_exceeded` with `max_admissible_tokens`, `429
   kv_pool_exhausted` with `retry_after_ms`, `503 model_loading` with progress — rendered
   in the caller's dialect envelope.

Without one taxonomy, each of the ten workspace crates (LD24) grows its own ad-hoc error
strings, the exit-code and HTTP-status mappings drift apart as failure paths are added, and
an agent driving both the CLI and the server sees the same out-of-memory condition spelled
three different ways. The corpus already cites named errors this RFC must own:
`unsupported_architecture` (RFC-0001 A11, [06 — Security](../spec/06-security.md)), the
gated-repo error with acceptance URL (RFC-0006 MP2), the admission rejection with
`max_tokens_admissible` (RFC-0004 FE18), and the exit-6 panic wrapper (RFC-0008 CLI15).
One decision record prevents ten local dialects.

## Goals

- One error type, defined once in `drakkar-core`, used by every crate above the FFI seam.
- Stable, namespaced error codes — a closed `ErrorCode` enum rendering to stable
  `subsystem.snake_case` strings (self-describing in logs and transcripts) — published as
  an additive-only versioned contract, registered in
  [04 — Error Model](../spec/04-error-model.md).
- A single total mapping site from error category to CLI exit code (RFC-0008 CLI8) and
  HTTP status (RFC-0007 AS8); no second mapping site can exist without failing CI.
- Every error a user can hit carries a domain-term message and, wherever a remedy exists,
  a typed remedy template rendered identically in human and JSON output (RFC-0008 CLI15).
- Explicit retryability semantics so agents never guess whether to retry.
- A panic policy that keeps panics bugs, never a UX: caught at exactly three boundaries,
  always rendered through the same taxonomy.
- Testable: registry totality, mapping exhaustiveness, and remedy coverage are CI gates
  from v0.1.

## Non-Goals

- Enumerating every error code for all milestones. This RFC fixes the type, the category
  set, the mapping rules, and the v0.1 seed registry; the living registry is
  [04 — Error Model](../spec/04-error-model.md), updated in the same PR as any new code.
- Localization. Messages are English in v1; the stable machine surface is the code, not
  the message text.
- Automatic recovery policies (actor auto-restart, request replay). v1 fails honestly;
  see [ER5](#er5-panic-policy) and Drawbacks.
- Client SDK error classes. Third-party clients dispatch on the published codes.
- Crash reporting / telemetry. No error leaves the machine (PRD P13, RFC-0008 CLI16).

## Proposed Design

### ER1: One error type — `DkError` in `drakkar-core`

- ER1. `drakkar-core` defines the workspace-wide error type. Every fallible public
  function in every crate above `drakkar-mlx-sys`/`drakkar-gguf` returns
  `Result<T, DkError>` (directly or via a crate-local enum that converts losslessly at the
  crate boundary, ER4).

```rust
/// drakkar-core::error — the one error type (RFC-0011 ER1). Flat struct, no
/// enum-of-variants sub-error design.
pub struct DkError {
    /// Stable machine code as a closed enum (ER3). Each variant renders to a stable
    /// &'static str via `ErrorCode::as_str()` in "<subsystem>.<snake_case>" form, e.g.
    /// ErrorCode::KvPoolExhausted => "kv.pool_exhausted". The subsystem prefix is drawn
    /// from the closed set (canonical area map): cli, config, models, download, store,
    /// fit, kv, engine, backend, abi, grammar, server, internal. The registry (§04) IS
    /// this enum; an unregistered code is unrepresentable, not a string CI must grep for.
    pub code: ErrorCode,

    /// Failure class. Determines exit code and default HTTP status (ER2).
    pub category: ErrorCategory,

    /// Human message in domain terms (memory, download, format, engine) — never a raw
    /// Rust/Metal/FFI string (those go in `context`/`source`). RFC-0001 principle 6.
    pub message: String,

    /// The single most useful next action, as a typed template (ER7). `None` only for
    /// codes carrying a `remedy_exempt` annotation in the registry.
    pub remedy: Option<Remedy>,

    /// Explicit retry semantics (ER6).
    pub retry: Retry,

    /// Typed key-value context, serialized into JSON and dialect envelopes.
    /// Keys are registered per code in docs/spec/04 (e.g. "max_admissible_tokens": u32
    /// on fit.context_exceeded; "backend_message": String on backend.* per ER8).
    pub context: ErrorContext,

    /// Optional source chain for logs and --verbose; never shown in default human output.
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

/// Closed enum: the registry in [04 — Error Model](../spec/04-error-model.md) is exactly
/// this set. Each variant renders to a stable &'static str via `as_str()`; the wire/JSON
/// code is that string. Adding a variant requires a matching §04 registry row; the CI
/// exhaustive-match and golden tuple snapshot enforce the correspondence (Testing
/// Strategy). No wildcard arms permitted anywhere the enum is matched.
pub enum ErrorCode { /* one variant per §04 row, e.g. KvPoolExhausted, FitContextExceeded */ }

/// Closed set. Adding a variant is a `drakkar.errors` major-version event (ER3) and
/// requires updating the total mapping (ER2) — the compiler enforces this (no wildcard
/// arms permitted, see Testing Strategy).
pub enum ErrorCategory {
    Usage,          // "usage"            bad flags, args, request fields, config values
    ModelNotFound,  // "model_not_found"  unresolvable reference, gated without token
    Infeasible,     // "infeasible"       fit verdict, admission rejection, pool exhaustion
    Network,        // "network"          hub unreachable, download failure
    Format,         // "format"           unsupported architecture, corrupt/invalid artifact
    Engine,         // "engine"           load, Metal, inference, backend FFI failures
    Disk,           // "disk"             space, store I/O
    Internal,       // "internal"         bugs: panics, invariant violations
}
```

- Code namespace and category are orthogonal: the namespace says *which subsystem
  detected* the failure (and matches the issue-label area map in
  [00 — Overview](../spec/00-overview.md)), the category says *what class of failure* it
  is and drives all mappings. Example: `models.unsupported_architecture` is detected by the
  model pipeline but is category `format`.
- The canonical serialization is the `drakkar.error/1` object, part of the machine JSON
  contract ([02 — Public API](../spec/02-public-api.md#4-surface-3-machine-json-contract)):

```jsonc
{
  "schema": "drakkar.error/1",
  "code": "kv.pool_exhausted",
  "category": "infeasible",
  "message": "KV pool is full: 8 active sequences hold 94% of the 6.0 GiB pool",
  "remedy": {
    "rendered": "retry after 1200 ms, or lower max_tokens below 3072",
    "template": "retry_after_or_reduce",
    "params": { "retry_after_ms": 1200, "max_tokens_admissible": 3072 }
  },
  "retry": { "kind": "after", "after_ms": 1200 },
  "context": { "pool_occupancy": 0.94, "active_sequences": 8, "pool_gib": 6.0 },
  "exit_code": 4,
  "request_id": "0197f9a2-..."        // when minted in a request scope (OBS5)
}
```

  This object appears verbatim as the `error` terminal event payload in `--stream-json`
  streams (RFC-0008 CLI7), as the body of `drakkar <cmd> --json` on failure, and as the
  source struct from which both HTTP dialect envelopes render (ER2). `request_id` follows
  [05 — Observability](../spec/05-observability.md) OBS5.

### ER2: One total mapping — category → exit code, code → HTTP status, both dialects

- ER2. Exactly one mapping site exists, in `drakkar-core::error::mapping` (one small
  file). No other crate may map categories or codes to exit codes, HTTP statuses, or
  dialect envelope types; the registry test (Testing Strategy) fails CI if any other
  mapping site appears.

**Category → CLI exit code** (total, exhaustive `match`, no wildcard arm; values fixed by
RFC-0008 CLI8):

| `ErrorCategory` | Exit code | RFC-0008 CLI8 meaning |
| --- | --- | --- |
| `Usage` | 2 | Usage error (bad flags/args) |
| `ModelNotFound` | 3 | Model or reference not found |
| `Infeasible` | 4 | Won't fit / feasibility failure without `--force` |
| `Network` | 5 | Download/network failure |
| `Format` | 6 | Engine/runtime failure — an artifact the engine cannot load is a runtime inability, not a missing model |
| `Engine` | 6 | Engine/runtime failure (load, Metal, inference) |
| `Disk` | 7 | Disk/space failure |
| `Internal` | 6 | Bugs surface as engine/runtime failure with a bug-report hint (CLI15) |

**Category → default HTTP status, with per-code overrides pinned in the registry** (the
override column of a code's registry row is the only place a status may deviate):

| `ErrorCategory` | Default status | Registered overrides (v0.1 seed) |
| --- | --- | --- |
| `Usage` | 400 | `grammar.schema_compile_failed` → 422 (well-formed request, uncompilable schema; RFC-0007 AS10) |
| `ModelNotFound` | 404 | — |
| `Infeasible` | 422 | `fit.context_exceeded` → 413 (admission-time, carries `max_admissible_tokens`); `kv.pool_exhausted` → 429 (carries `retry_after_ms` + occupancy) — per RFC-0007 AS8 |
| `Network` | 503 | — (upstream hub failure during an API-triggered load) |
| `Format` | 422 | — (well-formed request naming an artifact the engine cannot execute; retrying is futile and it is not a server fault) |
| `Engine` | 500 | `server.model_loading` → 503 (transient, carries load progress) — per RFC-0007 AS8 |
| `Disk` | 507 | — |
| `Internal` | 500 | — |

**Dialect envelope rendering** (v0.2 completes this; v0.1 ships the OpenAI column only,
since `/v1/messages` lands in v0.2 — see Migration / Rollout). Both renderers live in the
same mapping module and consume the same `DkError`:

| Condition | OpenAI `error.type` | Anthropic `error.type` |
| --- | --- | --- |
| 400 usage | `invalid_request_error` | `invalid_request_error` |
| 404 model_not_found | `not_found_error` | `not_found_error` |
| 413 context_exceeded | `invalid_request_error` | `request_too_large` |
| 422 infeasible / format | `invalid_request_error` | `invalid_request_error` |
| 429 pool exhausted | `rate_limit_error` | `rate_limit_error` |
| 503 network / model loading | `api_error` | `overloaded_error` |
| 500 engine / internal | `api_error` | `api_error` |
| 507 disk | `api_error` | `api_error` |

  In both dialects the envelope's `message` is `DkError.message`, the upstream `code`
  field (where the dialect has one) carries the DRAKKAR code string, and the full
  `drakkar.error/1` object rides under `x_drakkar` inside the error body per the extension
  namespace rule ([02 — Public API](../spec/02-public-api.md#34-the-x_drakkar-extension-namespace)
  API4). A 429 or 503 response also sets the standard `Retry-After` header from `retry`
  (ER6). Dialect fidelity is covered by the recorded-trace suites of RFC-0007 AC1.

### ER3: Error codes are a public versioned contract — `drakkar.errors/1`

- ER3. The set of registered codes, their categories, statuses, context keys, and remedy
  templates form the contract `drakkar.errors/1`. Rules:
  - **Additive-only** within a major version: new codes may be added in any release; a
    code's category, exit code, HTTP status, and registered context keys never change.
  - **Never reused**: a retired code (possible only at a major version bump) is tombstoned
    in the registry forever; its string may not be reassigned.
  - **Registry is the enum**: codes are the closed `ErrorCode` enum in `drakkar-core`, not
    free strings. An unregistered code is therefore unrepresentable — the compiler cannot
    construct it — so there is nothing to grep for. CI proves the enum and the normative
    registry table in [04 — Error Model](../spec/04-error-model.md) agree via an
    **exhaustive match over `ErrorCode`** (no wildcard arm) plus a committed **golden
    snapshot** of every variant's `(as_str, category, exit, http)` tuple: a new variant
    that is not mapped fails to compile, a changed tuple fails the snapshot diff. The
    registry doc and the enum are edited in the same PR (see Migration / Rollout).
  - The contract sits in the **Stable tier** of the compatibility policy from v1.0
    ([09 — Release and Versioning](../spec/09-release-and-versioning.md)); during v0.x it
    is versioned and additive but pre-stability, like every other schema.

**v0.1 seed registry** (illustrative rows only, string-identical to the normative source;
the complete table with all codes, context keys, and remedy templates is
[04 — Error Model §4](../spec/04-error-model.md#4-the-error-code-registry-drakkarerrors1),
which is authoritative — do not treat these six rows as exhaustive):

| Code | Category | Exit | HTTP | Retry |
| --- | --- | --- | --- | --- |
| `models.gated_repo_no_token` | model_not_found | 3 | 404 | terminal |
| `models.unsupported_architecture` | format | 6 | 422 | terminal |
| `download.hub_unreachable` | network | 5 | 503 | after_backoff |
| `fit.context_exceeded` | infeasible | 4 | 413 | terminal |
| `kv.pool_exhausted` | infeasible | 4 | 429 | after |
| `server.model_loading` | engine | 6 | 503 | after_backoff |

  Observability alignment: the `outcome` label values on `drakkar_requests_total`
  ([05 — Observability](../spec/05-observability.md)) map 1:1 onto registered codes or the
  `ok`/`cancelled` non-error outcomes; the mapping is part of the registry table.

### ER4: Construction discipline — build at origin, log once, no re-wrapping

- ER4. Rules, enforced by review and the log-once test (Testing Strategy):
  1. A `DkError` is constructed **at the failure origin**, the only place with full
     context (the download layer knows the URL and byte offset; the admission path knows
     occupancy and `max_admissible_tokens`). Callers up-stack MUST NOT synthesize context
     they did not observe.
  2. The error is **logged exactly once, at the origin**, at a level set by category:
     `usage`/`model_not_found`/`infeasible` at `info` (expected outcomes, not incidents),
     `network`/`format`/`disk` at `warn`, `engine`/`internal` at `error` with the full
     source chain. Propagation layers MUST NOT log the same error again; the CLI/HTTP
     renderers present, they do not log.
  3. Propagation is **transparent**: crate-local error enums (thiserror-style) may exist
     for internal exhaustiveness, but at each crate boundary they convert into `DkError`
     losslessly via `#[error(transparent)]`-style wrapping — the code, category, remedy,
     retry, and context minted at the origin pass through unchanged. Re-wrapping that
     replaces the code or discards context is forbidden.
  4. The `source` chain is for `--verbose` and logs only. Default human output shows
     `message` + rendered remedy; `--json` shows the full `drakkar.error/1` object minus
     `source` (raw source strings are unstable and MUST NOT become a machine surface).

### ER5: Panic policy

- ER5. **Panics are bugs.** No code path may use a panic to signal an expected failure;
  every expected failure has a registered code. Panics are caught at exactly three
  boundaries and nowhere else:
  1. **Engine actor message loop** (`drakkar-engine`): each message dispatch runs under
     `catch_unwind`. On panic the actor is **poisoned**: every in-flight sequence on that
     model fails with `internal.panic` (each waiting request channel receives the error;
     SSE streams emit the dialect error event and close), the model transitions to a
     `failed` state visible in `drakkar ps`, and the actor thread exits. **No automatic
     restart in v1**: honest failure beats flapping — an auto-restarted actor that panics
     again on the same workload converts one legible bug report into a crash loop.
     Recovery is explicit: unload/reload via `drakkar run`/`serve` or the keep-alive
     reload path (AS17). Restart-with-backoff is reconsidered only with v1.x field data.
  2. **CLI top level** (`drakkar-cli`): a catch-all hook renders exit 6 with a bug-report
     hint (issue URL, `drakkar doctor` snapshot suggestion); the backtrace prints only
     under `--verbose` (RFC-0008 CLI15). No raw Rust panic message ever reaches a user
     unwrapped (RFC-0008 AC5).
  3. **HTTP layer** (`drakkar-server`): the tower-style catch-panic middleware wraps every
     handler and renders `internal.panic` as a 500 in the caller's dialect envelope —
     the same envelope as every other error, so clients need no special case. The
     connection is answered, not dropped.
  - Poisoning is deliberately scoped: a panicked actor takes down its model, never the
    process; the server keeps serving other endpoints (`/health` reports the degraded
    model), and in the v0.3 multi-model pool other actors are unaffected (RFC-0001 A5).
    Cross-process crash isolation for the desktop app remains the LD13 open item on
    RFC-0001, not this RFC.

### ER6: Retryability is an explicit field

- ER6. Agents MUST NOT parse messages to decide on retry. `DkError.retry` is total:

```rust
pub enum Retry {
    /// Default for every category except network. Retrying without changing the
    /// request or the environment will fail identically.
    Terminal,
    /// Transient by nature; retry with exponential backoff + jitter. The download
    /// layer (drakkar-models) implements this internally with bounded attempts and
    /// resume-from-offset (P9); the error surfaces only after retries are exhausted,
    /// still marked AfterBackoff so an outer agent may retry the whole operation.
    AfterBackoff,
    /// Retry after a computed delay. Set by kv.pool_exhausted, where the scheduler
    /// derives after_ms from the expected release horizon of active sequences, and by
    /// server.model_loading from remaining load time.
    After { after_ms: u64 },
}
```

  - Serialization: `{"kind": "terminal"}`, `{"kind": "after_backoff"}`,
    `{"kind": "after", "after_ms": N}` inside `drakkar.error/1`.
  - HTTP: `After { after_ms }` sets `Retry-After` (seconds, rounded up) on 429/503
    responses; `retry_after_ms` also appears in the body per AS8.
  - The registry pins each code's retry class; the totality test verifies every code has
    one.

### ER7: Remedies are typed templates

- ER7. A remedy is not free text. Each registered code either references a remedy template
  or carries a `remedy_exempt` annotation with a justification (e.g. `internal.*`: the
  remedy is filing a bug, covered by the panic hint). Templates live next to the registry
  and take typed parameters filled at construction:

```rust
pub struct Remedy {
    pub template: &'static str,          // registered template id, e.g. "run_sibling"
    pub params: RemedyParams,            // typed per template
}
// Rendering (drakkar-core, one implementation for all surfaces):
//   human:  "Try the 4-bit sibling that fits: drakkar run mlx-community/Qwen3-8B-4bit"
//   json:   { "rendered": "...", "template": "run_sibling",
//             "params": { "sibling_id": "mlx-community/Qwen3-8B-4bit" } }
```

  v0.1 seed templates: `run_sibling` (`fit.wont_fit` — the concrete sibling id comes from
  the resolver's sibling discovery, RFC-0006 MP3
  ([Model Pipeline](RFC-0006-model-pipeline.md#proposed-design)), so the remedy is one
  keypress, not a research project); `retry_after_or_reduce` (`kv.pool_exhausted`);
  `reduce_context` (`fit.context_exceeded`, fills `--ctx {max_admissible_tokens}`);
  `resume_pull` (`download.network_failed`, fills `drakkar pull {ref}`); `prune_store`
  (`download.no_space`, fills reclaimable GiB from `drakkar prune`'s dry-run accounting);
  `accept_license` (`models.gated_repo_no_token`, fills the acceptance URL). Fit-report remedy
  *plans* (the ranked FE19 list) are a superset rendered by `drakkar-fit`; when a fit
  failure becomes an error, its first ranked remedy populates this field — same data, one
  truth (RFC-0001 I3).
  - Human and JSON output render from the same `Remedy` value (RFC-0001 component rule:
    both views from one struct); they cannot drift.

### ER8: FFI status mapping

- ER8. The backend ABI ([RFC-0010](RFC-0010-backend-abi.md#proposed-design)) reports
  failures as `dk_status` values with a thread-local detail string readable via
  `dk_last_error_message`. `drakkar-mlx` (and `drakkar-gguf` for its own FFI layer) maps
  every `dk_status` into the taxonomy through a single total function — an exhaustive match
  on the `dk_status` enum with no wildcard arm, so a new ABI status that is not mapped fails
  compilation, not runtime. The mapping partitions across three subsystem prefixes: `abi.*`
  for ABI-boundary faults (version/struct-size/thread/argument), `backend.*` for compute
  faults (Metal, capability, I/O), and `internal.*` for memory-pressure statuses. The
  exact `dk_status` → code table is [RFC-0010 AB9](RFC-0010-backend-abi.md#proposed-design).
  The shim's message is attached as `context.backend_message`, never used as
  `DkError.message`: the user-facing message stays in domain terms (RFC-0001 principle 6),
  and the raw shim string remains available in logs, `--verbose`, and the JSON context. A
  backend-reported OOM or KV exhaustion means admission control failed to enforce I2
  (RFC-0001) — an internal invariant violation, not an infeasible user error; it maps to
  `internal.*` (`internal.budget_breach` / `internal.invariant`) and is **never** surfaced
  as `kv.pool_exhausted` or any `infeasible`-category code from the backend.

## Alternatives Considered

- **Opaque dynamic errors everywhere (anyhow-style `Box<dyn Error>` with context
  strings).** Fastest to write, and fine for internal tools. Rejected: there is no stable
  machine contract — PRD P7 requires deterministic exit codes and stable JSON, and an
  agent cannot dispatch on prose. Retrofitting codes onto an opaque-error codebase later
  means auditing every construction site; locking the type first costs less.
- **Per-crate error enums with `From` conversions and no central registry.** Idiomatic
  Rust, keeps crates self-contained. Rejected: the mapping to exit codes and HTTP statuses
  ends up re-implemented wherever a crate's error meets a surface, and the mappings drift
  (the exact failure mode this RFC exists to prevent); duplicate or colliding code strings
  across crates are undetectable without a registry. Crate-local enums survive as an
  internal implementation detail (ER4 rule 3) — the rejection is of enums *as the
  contract*.
- **Numeric error codes (integer space, HTTP-style).** Compact, trivially stable.
  Rejected: unreadable in agent logs and transcripts — `kv.pool_exhausted` is
  self-describing and grep-able across the codebase, docs, and issues; `4203` is not. The
  agent-native principle (RFC-0001 principle 4) favors legibility over compactness; the
  bytes are irrelevant at error rates.
- **HTTP-first error design (design the API error bodies, derive the CLI from them).**
  Rejected: the CLI is a first-class surface of equal weight (RFC-0008), ships first
  (v0.1 has no Anthropic dialect yet), and several failure classes (usage errors, store
  GC, doctor findings) never touch HTTP. The taxonomy must be transport-neutral with
  surfaces as renderers, which is exactly the ER1/ER2 split.

## Drawbacks

- **Registry discipline is process overhead.** Every new failure path requires a registry
  row, a remedy template or exemption, and a doc update in the same PR. This is real
  friction on small fixes; the CI check makes it unavoidable rather than optional, which
  is the point.
- **Static code strings tempt reuse.** Reaching for an existing "close enough" code is
  easier than minting one, which erodes precision over time. Mitigated by the CI registry
  check (construction sites must match registered codes, making minting cheap and
  mechanical) and by review convention: a code is reused only when the failure is
  genuinely identical, not merely similar.
- **The single mapping file is a merge hotspot.** Every feature branch adding errors
  touches it. Accepted: the file is small (one table row per code), conflicts are
  line-local and trivially resolvable, and the alternative — distributed mapping — is the
  drift this RFC forbids. The file has a single owner (core area).
- **No auto-restart after actor panic is worse availability on paper.** A transient
  panic takes the model down until an explicit reload. Accepted for v1: a panic is by
  definition an unknown state, and silently restarting over possibly-corrupt accounting
  violates the memory contract's spirit (I2). Revisit with field data in v1.x.

## Migration / Rollout

- **v0.1 "First light":** `DkError`, `ErrorCategory`, `Retry`, `Remedy` land in
  `drakkar-core`; the seed registry (ER3 table) lands in
  [04 — Error Model](../spec/04-error-model.md); the mapping module ships category → exit
  code and code → HTTP status; CLI human/JSON rendering (CLI15 shape) and the two panic
  boundaries the v0.1 binary has (CLI top level, engine actor); minimal HTTP errors on
  the OpenAI dialect (the only dialect in v0.1). The CI registry check
  (exhaustive `ErrorCode` match + golden tuple snapshot) gates from the first release.
- **v0.2 "Convoy":** full dual-dialect envelopes as `/v1/messages` lands (Anthropic
  column of the ER2 table); `Retry-After` header emission and complete retry metadata;
  new codes for the v0.2 surface (grammar/tool-harness failures, bench, GGUF `backend.*`
  compute-fault mappings, KV-quantization and prefix-cache failure paths); HTTP
  catch-panic boundary hardening tests.
- **v0.3 "Fleet":** codes for daemon lifecycle, multi-model pool admission, SSD KV tier
  I/O, embeddings and MCP surfaces — all additive under `drakkar.errors/1`.
- **v1.0 "Harbor":** the taxonomy enters the Stable compatibility tier
  ([09 — Release](../spec/09-release-and-versioning.md)); the embedder-facing C ABI
  documents the `dk_status` ↔ `engine.*` correspondence for third parties (RFC-0010);
  the desktop app consumes the same codes through the ABI.
- **Process rule (all milestones):** any PR minting a new code updates
  `docs/spec/04-error-model.md` and the `ErrorCode` enum in the same PR; CI's exhaustive
  match plus golden tuple snapshot fails the build if the enum and the registry table
  disagree, so the doc cannot lag the code.

## Testing Strategy

- **Registry totality test** (`drakkar-core`, golden): iterates every registered code and
  asserts it has a category, an exit code, an HTTP status, a retry class, and — from
  v0.2 — both dialect envelopes; renders each into golden snapshot files
  (human line, `drakkar.error/1` JSON, OpenAI body, Anthropic body) so any presentation
  change is a reviewed diff.
- **Exhaustive-match tests**: the category → exit-code and category → default-status
  matches carry no wildcard arm (compiler-enforced on `ErrorCategory`); the ER8
  `dk_status` match likewise. A lint gate rejects `_ =>` arms in the mapping module.
- **Single-mapping-site check** (CI): greps the workspace for exit-code and status
  literals outside `drakkar-core::error::mapping` (allow-listed test fixtures only).
- **Registry–enum correspondence** (CI, replaces any code-literal grep): the registry in
  [04 — Error Model §4](../spec/04-error-model.md#4-the-error-code-registry-drakkarerrors1)
  is exactly the closed `ErrorCode` enum, so an unregistered code is unrepresentable. A
  golden test enumerates `ErrorCode` by an exhaustive match (no wildcard arm) and snapshots
  each variant's `(as_str, category, exit, http)` tuple against a committed golden file
  that mirrors the §4 table; a new variant fails to compile until mapped, and any tuple
  drift fails the snapshot diff (ER3).
- **Serde roundtrip property test**: arbitrary `DkError` values (proptest-generated over
  registered codes, arbitrary context/remedy params) serialize to `drakkar.error/1` JSON
  and deserialize back to an equal value (minus `source`, which is documented as
  non-machine surface).
- **CLI integration matrix** (fulfills RFC-0008 AC1/AC5): forces each failure class
  through the real binary and asserts exit code, schema-valid JSON, and a remedy line —
  `models.repo_not_found` via a nonexistent repo; `models.gated_repo_no_token` via a gated
  fixture repo without a token; `download.network_failed` via an unroutable hub URL;
  `download.no_space` via a quota-limited store volume; `download.integrity_mismatch` via a
  corrupted shard fixture; `fit.wont_fit` via `--machine` simulation of an undersized
  profile (FE2); `cli.invalid_args` via bad flags.
- **Server integration** (fulfills RFC-0007 AS8): drives 400/404/413/422/429/503/500
  through the real server and asserts status, dialect envelope, `Retry-After` presence
  on 429/503, and `x_drakkar` payload; recorded-trace dialect suites (RFC-0007 AC1)
  cover envelope fidelity against reference clients.
- **Panic-injection tests**: a test-only actor message and a test-only HTTP route that
  panic on demand; assert in-flight requests receive `internal.panic`, the model shows
  `failed` in `drakkar ps`, the process survives, the HTTP 500 envelope is well-formed,
  and the CLI top-level hook produces exit 6 with the hint (backtrace only under
  `--verbose`).
- **Log-once test**: a log-capture harness forces each seed error and asserts exactly one
  log record per error at the ER4 level for its category.
- **UX lint test**: every registered code has a non-empty remedy template with all
  parameters bound, or an explicit `remedy_exempt` annotation with justification; fails
  otherwise.

## Open Questions

None. All questions raised during drafting were resolved into the design above
(actor restart policy: no auto-restart in v1, ER5; format-category exit-code folding:
exit 6, ER2).

## References

- [PRD](../../PRD.md) — P7 (JSON + deterministic exit codes), P11 (admission over Metal
  failure), P13 (no telemetry), §3 (agent builder as primary user)
- [RFC-0001 — Architecture](RFC-0001-architecture.md) — principle 6 (fail legibly),
  A11 (named error for unsupported architectures), I2/I3, LD13
- [RFC-0004 — Feasibility Engine](RFC-0004-feasibility-engine.md) — FE18 (admission
  rejection), FE19 (verdicts and ranked remedies)
- [RFC-0005 — KV Cache](RFC-0005-kv-cache.md) — KV21 (pool pressure and admission)
- [RFC-0006 — Model Pipeline](RFC-0006-model-pipeline.md) — MP2 (gated-repo error),
  MP3 (sibling discovery feeding `run_sibling` remedies)
- [RFC-0007 — API Server](RFC-0007-api-server.md) — AS2 (fail-loud fields), AS8
  (structured HTTP errors)
- [RFC-0008 — CLI and UX](RFC-0008-cli-ux.md) — CLI8 (exit codes), CLI15 (error shape,
  panic wrapper), AC1/AC5
- [RFC-0010 — Backend ABI](RFC-0010-backend-abi.md) — `dk_status`,
  `dk_last_error_message`
- [04 — Error Model](../spec/04-error-model.md) — the normative code registry
- [02 — Public API](../spec/02-public-api.md) — `drakkar.error/1` in the machine JSON
  contract, `x_drakkar` namespace rule
- [05 — Observability](../spec/05-observability.md) — OBS5 request ids in error bodies,
  `outcome` label alignment
- [09 — Release and Versioning](../spec/09-release-and-versioning.md) — compatibility
  tiers for `drakkar.errors/1`
