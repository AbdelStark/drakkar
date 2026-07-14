# CLI reference

> Generated from the `drakkar-cli` command tree. Do not edit by hand;
> regenerate with `UPDATE_DOCS=1 cargo test -p drakkar-cli --test cli_reference`.

A native LLM inference engine for Apple Silicon.

## Global flags

Accepted on every command; stdout carries machine output only under `--json`, logs and progress always go to stderr.

| Flag | Description |
| --- | --- |
| `--json` | Emit a single machine-readable JSON object on stdout |
| `--stream-json` | Emit JSON Lines streaming events (streaming commands only) |
| `--quiet`, `-q` | Suppress non-error progress on stderr |
| `--verbose`, `-v` | Raise the stderr log level; repeat for more detail (`-vv`) |
| `--yes`, `-y` | Assume "yes" to interactive confirmations |
| `--force` | Override a `Won't fit` verdict and proceed (exit-4 path) |

`NO_COLOR` (or a non-TTY stdout) disables ANSI. `DRAKKAR_*` environment variables override config file values; precedence is flags > `DRAKKAR_*` env > `~/.config/drakkar/config.toml` > built-in defaults (LD23).

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 2 | Usage error (bad flags/args) |
| 3 | Model or reference not found |
| 4 | Won't fit (feasibility failure without `--force`) |
| 5 | Download/network failure |
| 6 | Engine/runtime failure (load, Metal, inference); also the panic wrapper |
| 7 | Disk/space failure |

Code 1 is never emitted intentionally. See the [error-code reference](error-codes.md) for the per-code mapping.

## Commands

### `drakkar run <reference> [prompt]`

Fit-check, acquire, load, then REPL or one-shot generate

- Milestone: v0.1 · Stability: stable

| Argument / flag | Description |
| --- | --- |
| `<REFERENCE>` | The model reference (e.g. `qwen3:8b`, `org/repo`) |
| `<PROMPT>` | A one-shot prompt; omit for an interactive REPL |

### `drakkar pull <reference>`

Acquire and prepare a model without running it

- Milestone: v0.1 · Stability: stable

| Argument / flag | Description |
| --- | --- |
| `<REFERENCE>` | The model reference |

### `drakkar fit <reference> [OPTIONS]`

Print a feasibility report without downloading (FE25)

- Milestone: v0.1 · Stability: stable

| Argument / flag | Description |
| --- | --- |
| `<REFERENCE>` | The model reference |
| `--ctx` | Target context length |
| `--kv-bits` | KV precision in bits (16, 8, or 4) |
| `--concurrency` | Concurrency to plan for |
| `--machine` | Simulate a machine profile instead of probing |

### `drakkar ls`

List installed models

- Milestone: v0.1 · Stability: stable

### `drakkar rm <reference>`

Remove a model

- Milestone: v0.1 · Stability: stable

| Argument / flag | Description |
| --- | --- |
| `<REFERENCE>` | The model reference |

### `drakkar prune`

Garbage-collect blobs unreferenced by any manifest

- Milestone: v0.1 · Stability: stable

### `drakkar doctor [OPTIONS]`

Report the environment, GPU, and configuration

- Milestone: v0.1 · Stability: stable

| Argument / flag | Description |
| --- | --- |
| `--check-update` | Check for a newer DRAKKAR release (explicit, on-demand) |

### `drakkar serve [reference]`

Run the HTTP server in the foreground

- Milestone: v0.1 · Stability: stable

| Argument / flag | Description |
| --- | --- |
| `<REFERENCE>` | The model to load on start; omit to load on first request |

### `drakkar config <SUBCOMMAND>`

Read or write configuration (CLI10–CLI11)

- Milestone: v0.1 · Stability: stable

#### `drakkar config get <key>`

Print a config value

- Milestone: v0.1 · Stability: stable

| Argument / flag | Description |
| --- | --- |
| `<KEY>` | The dotted config key, e.g. `server.port` |

#### `drakkar config set <key> <value>`

Set a config value (validated, atomic write)

- Milestone: v0.1 · Stability: stable

| Argument / flag | Description |
| --- | --- |
| `<KEY>` | The dotted config key |
| `<VALUE>` | The new value |

#### `drakkar config path`

Print the config file path

- Milestone: v0.1 · Stability: stable

### `drakkar ps`

[v0.2] Show resident models and pool occupancy

- Milestone: v0.2 · Stability: stable

### `drakkar bench <reference> [OPTIONS]`

[v0.2] Benchmark a model, optionally writing calibration

- Milestone: v0.2 · Stability: experimental → stable v0.3

| Argument / flag | Description |
| --- | --- |
| `<REFERENCE>` | The model reference |
| `--calibrate` | Write a per-chip calibration store |

### `drakkar convert <reference> [OPTIONS]`

[v0.2] Quantize a model on device to the store

- Milestone: v0.2 · Stability: experimental → stable v0.3

| Argument / flag | Description |
| --- | --- |
| `<REFERENCE>` | The model reference |
| `--bits` | Target bit width |

