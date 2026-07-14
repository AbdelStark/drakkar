# 04 — Error Model

- Corpus: DRAKKAR specification, section 04
- Normative registry version: `drakkar.errors/1`
- Decision record: [RFC-0011: Error Taxonomy](../rfcs/RFC-0011-error-taxonomy.md) (ER requirement series)
- Related: [RFC-0008 CLI8/CLI15](../rfcs/RFC-0008-cli-ux.md#proposed-design), [RFC-0007 AS2/AS8](../rfcs/RFC-0007-api-server.md#proposed-design), [RFC-0004 FE18/FE19](../rfcs/RFC-0004-feasibility-engine.md#proposed-design), [RFC-0010 AB9](../rfcs/RFC-0010-backend-abi.md#proposed-design), [RFC-0001 design principle 6](../rfcs/RFC-0001-architecture.md#proposed-design)

This section is the **normative error registry** for DRAKKAR. RFC-0011 records why the
taxonomy is shaped this way; this document records what the taxonomy **is**: the closed
subsystem-prefix set, the category table with its total CLI exit-code and HTTP status
mapping, the stable error-code registry (which is a closed Rust enum), the `DkError` type
and its `Remedy`/`Retry` companions, the JSON envelopes for `--json` and both HTTP
dialects, and the propagation rules. Every error a user can hit maps to exactly one code
in this registry ([PRD](../../PRD.md) "fail legibly", RFC-0001 design principle 6). The
taxonomy lives in one crate, `drakkar-core` (workspace layout:
[01 — Architecture](01-architecture.md)); no other crate defines user-visible error types.

## 1. Invariants

Every rule below is normative (RFC 2119 MUST) and enforced by the tests named in
[RFC-0011 Testing Strategy](../rfcs/RFC-0011-error-taxonomy.md#testing-strategy).

- **INV-SINGLE-TAXONOMY.** All user-visible errors are `DkError` values whose `code` is a
  variant of the closed `ErrorCode` enum (§5.1). Subsystem-internal error types exist, but
  they MUST convert into `DkError` before crossing into the CLI renderer or an HTTP
  handler. Because `ErrorCode` is a closed enum — not a `&str` — an unregistered code is
  unrepresentable: it is a value the compiler cannot construct, so there is no string
  literal for CI to grep. The registry (§4) is exactly this enum; CI proves agreement with
  an **exhaustive match over `ErrorCode`** (no wildcard arm) plus a committed **golden
  snapshot** of every variant's `(as_str, category, exit, http)` tuple. A new variant that
  is not mapped fails to compile; a changed tuple fails the snapshot diff (Testing
  Strategy).
- **INV-LOG-ONCE.** An error is logged exactly once, at its origin, with full context
  (code, cause chain, request id). Layers above attach context to the value as it
  propagates; they MUST NOT re-log it. One failure produces one log record, not a stack
  of near-duplicates.
- **INV-NO-RAW-PANIC.** No panic reaches a user unwrapped. `catch_unwind` boundaries sit
  at the engine actor's message loop and at the CLI top level; the HTTP server wraps
  handlers in an equivalent panic-catching layer. A caught panic renders as
  `internal.panic` (exit 6 / HTTP 500) with a bug-report hint; the backtrace appears only
  under `--verbose` (RFC-0008 CLI15) or in the log file, never on default stderr. See §7.
- **INV-REMEDY-ALWAYS.** Every registry entry except the `internal` category carries a
  remedy template: the single most useful next action, as a command or flag, not prose
  advice (RFC-0008 CLI15). `internal.*` remedies are always the bug-report instruction.
- **INV-NO-SECRETS.** Error messages, remedy text, and `context` fields MUST NOT contain
  HF tokens (RFC-0001 A12), API keys, or prompt/completion bodies (RFC-0007 AS19). File
  paths and model ids are allowed; user content is not.
- **INV-FAIL-LOUD.** Unknown or unsupported request fields are errors, never silently
  ignored (RFC-0007 AS2). The honesty rule applies to failure as much as to speed.
- **INV-ADDITIVE-REGISTRY.** `drakkar.errors/1` is additive-only. Codes are never
  removed, renamed, or re-categorized; the category, exit code, and HTTP status of an
  existing code never change within registry major version 1. New codes MAY be added in
  any release. Consumers that see an unknown code MUST fall back to the `category` field,
  which is closed (§2) and sufficient to choose a handling strategy.

## 2. Error categories and the total mapping

The category set is closed: exactly eight categories, each mapped to one deterministic CLI
exit code (RFC-0008 CLI8, [CLI and UX](../rfcs/RFC-0008-cli-ux.md#proposed-design)) and one
default HTTP status. This mapping is total and is defined **once, here**; every other
document references it and no crate re-derives it (RFC-0011 ER2). Exit code 1 is
deliberately never emitted: it is the ambient failure code of shells and wrappers, and
reserving it means an exit 1 from a `drakkar` invocation always indicates the process did
not run to a DRAKKAR-controlled conclusion (killed, exec failure, harness bug) rather than
a taxonomy result.

| Category | Meaning | CLI exit | HTTP default | Retryable |
| --- | --- | --- | --- | --- |
| `usage` | Malformed or unsupported input: bad flags/args, unknown API fields, invalid config, uncompilable schema | 2 | 400 | no |
| `model_not_found` | The model reference does not resolve to anything servable: unknown/uninstalled/gated repo, no compatible artifact | 3 | 404 | no |
| `infeasible` | The feasibility engine or admission control rejects the plan or request (RFC-0004 FE18/FE19) | 4 | 422 | per-code |
| `network` | Hub or download-path failure: unreachable, interrupted | 5 | 503 | yes (backoff) |
| `format` | The artifact or its metadata is unusable: pickle rejected, unsupported architecture, corrupt/mismatched blob | 6 | 422 | no |
| `engine` | Backend/runtime failure: Metal init, model load, inference fault, transient loading state | 6 | 500 | no |
| `disk` | Local storage failure: insufficient space (preflighted, MP9), store write errors | 7 | 507 | no |
| `internal` | Invariant violations, ABI-boundary faults, and caught panics; always a DRAKKAR bug | 6 | 500 | no |

**Per-code HTTP overrides.** The HTTP status is the category default except for the four
codes whose registry row (§4) pins a different value. These are the only deviations
permitted, and each is registered:

- `fit.context_exceeded` → **413** (infeasible default 422; carries `max_admissible_tokens`).
- `kv.pool_exhausted` → **429** (infeasible default 422; carries `retry_after_ms`, retryable).
- `grammar.schema_compile_failed` → **422** (usage default 400; well-formed request, uncompilable schema).
- `server.model_loading` → **503** (engine default 500; transient, carries load progress, retryable).

Notes on the shared exit codes: `format`, `engine`, and `internal` all exit 6. CLI8
defines 6 as "engine/runtime failure (load, Metal, inference)"; format failures are
load-class failures of the artifact and share the user action space (get a different
artifact or a different DRAKKAR build), and `internal` reuses 6 per CLI15 ("panics are
caught at the top level and rendered as exit-6"). The mapping is total and deterministic,
not injective. Scripts that need finer resolution than the exit code MUST use `--json` and
read `code` — the exit code is a coarse classifier, the code string is the contract.

## 3. HTTP status mapping and dialect error types

HTTP statuses derive from the category (§2) with the four per-code overrides listed above.
The structured fields required by RFC-0007 AS8 are normative:

| Status | Canonical codes | Required structured fields |
| --- | --- | --- |
| 400 | `server.unsupported_field`, `config.invalid_key`, `config.invalid_value`; malformed bodies | `param` (offending field) where applicable |
| 404 | `models.not_found`, `models.not_installed`, `models.repo_not_found`, `models.gated_repo_no_token` | — |
| 413 | `fit.context_exceeded` | `max_admissible_tokens` (AS8) |
| 422 | `fit.wont_fit`, `grammar.schema_compile_failed`, `models.unsupported_architecture`, `models.pickle_rejected`, `download.integrity_mismatch`, `store.corrupt_blob` | `reason` |
| 429 | `kv.pool_exhausted` | `retry_after_ms`, `pool_occupancy` (AS8); `Retry-After` header also set |
| 500 | `engine.*` faults, `backend.*`, `abi.*`, `internal.*` | — |
| 503 | `server.model_loading`, `download.hub_unreachable`, `download.network_failed` (server-side load) | `progress` (0.0-1.0) for `model_loading` (AS8) |
| 507 | `store.write_failed`, `download.no_space` | — |

Both dialects render the same `DkError`; the wire envelope differs (§5). The
dialect-native `type` field is derived from the status:

| Status | OpenAI dialect `error.type` | Anthropic dialect `error.type` |
| --- | --- | --- |
| 400 | `invalid_request_error` | `invalid_request_error` |
| 404 | `invalid_request_error` | `not_found_error` |
| 413 | `invalid_request_error` | `request_too_large` |
| 422 | `invalid_request_error` | `invalid_request_error` |
| 429 | `rate_limit_error` | `rate_limit_error` |
| 500 | `server_error` | `api_error` |
| 503 | `server_error` | `overloaded_error` |
| 507 | `server_error` | `api_error` |

The DRAKKAR code string always travels in the envelope (`error.code` in the OpenAI
dialect, `error.x_drakkar.code` in the Anthropic dialect) so clients never have to parse
messages. Dialect fidelity, including error envelopes, is covered by the recorded-trace
suites of RFC-0007 AC1.

## 4. The error-code registry (`drakkar.errors/1`)

Code strings are `subsystem.snake_case_condition`. The subsystem prefix set is **closed**
and is exactly these thirteen (no others): `cli`, `config`, `models`, `download`, `store`,
`fit`, `kv`, `engine`, `backend`, `abi`, `grammar`, `server`, `internal`. The names are
plural where they denote a subsystem area (`models`, `download`, `store`); `server`
carries API-layer request errors; `backend` and `abi` split the FFI boundary (compute
faults vs ABI-contract faults, RFC-0010 AB9). There is no `api`, `net`, `sched`, or
`convert` prefix. Surfaces: `cli` (only reachable from the CLI), `http` (only from the
server), `both`. The `Retry` class of each code is fixed here (§5.1 defines the type);
`terminal` means the identical request will fail identically until the remedy is acted on,
`after_backoff` means transient (client-chosen backoff), `after` means a computed delay is
carried in `context.retry_after_ms`. Remedy templates use `{placeholder}` fields filled
from `context`; the rendered remedy is a single line and, wherever possible, a runnable
command (INV-REMEDY-ALWAYS).

This table is the normative source and corresponds one-to-one with the `ErrorCode` enum in
`drakkar-core` (§5.1). CI asserts agreement by an exhaustive match over `ErrorCode` and a
committed golden snapshot of each variant's `(as_str, category, exit, http)` tuple
(RFC-0011 Testing Strategy); any divergence fails the build.

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `cli.invalid_args` | usage | cli | 400 | terminal | `Run 'drakkar {command} --help' for accepted flags and arguments.` |
| `cli.missing_model_arg` | usage | cli | 400 | terminal | `This command needs a model reference: 'drakkar {command} <ref>'. 'drakkar ls' lists installed models.` |
| `config.invalid_key` | usage | both | 400 | terminal | `Unknown config key '{key}'. 'drakkar config path' shows the file; 'drakkar doctor' lists valid keys.` |
| `config.invalid_value` | usage | both | 400 | terminal | `'{key}' expects {expected}; got '{value}'. 'drakkar config set {key} <value>' validates before writing.` |
| `models.not_found` | model_not_found | both | 404 | terminal | `'{ref}' does not resolve to a servable model. Check the reference, or 'drakkar ls' for installed models.` |
| `models.not_installed` | model_not_found | both | 404 | terminal | `Model '{model}' is not installed. Run 'drakkar pull {model}', or pick an installed model from GET /v1/models.` |
| `models.repo_not_found` | model_not_found | both | 404 | terminal | `No repository '{repo}' on the hub. Check the spelling, or search: https://huggingface.co/models?search={repo}` |
| `models.gated_repo_no_token` | model_not_found | both | 404 | terminal | `{repo} is gated. Accept the license at {acceptance_url}, then provide a token (HF_TOKEN, ~/.huggingface, or keychain; RFC-0006 MP2).` |
| `models.unsupported_architecture` | format | both | 422 | terminal | `Architecture '{arch}' is not in the model-def layer of this build. Try a GGUF artifact ('drakkar pull {ref} --format gguf', backend B), or upgrade: architectures are added on a weekly cadence (RFC-0002 D3).` |
| `models.pickle_rejected` | format | both | 422 | terminal | `{file} is a pickle checkpoint; pickle executes code on load and is never accepted (RFC-0001 A11, RFC-0006 MP6). Use a safetensors or GGUF export of this model; conversion guidance: {docs_url}.` |
| `download.network_failed` | network | cli | 503 | after_backoff | `Download interrupted at {percent}% ({bytes_done} of {bytes_total}). Re-run the same command to resume; completed files are never re-fetched (RFC-0006 MP7).` |
| `download.hub_unreachable` | network | both | 503 | after_backoff | `Could not reach the hub: {cause}. Check connectivity and proxy settings; installed models keep working offline ('drakkar ls').` |
| `download.integrity_mismatch` | format | cli | 422 | terminal | `{file} failed integrity verification (expected {expected}, got {actual}); the blob was discarded (RFC-0006 MP8). Re-run to re-fetch; if it persists, pin a known-good revision with '@{rev}'.` |
| `download.no_space` | disk | cli | 507 | terminal | `Needs {needed_gib} GiB on {volume} (download + conversion workspace + output, RFC-0006 MP9); {free_gib} GiB free. 'drakkar prune' can reclaim {reclaimable_gib} GiB, or set storage.path to another volume.` |
| `store.write_failed` | disk | both | 507 | terminal | `Writing to the model store at {path} failed: {cause}. Check volume health and permissions; the store is reconstructible, so 'drakkar doctor' can verify and repair manifests.` |
| `store.corrupt_blob` | format | both | 422 | terminal | `Blob {blob} at {path} failed digest verification: its content does not match its name (RFC-0006 MP8, INV-CAS). 'drakkar rm {model}' then re-pull; 'drakkar doctor' quarantines the bad blob.` |
| `fit.wont_fit` | infeasible | both | 422 | terminal | `Needs {needed_gib} GiB even at the floor plan (lowest sane quant, 4k ctx, KV 8-bit); usable budget is {usable_gib} GiB (RFC-0004 FE19). Nearest sibling that fits: 'drakkar run {sibling}'. Override at your own risk with --force.` |
| `fit.context_exceeded` | infeasible | both | 413 | terminal | `prompt + max_tokens = {requested} exceeds the admissible {max_admissible_tokens} at the current KV precision. Reduce the request, or reload with --kv-bits 8 (ctx ceiling per precision: 'drakkar fit {model}').` |
| `kv.pool_exhausted` | infeasible | http | 429 | after | `KV pool at {pool_occupancy}% with no reclaimable blocks (RFC-0004 FE18). Retry after {retry_after_ms} ms, lower concurrency, or raise the pool via a smaller context ceiling.` |
| `grammar.schema_compile_failed` | usage | both | 422 | terminal | `The json_schema in response_format does not compile to a grammar: {reason}. Simplify the schema or use {"type":"json_object"} (RFC-0007 AS10).` |
| `server.unsupported_field` | usage | http | 400 | terminal | `Remove '{field}' or check capabilities via GET /v1/models. DRAKKAR never silently ignores parameters (RFC-0007 AS2).` |
| `server.model_loading` | engine | http | 503 | after_backoff | `{model} is loading ({progress_percent}%, ~{eta_s} s at current SSD bandwidth). Retry after {retry_after_ms} ms.` |
| `engine.load_failed` | engine | both | 500 | terminal | `Loading {model} failed in the backend: {cause}. 'drakkar doctor' checks the store and environment; 'drakkar rm {model}' then re-pull rules out a damaged artifact.` |
| `engine.metal_init_failed` | engine | both | 500 | terminal | `Metal device initialization failed: {cause}. 'drakkar doctor' reports GPU, macOS ({min_macos}+ required), and wired-limit status.` |
| `engine.inference_failed` | engine | both | 500 | terminal | `Generation failed mid-flight: {cause}. The sequence was aborted and its blocks freed. Recurrence on the same input is a bug — report it.` |
| `backend.metal_fault` | engine | both | 500 | terminal | `The backend reported a Metal fault: {backend_message} (RFC-0010). 'drakkar doctor' checks GPU and driver state; recurrence on the same input is a bug — report it.` |
| `backend.capability_absent` | engine | both | 500 | terminal | `The backend lacks a required capability: {capability} (RFC-0010, gated by Capabilities). Upgrade DRAKKAR or choose an artifact this build can run ('drakkar fit {model}').` |
| `backend.io` | engine | both | 500 | terminal | `The backend failed a weight I/O operation on {path}: {backend_message} (RFC-0010). Check volume health; 'drakkar rm {model}' then re-pull rules out a damaged artifact.` |
| `abi.version_mismatch` | internal | both | 500 | terminal | `Backend shim ABI is {found}, this binary expects {expected} (RFC-0010 AB3). The installation is inconsistent — reinstall DRAKKAR (brew reinstall drakkar or re-download).` |
| `abi.struct_size_mismatch` | internal | both | 500 | terminal | `An ABI struct is larger than the shim understands ({found} > {expected} bytes, RFC-0010 AB13). The installation is inconsistent — reinstall DRAKKAR.` |
| `abi.thread_violation` | internal | both | 500 | terminal | `A backend call crossed the one-thread contract (RFC-0010 AB6). This is a bug — open an issue with the log at {log_path}.` |
| `abi.invalid_argument` | internal | both | 500 | terminal | `The control plane passed an invalid argument across the ABI (RFC-0010 AB9). This is a bug — open an issue with the log at {log_path}.` |
| `internal.panic` | internal | both | 500 | terminal | `This is a bug in DRAKKAR. Re-run with --verbose for a backtrace and open an issue including the report at {report_path}.` |
| `internal.invariant` | internal | both | 500 | terminal | `A runtime invariant was violated ({invariant}); the operation was aborted. This is a bug — open an issue with 'drakkar doctor --json' output attached.` |
| `internal.budget_breach` | internal | both | 500 | terminal | `The engine exceeded its declared memory contract ({actual_gib} > {contract_gib} GiB), violating invariant I2. This is a bug — open an issue with 'drakkar doctor --json' output attached.` |

Registry notes:

- `fit.wont_fit` on the CLI is exit 4 and stops before any download (RFC-0008 CLI1);
  `--force` downgrades it to a printed warning and proceeds, per RFC-0008 §3 step 3.
- `engine.inference_failed` marks the failed sequence only; the batch continues, and the
  code is `terminal` — a second identical failure for the same input is a deterministic
  engine bug whose remedy is to report it, not to retry.
- The closed prefix set admits no `api`, `net`, `sched`, or `convert` namespace:
  API-layer request errors are `server.*`; network and download failures are `download.*`;
  scheduler rejections are always feasibility decisions and use `fit.*` / `kv.*` codes;
  conversion failures surface under `engine.*` (workspace/compute), `download.*` (fetch),
  or `store.*` (write) as appropriate — one source of truth, RFC-0001 I3.
- A backend-reported OOM or KV exhaustion is never surfaced as an infeasible user error:
  by invariant I2, admission control (not the backend) discovers memory limits, so such a
  status maps to `internal.*` (`internal.budget_breach` / `internal.invariant`), per
  RFC-0010 AB9 and RFC-0011 ER8.

## 5. Wire envelopes

### 5.1 Internal type (`drakkar-core`)

All envelopes below serialize from one flat struct; there is no per-surface error type and
no enum-of-variants sub-error design. The `code` is a variant of the closed `ErrorCode`
enum — the registry (§4) IS this enum:

```rust
pub struct DkError {
    pub code: ErrorCode,          // closed enum, one variant per §4 row
    pub category: ErrorCategory,  // §2
    pub message: String,          // human, domain terms, no secrets
    pub remedy: Option<Remedy>,   // typed template; None only pre-render (INV-REMEDY-ALWAYS)
    pub retry: Retry,             // §4 pins the class per code
    pub context: ErrorContext,    // typed key/value fields; per-code schema, additive-only
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>, // cause chain; logged at origin, never serialized
}

impl DkError {
    pub fn code(&self) -> ErrorCode;
    pub fn category(&self) -> ErrorCategory;  // from ErrorCode, §4
    pub fn exit_code(&self) -> u8;            // category mapping, §2
    pub fn http_status(&self) -> StatusCode;  // category default + per-code override, §2/§3
    pub fn remedy(&self) -> Option<&Remedy>;
}

/// Closed enum: the registry (§4) is exactly this set. Each variant renders to a stable
/// &'static str via as_str() in `subsystem.snake_case` form; the wire/JSON code is that
/// string. Adding a variant requires a matching §4 row; the CI exhaustive-match and golden
/// tuple snapshot enforce the correspondence (INV-SINGLE-TAXONOMY).
pub enum ErrorCode {
    CliInvalidArgs, CliMissingModelArg,
    ConfigInvalidKey, ConfigInvalidValue,
    ModelsNotFound, ModelsNotInstalled, ModelsRepoNotFound, ModelsGatedRepoNoToken,
    ModelsUnsupportedArchitecture, ModelsPickleRejected,
    DownloadNetworkFailed, DownloadHubUnreachable, DownloadIntegrityMismatch, DownloadNoSpace,
    StoreWriteFailed, StoreCorruptBlob,
    FitWontFit, FitContextExceeded,
    KvPoolExhausted,
    GrammarSchemaCompileFailed,
    ServerUnsupportedField, ServerModelLoading,
    EngineLoadFailed, EngineMetalInitFailed, EngineInferenceFailed,
    BackendMetalFault, BackendCapabilityAbsent, BackendIo,
    AbiVersionMismatch, AbiStructSizeMismatch, AbiThreadViolation, AbiInvalidArgument,
    InternalPanic, InternalInvariant, InternalBudgetBreach,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str;      // e.g. ErrorCode::KvPoolExhausted => "kv.pool_exhausted"
    pub fn category(self) -> ErrorCategory;   // static, from §4 (exhaustive match, no wildcard)
    pub fn exit_code(self) -> u8;             // §2
    pub fn http_status(self) -> StatusCode;   // §2 default + §3 override
}

pub enum ErrorCategory {
    Usage, ModelNotFound, Infeasible, Network, Format, Engine, Disk, Internal,
}

pub enum Retry {
    Terminal,                       // default; retrying without change fails identically
    AfterBackoff,                   // transient; client-chosen exponential backoff + jitter
    After { after_ms: u64 },        // computed delay; kv.pool_exhausted / server.model_loading
}

pub struct Remedy {
    pub rendered: String,           // single-line, runnable where possible
    pub template: &'static str,     // registered template id, e.g. "run_sibling"
    pub params: ErrorContext,       // typed params that filled the template
}
```

`context` keys are part of the schema: for a given code, keys are additive-only across
releases exactly like the registry itself (INV-ADDITIVE-REGISTRY). JSON serialization:
`retry` renders as `{ "kind": "terminal|after_backoff|after", "after_ms": <n|null> }`;
`remedy` renders as `{ "rendered": "...", "template": "...", "params": {...} }` or `null`.

### 5.2 CLI `--json` envelope

On failure, a `--json` invocation prints exactly one JSON object on stdout (stdout stays
machine-parseable, RFC-0008 CLI6) and exits with the category's code. The object is the
canonical `drakkar.error/1` object (schema tag `drakkar.error/1`, C7):

```json
{
  "schema": "drakkar.error/1",
  "code": "fit.wont_fit",
  "category": "infeasible",
  "message": "Llama-3.3-70B-4bit needs 39.1 GiB even at the floor plan; usable budget is 34.2 GiB.",
  "remedy": {
    "rendered": "drakkar run qwen3:30b-a3b",
    "template": "run_sibling",
    "params": { "sibling": "qwen3:30b-a3b" }
  },
  "retry": { "kind": "terminal", "after_ms": null },
  "context": {
    "needed_gib": 39.1,
    "usable_gib": 34.2,
    "sibling": "qwen3:30b-a3b"
  },
  "exit_code": 4
}
```

Streaming commands under `--stream-json` (RFC-0008 CLI7) emit the same object as the
payload of a terminal `error` event, then exit with the mapped code:

```json
{"event":"error","code":"engine.inference_failed","category":"engine","message":"...","remedy":null,"retry":{"kind":"terminal","after_ms":null},"context":{},"exit_code":6}
```

Human (non-`--json`) rendering follows RFC-0008 CLI15: one block stating what failed, why
in domain terms, and the remedy line; stack traces only under `--verbose`.

### 5.3 HTTP, OpenAI dialect

Status per §3; body is the OpenAI error envelope with the DRAKKAR code in the standard
`code` slot and the full `drakkar.error/1` object under `x_drakkar` (same extension
namespace as usage accounting, RFC-0007 AS6):

```json
{
  "error": {
    "message": "prompt + max_tokens = 141312 exceeds the admissible 131072 at the current KV precision.",
    "type": "invalid_request_error",
    "param": "max_tokens",
    "code": "fit.context_exceeded",
    "x_drakkar": {
      "schema": "drakkar.error/1",
      "code": "fit.context_exceeded",
      "category": "infeasible",
      "retry": { "kind": "terminal", "after_ms": null },
      "remedy": {
        "rendered": "Reduce the request, or reload with --kv-bits 8.",
        "template": "reduce_context",
        "params": { "max_admissible_tokens": 131072 }
      },
      "context": { "max_admissible_tokens": 131072 }
    }
  }
}
```

For 429, `retry_after_ms` and `pool_occupancy` appear in `x_drakkar.context` and a
`Retry-After` header (seconds, rounded up) is set. If the error occurs after streaming has
begun, the response status is already committed; the server emits one final SSE frame whose
data is the envelope above, then closes the stream without a `[DONE]` sentinel — clients
treat a close without `[DONE]` as abnormal termination.

### 5.4 HTTP, Anthropic dialect

```json
{
  "type": "error",
  "error": {
    "type": "rate_limit_error",
    "message": "KV pool at 97% with no reclaimable blocks. Retry after 1800 ms.",
    "x_drakkar": {
      "schema": "drakkar.error/1",
      "code": "kv.pool_exhausted",
      "category": "infeasible",
      "retry": { "kind": "after", "after_ms": 1800 },
      "remedy": {
        "rendered": "Retry after 1800 ms, or lower max_tokens.",
        "template": "retry_after_or_reduce",
        "params": { "retry_after_ms": 1800 }
      },
      "context": { "retry_after_ms": 1800, "pool_occupancy": 0.97 }
    }
  }
}
```

`error.type` follows the §3 mapping; the DRAKKAR code always rides in
`error.x_drakkar.code` (the Anthropic envelope has no native `code` slot; reference SDKs
ignore unknown fields, verified by the RFC-0007 AC1 trace suites). Mid-stream errors use
the dialect's native `event: error` SSE event with this body, then the stream closes
without `message_stop`.

## 6. Retryability contract

The `retry` field is a promise to agent clients, defined precisely by its three kinds
(§5.1):

- `{"kind":"terminal"}` — resubmitting the identical request will fail identically until a
  human or agent acts on the remedy. Clients MUST NOT retry-loop these. This is the default
  for every category except `network`.
- `{"kind":"after_backoff"}` — transient; retry with client-chosen exponential backoff and
  jitter (`network` class, `server.model_loading`). The download layer already retries
  internally with resume-from-offset (RFC-0006 MP7); the error surfaces only after those
  attempts are exhausted, still marked `after_backoff` so an outer agent may retry the
  whole operation.
- `{"kind":"after","after_ms":N}` — the server computed a hint (`kv.pool_exhausted` from
  the expected release horizon of active sequences; `server.model_loading` may also carry a
  computed delay). Retrying earlier is permitted but wasteful. On HTTP 429/503 this sets
  `Retry-After` (seconds, rounded up); `retry_after_ms` also appears in `context`.

Retryability is a property of the code (§4 pins the class), not of the individual
occurrence.

## 7. Propagation rules

1. **Origin conversion.** The subsystem that detects a failure constructs the
   `DkError` (choosing the code, filling `context`) and logs it there — the only
   log record for this failure (INV-LOG-ONCE). Everything upstream is transport.
2. **Engine actor boundary.** The engine actor's message loop runs each message under
   `catch_unwind`. A panic in Rust-side code converts to `internal.panic`, fails
   the in-flight sequences with that error, marks the model failed, and unloads it; the
   actor thread itself survives or is respawned by the pool manager. A Metal-level fault
   surfaced as an error (not a panic) converts to `engine.inference_failed` and fails
   only the affected sequence. FFI faults MUST NOT unwind across the C ABI: the shim
   returns `dk_status` codes, and the Rust side maps them into the taxonomy (RFC-0010 AB9,
   [Backend ABI](../rfcs/RFC-0010-backend-abi.md)) — `abi.*` for ABI-contract faults,
   `backend.*` for compute faults, `internal.*` for memory-pressure statuses;
   `catch_unwind` at the actor is the backstop for Rust-side panics, not a substitute for
   ABI discipline.
3. **HTTP handler boundary.** Handlers are wrapped in a panic-catching layer; a caught
   panic renders as `internal.panic` / 500 in the caller's dialect. A dead engine actor
   (broken scheduler channel) renders as `internal.invariant`.
4. **CLI top level.** `main` wraps the command dispatch in `catch_unwind` and converts
   any escaped panic to `internal.panic`, printed per CLI15 and exiting 6. No Rust panic
   message or default abort handler output ever reaches a user (RFC-0008 AC5).
5. **Context, not wrapping.** Intermediate layers MAY append context strings to the
   cause chain (visible under `--verbose` and in logs) but MUST NOT change the code,
   category, or remedy chosen at origin. Re-categorizing an error away from its origin
   is how taxonomies rot.
6. **Aggregation.** When one user action triggers multiple failures (e.g., parallel
   ranged downloads), the surface reports the single most actionable error (highest
   category severity, ties broken by first occurrence); the rest are log records only.

## 8. Registry governance and schema identifiers

Two distinct identifier strings are in play and MUST NOT be conflated:

- **`drakkar.error/1`** is the schema tag of the error JSON **object**. It is the value of
  the `schema` field in the CLI `--json` envelope, in the `--stream-json` error event, and
  in the `x_drakkar` slot of both HTTP dialect error bodies (§5). It names the *shape* of a
  single serialized error.
- **`drakkar.errors/1`** is the version of the code **registry** contract — the additive-only
  governance version of the set of codes, categories, statuses, and context keys in §4. It
  appears only in governance prose (this section, the header, INV-ADDITIVE-REGISTRY). It is
  **never** the value of an envelope's `schema` field.

The object tag and the registry version bump independently: a change to the object shape
would bump `drakkar.error/1` → `/2`; a breaking change to the code registry (removal,
rename, re-categorization) would bump `drakkar.errors/1` → `/2`. Neither is anticipated.

- The registry contract `drakkar.errors/1` bumps to `/2` only on a breaking change, which
  requires a superseding RFC amending
  [RFC-0011](../rfcs/RFC-0011-error-taxonomy.md) — none is anticipated.
- Adding a code is a PR that edits exactly two places: this table (§4) and the
  `drakkar-core` `ErrorCode` enum. CI enforces the sync by exhaustive match plus the golden
  tuple snapshot (§1, §4) and rejects rows whose category, HTTP status, or exit code
  contradict §2/§3.
- Milestone growth is expected and normal: v0.1 ships the `cli`, `config`, `models`,
  `download`, `store`, `fit`, `engine`, `backend`, `abi`, `internal` rows above; v0.2 adds
  the `kv.*`, `server.*`, and `grammar.*` serving rows alongside continuous batching,
  structured output, and the second HTTP dialect; v0.3+ adds daemon- and pool-lifecycle
  codes under the existing categories. No milestone may change an existing row.
