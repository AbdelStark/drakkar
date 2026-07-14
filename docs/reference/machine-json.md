# Machine JSON contract

Every `drakkar` command supports `--json`: it prints **exactly one JSON object**
on stdout (logs and progress go to stderr), and that object's first-class
`schema` field names its contract as `drakkar.<cmd>/<major>`. Streaming commands
support `--stream-json`, one JSON object per line. Agents dispatch on `schema`
alone — you never parse human messages.

## Schema registry

| Schema | Emitted by | Ships |
| --- | --- | --- |
| `drakkar.fit/1` | `drakkar fit --json`, `POST /fit` | v0.1 |
| `drakkar.run/1` | `drakkar run --json` (one-shot result) | v0.1 |
| `drakkar.pull/1` | `drakkar pull --json` | v0.1 |
| `drakkar.ls/1` | `drakkar ls --json` | v0.1 |
| `drakkar.rm/1` | `drakkar rm --json`, `drakkar prune --json` | v0.1 |
| `drakkar.doctor/1` | `drakkar doctor --json` | v0.1 |
| `drakkar.config/1` | `drakkar config get --json` | v0.1 |
| `drakkar.error/1` | error object on any `--json` failure / `--stream-json` `error` event | v0.1 |
| `drakkar.ps/1` | `drakkar ps --json` | v0.2 |
| `drakkar.bench/1` | `drakkar bench --json` | v0.2 |
| `drakkar.convert/1` | `drakkar convert --json` | v0.2 |
| `drakkar.cache/1` | `drakkar cache ls --json` | v0.3 |

The `schema` field is **mandatory** on every machine JSON object, including error
objects and every stream header (API5).

## Compatibility rules

- **Additive-only within a major.** Within `drakkar.<cmd>/1`, new optional fields
  may be added; existing fields are never removed, renamed, retyped, or given a
  new meaning. A breaking change mints `drakkar.<cmd>/2`. Readers **must ignore
  unknown fields** (API11/RV11).
- **Unit suffixes** are part of the field name: `_gib` (GiB, f64), `_kib` (KiB),
  `_mb` (MiB), `_ms` (milliseconds), `_s` (seconds), `_gbs` (GB/s), `_tps`
  (tokens/second). A number without a unit suffix is a count.
- **Confidence** on every prediction: `measured` (on this machine), `calibrated`
  (from this chip's calibration file), or `modeled` (shipped constants).

## `drakkar.fit/1`

The feasibility report, served identically by `drakkar fit --json` and
`POST /fit` — the human report card and the JSON serialize from the same struct,
so they cannot drift.

```jsonc
{
  "schema": "drakkar.fit/1",
  "model":   { "id": "Qwen/Qwen3-8B", "arch": "qwen3",
               "params_total": 8.19e9, "params_active": 8.19e9,
               "quant": { "scheme": "mlx_affine", "bits": 4, "group": 64, "bpw_eff": 4.5 } },
  "machine": { "chip": "Apple M4 Pro", "ram_gib": 48, "budget_gib": 36.0,
               "budget_source": "probe",          // "probe" | "table"
               "bandwidth_gbs": 273, "nax": false, "wired_limit_mb": 0 },
  "memory":  { "weights_gib": 4.21, "kv_per_token_kib": 144, "kv_at_ctx_gib": 4.5,
               "activation_gib": 0.4, "runtime_gib": 1.2, "total_gib": 10.4,
               "confidence": "modeled" },
  "verdict": "comfortable",                        // comfortable | tight | needs_tuning | wont_fit
  "headroom_gib": 25.6,
  "context": { "requested": 32768, "max_fp16": 214000, "max_kv8": 468000,
               "max_kv4": 900000, "advertised": 131072 },
  "performance": { "decode_tps":  { "value": 55,  "confidence": "calibrated" },
                   "ttft_cold_s": { "value": 1.9, "prompt": 4096, "confidence": "modeled" },
                   "load_s": 1.4 },
  "remedies": [ { "rank": 1, "kind": "official_quant",
                  "command": "drakkar pull qwen3:8b --quant 4bit-g64",
                  "effect": "Use an official 4-bit artifact." } ]
}
```

Field notes:

- **`model`** — the reference id, architecture family, total/active parameter
  counts (active < total for MoE), and the quantization descriptor
  (`scheme`, `bits`, `group`, `bpw_eff`). `recipe` appears only when a curated
  per-model recipe applies.
- **`machine`** — chip identity, unified memory, GPU budget and its source
  (`probe` = live Metal probe; `table` = the offline `--machine` fallback),
  bandwidth, Neural-Accelerator availability, and the current wired limit.
- **`memory`** — the six-term decomposition in GiB (per-token KV in KiB) with a
  `confidence` tier. `total_gib` is the master identity
  `weights + kv + activation + runtime + fragmentation`.
- **`verdict`** — one of the four FE19 tiers; `headroom_gib` is the slack under
  the budget.
- **`context`** — the requested context and the maximum admissible context per
  KV precision (`max_kv4` is optional). This is the answer file-size heuristics
  cannot give: *fits at 16k, not at 32k*.
- **`performance`** — decode throughput, cold time-to-first-token (with the
  assumed `prompt` length), and load time; each estimate carries its
  `confidence` tier.
- **`remedies`** — ranked by expected quality impact (FE19), each with a
  copy-pasteable `command` and its predicted `effect`.

## JSON Lines streaming (`--stream-json`)

Streaming commands (`run`, `bench`) emit one JSON object per line on stdout,
discriminated by `event`:

```jsonc
{"schema": "drakkar.run-stream/1", "event": "start", "model": "qwen3:8b", "ctx_max": 32768}
{"event": "token", "text": " fjord", "token_id": 48231}
{"event": "stats", "ttft_ms": 412, "itl_ms_p50": 21.3, "prompt_tokens": 2048, "completion_tokens": 96}
{"event": "done",  "finish_reason": "stop", "usage": {"prompt_tokens": 2048, "cached_prompt_tokens": 1792, "completion_tokens": 128}}
{"event": "error", "code": "engine.inference_failed", "message": "...", "remedy": "...", "exit_code": 6}
```

Rules (API6):

- The **first line** is always a `start` event carrying the `schema` field;
  subsequent lines inherit that schema.
- The `event` set (`start`, `token`, `stats`, `done`, `error`) is **append-only**
  within a schema major; a consumer **must ignore** event types it does not
  recognize.
- **Exactly one terminal event** (`done` or `error`) ends every stream. `error`
  carries the machine error `code` and the process `exit_code`.

## `drakkar.error/1`

Any `--json` failure (and the `--stream-json` `error` event) is a
`drakkar.error/1` object carrying the stable `code`, `category`, `message`,
`remedy`, `retry`, `context`, and `exit_code`. See the
[error-code reference](error-codes.md) for the code registry and the
category → exit-code / HTTP mapping.
