# 05 — Observability

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Sources: RFC-0007 AS21-AS22 ([API Server](../rfcs/RFC-0007-api-server.md#proposed-design)), RFC-0005 KV23-KV24 ([KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)), RFC-0003 IC27 ([Inference Core](../rfcs/RFC-0003-inference-core.md#proposed-design)), RFC-0008 CLI9/CLI12/CLI15/CLI16 ([CLI and UX](../rfcs/RFC-0008-cli-ux.md#proposed-design)), RFC-0001 A10/A12 ([Architecture](../rfcs/RFC-0001-architecture.md#proposed-design)), [PRD P13](../../PRD.md#52-non-functional)

This document is the single specification of what DRAKKAR observes about itself, how it exposes that data, and — with equal weight — what it MUST NOT observe. Observability serves two audiences: the operator diagnosing a local install (`drakkar doctor`, logs, `ps`) and a metrics scraper watching a long-running `serve` (`GET /metrics`, optional OTLP). It serves no third party: DRAKKAR emits nothing off-machine without explicit, per-destination opt-in (PRD P13, RFC-0008 CLI16, RFC-0001 A10).

## 1. Invariants

- **O1 — Content-blind by default.** No prompt text, completion text, tool arguments, image bytes, or chat-template output appears in any log record, metric name, metric label, trace attribute, or diagnostics bundle unless the operator passes `--log-bodies` for that specific process invocation (RFC-0007 AS19). Metadata (token counts, timings, hashes, outcomes) is always fair game; content never is.
- **O2 — Bounded cardinality.** Every metric label value comes from a closed enumeration defined in this document, or from the set of locally installed model names. No label value is ever derived from request content, headers, client identity, or user input.
- **O3 — Observation never blocks inference.** The engine actor (RFC-0001 A2/I1) publishes counters and snapshots via atomics and a per-step stats struct; rendering `/metrics`, `drakkar ps`, or log sinks happens on tokio/blocking threads and MUST NOT send messages to, or take locks shared with, the engine thread.
- **O4 — No egress without opt-in.** The only network destinations observability may touch are those the operator names explicitly (`--otel <endpoint>`). There is no default collector, no crash uploader, no usage ping (CLI16). `doctor --check-update` is on-demand and sends no payload beyond the version-check request.
- **O5 — One truth per number.** `drakkar ps`, `GET /metrics`, the `x_drakkar` usage extensions (RFC-0007 AS6), and `--json` outputs all render from the same internal stats structs (RFC-0008 CLI6 discipline applied to observability). A number MUST NOT be computed two ways in two surfaces.

## 2. Structured logging contract

### 2.1 Framework and streams

- OBS1. All logging goes through the `tracing` crate (workspace-pinned; structured spans + events, per RFC-0007 AS22). Free-form `println!`/`eprintln!` logging is forbidden outside the CLI's human-rendering layer. Crates log under their own `target` (`drakkar_server`, `drakkar_sched`, `drakkar_engine`, `drakkar_kv`, `drakkar_models`, `drakkar_fit`, `drakkar_cli`), so per-subsystem filtering works with standard `DRAKKAR_LOG` env-filter syntax (e.g. `DRAKKAR_LOG=info,drakkar_kv=debug`).
- OBS2. Sink layout:
  - Foreground commands: human-readable log lines to **stderr** only; stdout is reserved for command output and `--json` payloads (RFC-0008 CLI6). `--quiet` suppresses all non-error stderr; each `-v` raises the level one step (`INFO` → `DEBUG` → `TRACE`); `NO_COLOR` and non-TTY stderr disable ANSI (CLI9).
  - `drakkar serve` (foreground): stderr as above **plus** the file sink.
  - `drakkar serve --daemon`: file sink only; `drakkar serve --logs` tails it (RFC-0008 CLI12).
- OBS3. File sink: JSON Lines (one record per line, schema `drakkar.log/1`) written to `~/.drakkar/logs/drakkar.log`. Rotation: daily at UTC midnight **and** whenever the active file exceeds 64 MiB, whichever comes first; rotated files are named `drakkar.log.<YYYY-MM-DD>.<n>`; retention keeps the newest 14 files or 256 MiB total, whichever bound trips first, deleting oldest-first. Log files are created mode `0600` inside a `0700` directory. Rotation and retention run in-process (blocking pool, RFC-0001 A4); no external logrotate dependency.
- OBS4. Log record schema (`drakkar.log/1`, additive-only within the major version):

```json
{
  "ts": "2026-07-14T09:31:07.412Z",
  "level": "INFO",
  "target": "drakkar_server::request",
  "message": "request completed",
  "request_id": "01J2X9M3T4V5W6X7Y8Z9AB01",
  "span": ["request", "decode"],
  "fields": {
    "endpoint": "chat_completions",
    "dialect": "openai",
    "model": "qwen3-8b-4bit",
    "prompt_tokens": 8231,
    "cached_tokens": 8192,
    "completion_tokens": 214,
    "ttft_ms": 182,
    "itl_ms_p50": 11.9,
    "itl_ms_p95": 19.4,
    "outcome": "ok",
    "duration_ms": 2891
  }
}
```

  `ts` is ISO 8601 UTC with millisecond precision. `fields` keys are typed and enumerated per event kind; unknown keys are permitted for readers (additive evolution).

### 2.2 Request identity and spans

- OBS5. Every HTTP request gets a `request_id`: a UUIDv7 minted at accept time, unless the client supplied a valid `X-Request-Id` header (≤ 64 chars, `[A-Za-z0-9_-]+`; anything else is discarded and replaced, never logged raw). The id is echoed in the `X-Request-Id` response header, embedded in structured error bodies (see [04 — Error Model](04-error-model.md)), attached to every log record and span in the request's lifetime, and carried as the OTLP trace correlation key. One-shot CLI generations mint a `request_id` the same way so `--stream-json` events and logs correlate.
- OBS6. Span hierarchy per request, matching the lifecycle in RFC-0001 §6: `request{endpoint, dialect, model}` → `normalize` → `tokenize` → `admission{decision}` → `prefix_lookup{hit_tokens}` → `prefill{chunk_index, chunk_tokens}` (repeated) → `decode{steps}` → `finalize{outcome}`. Span timings are the source for the TTFT/ITL figures in usage accounting (AS6) and metrics — the same measurement, per invariant O5.

### 2.3 What is logged at each level

- OBS7. Level contract (what an operator may rely on finding, and the ceiling of what may appear):

| Level | Contents | Examples |
| --- | --- | --- |
| `ERROR` | Failures that lost work or violated a contract | engine/Metal failure, memory-contract breach detected by `memory_report()` (RFC-0001 I2), disk-tier corruption, caught panic (rendered per RFC-0008 CLI15: cause in domain terms + remedy + bug-report hint; backtrace only under `--verbose`) |
| `WARN` | Degradation and refusals that self-healed or were policy | admission rejections (413/429) with remediation fields, NAX self-test failure (RFC-0003 IC26), stale/invalid config keys, KV disk restore slower than the 3x eligibility bar (KV18), eviction under pool pressure, user alias shadowing a shipped alias |
| `INFO` | Lifecycle + one completion record per request (metadata only, schema in OBS4) | server start/stop with bind address, model load/unload with budget breakdown and residency transitions (RFC-0007 AS17), calibration file loaded, KV disk tier enabled/disabled, rotation events |
| `DEBUG` | Scheduling and cache mechanics | batch composition per step-window (size, occupancy), chunk-budget adaptation (AS13), prefix-lookup results (matched block count, truncated hash-chain prefix), evictions with score/reason, speculation acceptance rates, keep-alive timers |
| `TRACE` | Per-step engine internals | per-decode-step duration, channel depths, graph-compile cache events (RFC-0003 IC2), per-block disk-tier I/O |

  Default level: `INFO` for `serve` and the file sink, `WARN` for stderr on quiet-ish CLI paths where progress UI already reports state. `DEBUG`/`TRACE` are never defaults anywhere. Content-blindness (O1) holds at **every** level: `TRACE` may log token *counts* and *hashes*, never token *ids in sequence* nor decoded text (a token-id sequence is the prompt).
- OBS8. Truncated content hashes (first 16 hex chars of the KV12 hash-chain keys) MAY appear at `DEBUG`+ for cache diagnostics: they are one-way content addresses, not content. Full hash chains appear only in `TRACE`.

### 2.4 Body logging (`--log-bodies`)

- OBS9. `--log-bodies` is the only mechanism that lifts O1, and it is deliberately inconvenient: it is a process flag with **no config-file or environment equivalent**, so it cannot be enabled persistently by accident; it MUST print a sensitive-data warning to stderr at startup naming the log path; body records are written only to the file sink, at `DEBUG`, with `"sensitive": true` in the record, and the daemon status output (`serve --status`) MUST show that body logging is active. Bundles (OBS31) exclude sensitive records even when present in the files.

### 2.5 Secret redaction

- OBS10. A redaction layer sits between `tracing` and every sink (file, stderr, OTLP). It replaces the values of any field whose key case-insensitively matches the denylist {`authorization`, `api_key`, `api-key`, `x-api-key`, `hf_token`, `token`, `cookie`, `set-cookie`} with `"[redacted]"`, and scrubs userinfo and known credential query parameters from any URL-shaped value. HF tokens are read from the standard locations or keychain and MUST never be written to logs (RFC-0001 A12) — including inside download URLs and error messages from the hub client (`drakkar-models` wraps hub errors before they reach `tracing`).
- OBS11. Property test (release-gating): a fuzz corpus of synthetic secrets injected through every request path, config key, and failing-download path never appears in any sink output byte stream. This test is part of the `server` area CI suite from v0.1.

## 3. Metrics catalog

### 3.1 Conventions

- OBS12. Exposition: Prometheus text format at `GET /metrics` (endpoint table, RFC-0007 §2 — [API Server](../rfcs/RFC-0007-api-server.md#proposed-design)). Rendering reads the O3 snapshot; a scrape costs no engine-thread time. When the server binds a non-loopback interface (AS18), `/metrics` requires the API key; `/health` remains unauthenticated.
- OBS13. Naming rules: prefix `drakkar_`; base units in the name (`_seconds`, `_bytes`, `_joules`); counters end `_total`; ratios are gauges in `[0,1]` ending `_ratio` and are always derived from exported counters (the counters are the source of truth; ratio gauges are computed over a sliding 5-minute window for dashboard convenience). The `model` label is the resolved local model name (bounded by installed models, per O2).
- OBS14. Every metric below states its type, labels (with the closed value set), and — for histograms — explicit bucket boundaries. Buckets are chosen so the RFC-0009 Tier-1 targets fall inside distinct buckets (a target regression is visible in a scrape, not smeared across one bucket).

### 3.2 Server and request metrics

| Metric | Type | Labels | Notes |
| --- | --- | --- | --- |
| `drakkar_build_info` | gauge (const 1) | `version`, `backend` (`mlx`\|`gguf`), `mlx_pin`, `macos` | identity for dashboards and bug reports |
| `drakkar_requests_total` | counter | `endpoint` (`chat_completions`\|`completions`\|`messages`\|`embeddings`\|`models`\|`fit`\|`health`\|`metrics`), `dialect` (`openai`\|`anthropic`\|`drakkar`), `outcome` (`ok`\|`invalid_request`\|`context_exceeded`\|`kv_pool_exhausted`\|`model_loading`\|`cancelled`\|`engine_error`) | `outcome` values map 1:1 to the error taxonomy in [04 — Error Model](04-error-model.md) |
| `drakkar_request_duration_seconds` | histogram | `endpoint`, `outcome` | buckets: 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 30, 60, 120, 300 |
| `drakkar_requests_in_flight` | gauge | `endpoint` | |
| `drakkar_streams_active` | gauge | — | open SSE streams |
| `drakkar_stream_disconnects_total` | counter | `endpoint` | client-initiated cancels (AS4); pairs with the AC4 leak check in RFC-0007 |

### 3.3 Latency and throughput metrics (per RFC-0007 AS21)

| Metric | Type | Labels | Notes |
| --- | --- | --- | --- |
| `drakkar_ttft_seconds` | histogram | `model`, `temperature` (`cold`\|`warm`) | cold/warm per RFC-0009 PB1: warm = any prefix-cache hit ≥ 1 block; load-from-disk time is **never** folded in (separate `drakkar_model_load_duration_seconds`). Buckets: 0.05, 0.1, 0.15, 0.2, 0.3, 0.5, 0.7, 1.0, 1.5, 2.0, 3.0, 5.0, 10, 30 — brackets every PB12 TTFT target |
| `drakkar_itl_seconds` | histogram | `model` | per-token decode intervals. Buckets: 0.005, 0.01, 0.02, 0.03, 0.045, 0.06, 0.09, 0.12, 0.2, 0.5, 1.0 — brackets the 30/45/55/60 ms agent targets and the AS13 ITL guard |
| `drakkar_prompt_tokens_total` | counter | `model`, `source` (`computed`\|`cache`) | `cache` counts tokens served from prefix hits (KV23 numerator) |
| `drakkar_completion_tokens_total` | counter | `model` | |
| `drakkar_prefill_tokens_per_second` | gauge | `model` | 5-min window over computed prefill tokens; the NAX-path regression signal (RFC-0003 IC12) |
| `drakkar_decode_tokens_per_second` | gauge | `model` | 5-min window; compare against the FE21 roofline |

### 3.4 Scheduler metrics

| Metric | Type | Labels | Notes |
| --- | --- | --- | --- |
| `drakkar_batch_occupancy` | gauge | `model` | sequences in the current decode batch (AS12); sampled per step-window |
| `drakkar_queue_depth` | gauge | `priority` (`interactive`\|`batch`) | AS15 queues |
| `drakkar_queue_wait_seconds` | histogram | `priority` | buckets: 0.001, 0.01, 0.05, 0.1, 0.5, 1, 5, 30, 120 |
| `drakkar_admissions_total` | counter | `model`, `decision` (`admitted`\|`rejected_context`\|`rejected_pool`) | FE18 admission control outcomes |
| `drakkar_prefill_chunk_tokens` | gauge | `model` | current adaptive chunk budget (256-2048, AS13); its trajectory is the ITL-guard diagnostic |
| `drakkar_speculation_tokens_total` | counter | `model`, `tier` (`ngram`\|`draft`), `result` (`accepted`\|`rejected`) | acceptance rate drives the IC18 auto-disable; the occupancy crossover (IC21) shows up as this counter flatlining at high `drakkar_batch_occupancy` |

### 3.5 KV cache metrics (RFC-0005 KV23)

| Metric | Type | Labels | Notes |
| --- | --- | --- | --- |
| `drakkar_kv_pool_blocks` | gauge | `model`, `state` (`free`\|`active`\|`cached`) | the three KV4 states; sums to pool size — the AC4 leak check asserts accounting equality against this |
| `drakkar_kv_pool_bytes` | gauge | `model` | pool size from the KV2 carve-out |
| `drakkar_kv_prefix_hit_ratio` | gauge | `model` | cached prompt tokens / total prompt tokens, 5-min window; counters in §3.3 are the source |
| `drakkar_kv_evictions_total` | counter | `model`, `reason` (`ttl`\|`pressure`\|`invalidated`\|`manual`) | KV20/KV21 reclaim order made visible; `invalidated` = KV12 key changes |
| `drakkar_kv_disk_bytes` | gauge | — | disk-tier usage vs the 8 GiB default budget (KV19) |
| `drakkar_kv_disk_ops_total` | counter | `op` (`persist`\|`restore`), `outcome` (`ok`\|`error`\|`skipped`) | `skipped` = failed the KV18 ≥ 3x cost-model eligibility |
| `drakkar_kv_disk_lookups_total` | counter | `result` (`hit`\|`miss`) | disk-tier hit rate (KV23) |
| `drakkar_kv_disk_restore_bytes_per_second` | gauge | — | last-restore streaming bandwidth; validates the KV18 cost model against calibrated SSD bandwidth |

### 3.6 Memory metrics (RFC-0001 I2, RFC-0003 IC25)

| Metric | Type | Labels | Notes |
| --- | --- | --- | --- |
| `drakkar_memory_contract_bytes` | gauge | `model` | declared budget at load |
| `drakkar_memory_actual_bytes` | gauge | `model` | from `memory_report()`; `actual > contract` is a contract breach — alert-worthy always, release-blocking in CI (RFC-0009 PB4/PB16) |
| `drakkar_memory_component_bytes` | gauge | `model`, `component` (`weights`\|`kv_pool`\|`activation_watermark`\|`runtime_overhead`) | the I2 decomposition, so a breach is attributable |
| `drakkar_memory_wired_limit_bytes` | gauge | — | active GPU-wired limit; moves if the operator applies the RFC-0004 wired-limit remedy |

### 3.7 Model lifecycle metrics

| Metric | Type | Labels | Notes |
| --- | --- | --- | --- |
| `drakkar_model_resident` | gauge (0/1) | `model` | load/unload transitions are announced here and in `INFO` logs (AS17) |
| `drakkar_model_load_duration_seconds` | histogram | `model` | buckets: 0.5, 1, 2, 4, 8, 16, 32, 64, 128 — SSD-bandwidth-bound expectation (IC24) |
| `drakkar_model_loads_total` | counter | `model`, `outcome` (`ok`\|`error`) | |
| `drakkar_model_unloads_total` | counter | `model`, `reason` (`keep_alive`\|`manual`\|`pool_evict`) | `pool_evict` appears with the v0.3 pool |

### 3.8 Energy metrics (RFC-0003 IC27, RFC-0009 PB5)

| Metric | Type | Labels | Notes |
| --- | --- | --- | --- |
| `drakkar_energy_joules_total` | counter | — | integrated from powermetrics sampling |
| `drakkar_power_watts` | gauge | — | last sample |

- OBS15. Energy metrics exist only while power sampling is active. Sampling requires elevated privileges (powermetrics), so it is off by default, enabled by `drakkar bench` (which owns the tokens/joule report) or by an explicit `serve --power-sampling` opt-in; absence of the metrics means sampling is off, and `/metrics` MUST NOT expose stale energy values.
- OBS16. `drakkar ps` renders the §3.3-§3.7 snapshot per resident model (residency, pool occupancy by state, hit ratio, throughput), and `ps --json` includes the full stats struct (RFC-0005 KV24) under schema `drakkar.ps/1` — same struct, same numbers as `/metrics`, per invariant O5.

## 4. Redaction and privacy rules (hard requirements)

- OBS17. Logs and metrics MUST NOT contain prompt or completion bodies, rendered chat templates, tool-call arguments or results, or token-id sequences, at any level, by default (invariant O1; RFC-0007 AS19). The only exception is OBS9's `--log-bodies`.
- OBS18. Metric label values MUST come from the closed enumerations in §3 or the installed-model name set (invariant O2). Code review plus a CI test that scrapes `/metrics` under a hostile-input request corpus and asserts the label-value universe enforces this.
- OBS19. HF tokens, API keys, and any credential material MUST never reach a sink (OBS10; RFC-0001 A12). The AS18 API key is checked with a constant-time compare and is never logged, not even fingerprinted.
- OBS20. KV disk-tier files under `~/.drakkar/kv-cache/` are user prompt content in serialized form: mode `0600`, treated as sensitive, and excluded from diagnostics bundles by default (RFC-0005 KV19). `drakkar cache ls` shows sizes and ages, never content.
- OBS21. REPL history (`~/.drakkar/`, RFC-0008 CLI5) and saved conversations are content: excluded from bundles, never logged.
- OBS22. Per-request cache opt-out (`cache: false`, locked decision LD8) implies observability opt-out of cache artifacts too: an opted-out request donates nothing to RAM or disk tiers, and its prefix hashes appear in no `DEBUG`/`TRACE` record.
- OBS23. No telemetry, ever, without explicit opt-in (RFC-0008 CLI16; PRD P13). There is no "anonymous usage statistics" middle ground in v1: the config key `telemetry` exists, is `off` by default, and no code path reads it as anything but `off` until a future opt-in design ships with its own documented spec. Crash-free-rate metrics (PRD M6) come only from opt-in crash reports, which do not exist before v1.0.

## 5. OTLP export (`--otel`)

- OBS24. `drakkar serve --otel <endpoint>` (config: `otel.endpoint`, plus `otel.protocol = "grpc" | "http"`, default `grpc`) exports traces (the OBS6 span tree) and metrics (the §3 catalog) via OTLP to the named endpoint. Off unless an endpoint is explicitly configured (invariant O4); there is no default endpoint and no service-discovery. Export is optional in the build sense too: it adds no startup cost when unconfigured.
- OBS25. Exported spans carry the same redaction layer (OBS10) and the same content-blindness (O1): span attributes are the OBS4/OBS6 metadata fields only. `--log-bodies` does **not** extend to OTLP; bodies never leave the machine over the export path under any flag combination.
- OBS26. Resource attributes: `service.name = "drakkar"`, `service.version`, plus `drakkar.backend` and `drakkar.chip` (from the IC26 capability probe). Trace ids correlate with `request_id` via an explicit span attribute so a log line and an exported trace meet in the middle.

## 6. `drakkar doctor` — the operator diagnostic surface

- OBS27. `drakkar doctor` is the canonical "why is this machine behaving this way" command (RFC-0008 §2). It runs a fixed, versioned check list and renders human output and `--json` (schema `drakkar.doctor/1`) from the same results struct. Exit code: 0 if no check fails, 6 if any `fail` (engine-relevant) check fails, per the RFC-0008 CLI8 code table.
- OBS28. Check list (each check reports `pass | warn | fail`, a measured value, and — on non-pass — the single most useful remedy per RFC-0008 CLI15):

| Check | Verifies | Source |
| --- | --- | --- |
| `chip` | chip identity, GPU core count, bandwidth class | IC26 capability probe |
| `macos` | version ≥ 15; notes 26.2+ NAX eligibility | PRD P15 |
| `nax` | Metal 4 tensor-op **functional self-test** result (not version sniffing) | RFC-0003 IC26; a silent-off here is the LM Studio-incident anti-pattern |
| `memory_budget` | total RAM, GPU-wired limit (default vs sysctl-modified), usable budget | RFC-0004 model |
| `disk` | free space on the store volume; store path writable | RFC-0006 / LD14 |
| `store` | content-store index consistency (dangling links, orphan blobs, reclaimable bytes) | RFC-0006 |
| `config` | config parses; unknown/stale keys flagged; value ranges valid | RFC-0008 CLI11 |
| `logs` | log directory writable; rotation healthy; total size within retention | OBS3 |
| `kv_disk` | disk-tier index sidecar consistent; usage vs budget; file modes are `0600` | RFC-0005 KV17/KV19 |
| `calibration` | per-chip calibration file present/absent and its age | RFC-0009 PB15 |
| `port` | configured bind address:port available or already held by a live daemon | RFC-0007 AS18 |
| `daemon` | launchd agent state matches `serve --status` | RFC-0008 CLI12 |

- OBS29. `doctor` is offline: it makes no network call. `doctor --check-update` is the sole, explicit, on-demand exception (CLI16) and reports only the latest released version against the running one.
- OBS30. `doctor` output states which fast paths are active (NAX on/off, KV disk tier on/off, calibrated vs shipped fit constants) so a benchmark discussion always starts from a machine-state disclosure — the honesty rule applied to support threads.

### 6.1 Diagnostics bundle

- OBS31. `drakkar doctor --bundle` writes `drakkar-diagnostics-<YYYY-MM-DD>.tar.gz` to the current directory for attachment to bug reports. Contents, all passed through the OBS10 redaction filter a second time at bundle time:
  - `doctor.json` (the OBS27 results),
  - the last 7 days of log files with `"sensitive": true` records stripped (OBS9),
  - `config.toml` with the denylist keys removed,
  - calibration files (`~/.drakkar/calibration/`),
  - a store manifest (model names, formats, quants, sizes, content hashes — no weights).

  Excluded always, not optionally: the KV disk tier (`~/.drakkar/kv-cache/`, per KV19), REPL history and saved conversations (OBS21), model weights, HF tokens, API keys. The bundle prints its own file list to stderr before writing so the operator sees exactly what will be shared.

## 7. Rollout

| Milestone | Observability scope |
| --- | --- |
| v0.1 "First light" | `tracing` + file sink + rotation (OBS1-OBS4), request ids/spans (OBS5-OBS6), redaction layer + fuzz property test (OBS10-OBS11), `/metrics` with §3.2, §3.3, §3.6, §3.7 (single model, no scheduler/KV-pool metrics yet), `doctor` checks minus `kv_disk`/`daemon`, exit-code contract |
| v0.2 "Convoy" | scheduler metrics (§3.4), KV pool metrics (§3.5 RAM rows), speculation counters, energy metrics via `bench` (OBS15), bucket boundaries locked from first fleet data (see open question), CI label-cardinality test (OBS18) |
| v0.3 "Fleet" | disk-tier metrics (§3.5 disk rows), `kv_disk` + `daemon` doctor checks, multi-model `model`-labeled series under the pool, `--otel` OTLP export (OBS24-OBS26), `doctor --bundle` (OBS31) |
| v1.0 "Harbor" | desktop app consumes the same stats structs over the C ABI (no parallel metrics path — invariant O5 holds across the ABI); opt-in crash reporting designed and specced separately |

## 8. Testing strategy

- Unit: rotation boundary conditions (midnight + size trip in the same window, retention deleting oldest-first); request-id header validation; redaction denylist matching including nested URL-shaped values.
- Property/fuzz: OBS11 secret-leak corpus across all sinks; KV12-style invalidation events never emit reversible content; hostile-input scrape corpus for OBS18 label cardinality.
- Integration: scrape `/metrics` during RFC-0007 AC2 (ITL guard) and AC4 (disconnect storm) runs and assert the relevant series move (`drakkar_prefill_chunk_tokens` adapts; `drakkar_kv_pool_blocks` accounting equality after quiesce); `doctor --json` schema-valid across a matrix of induced check failures; `--log-bodies` records marked sensitive and stripped from bundles.
- Golden-fixture: a recorded `/metrics` exposition checked against the catalog in §3 (names, types, label sets, bucket boundaries) so a metric rename is a reviewed schema change, not an accident.
- Soak: the 24 h mixed-load soak (PRD P14) runs with `DEBUG` file logging active; rotation, retention, and RSS-drift bounds must hold with observability on — observability overhead is inside the performance contract, not exempt from it.

## 9. Open questions

- OPEN QUESTION (owner: abdelstark): final histogram bucket boundaries for `drakkar_ttft_seconds` and `drakkar_itl_seconds`. The §3.3 boundaries bracket the RFC-0009 modeled targets; they are locked (and the golden fixture updated) after the first Tier-1 fleet calibration run in v0.2 (`bench` milestone, RFC-0009 PB15), since renaming or re-bucketing after dashboards exist is a breaking change.
