# RFC-0008: CLI and UX Specification

**Status:** Draft
**Author:** A. Bakhta
**Created:** 2026-07-14
**Requires:** RFC-0001, RFC-0004, RFC-0006, RFC-0007

## 1. Summary

The CLI is the whole product in v0.x, and it has two users of equal weight: a human at a terminal and an agent driving a subprocess. Every command therefore renders to both a clean human view and a stable `--json` view from the same data, and every command returns a deterministic exit code. This RFC fixes the command surface, the one-command run experience, the JSON/exit-code contract, and configuration.

## 2. Command surface

| Command | Purpose |
| --- | --- |
| `drakkar run <ref> [prompt]` | Fit-check, acquire if needed, load, then interactive REPL or one-shot if `prompt`/stdin given |
| `drakkar serve [<ref>]` | Start the HTTP server (RFC-0007); optionally preload a model; foreground or `--daemon` |
| `drakkar fit <ref>` | Feasibility report only, no download (RFC-0004 FE25) |
| `drakkar pull <ref>` | Acquire + prepare (convert/quantize) without running |
| `drakkar ls` | Installed models: size, format, quant, last-used |
| `drakkar ps` | Running models: residency, pool occupancy, hit rate, throughput |
| `drakkar rm <ref>` / `prune` | Remove a model / GC unreferenced blobs (reports reclaimable first) |
| `drakkar convert <ref> --bits B` | On-device quantization to the store (RFC-0006 §6) |
| `drakkar bench <ref>` | Benchmark + optional `--calibrate` (RFC-0009) |
| `drakkar cache ls|clear` | Manage the SSD KV tier (RFC-0005 §6) |
| `drakkar doctor` | Environment report: chip, budget, wired limit, macOS, NAX status, disk, config sanity |
| `drakkar config get|set|path` | Read/write config |
| `drakkar completions <shell>` | Shell completions |

## 3. The one-command run experience

`drakkar run qwen3:8b` on a cold machine:

1. Resolve reference and fetch metadata only (RFC-0006 MP2).
2. Print a **fit report card** (RFC-0004): verdict, memory breakdown, chosen artifact and quant with provenance, max context at the default KV precision, estimated cold TTFT and decode speed with confidence tiers.
3. If not Comfortable, show the ranked remedy and the exact command/flag for each; if Won't fit, name the nearest sibling that fits and stop (exit 4) unless `--force`.
4. On proceed (auto if Comfortable and stdout is a TTY; `--yes` for scripts): download with progress (bytes, rate, ETA, and the next step previewed), convert/quantize if needed, load (SSD-bound, progress shown).
5. Enter the REPL, or run the one-shot prompt and exit. First line of the REPL states model, quant, context ceiling, and the server-free local mode.

- CLI1. The report card is shown before any multi-gigabyte action, always. A user never discovers infeasibility after a download.
- CLI2. Everything after resolution is skippable via flags for automation; nothing requires an interactive terminal.

## 4. Interactive REPL

- CLI3. Streaming output; `Ctrl-C` cancels the current generation (one decode step latency) without exiting; `Ctrl-D` exits.
- CLI4. Meta-commands: `/model <ref>` hot-swap, `/context` show token usage vs ceiling, `/stats` last-turn TTFT/ITL/tokens, `/system <text>` set system prompt, `/save` and `/load` a conversation, `/fit` re-show the report, `/tools <file>` load tool definitions for a local tool-call test loop.
- CLI5. Multi-line input via bracketed paste or a `"""` fence; history persisted per model under `~/.drakkar/`.

## 5. Agent contract: JSON and exit codes

- CLI6. Every command accepts `--json`; output is a single JSON object (or JSON Lines for streaming subcommands) on stdout, logs and progress on stderr, so stdout is always machine-parseable. Schemas are versioned (`drakkar.<cmd>/1`) and additive-only within a major version.
- CLI7. Streaming commands (`run`, `bench`) support `--stream-json` emitting JSON Lines events (`token`, `stats`, `done`, `error`) for programmatic consumption.
- CLI8. Deterministic exit codes:

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 2 | Usage error (bad flags/args) |
| 3 | Model or reference not found |
| 4 | Won't fit (feasibility failure without `--force`) |
| 5 | Download/network failure |
| 6 | Engine/runtime failure (load, Metal, inference) |
| 7 | Disk/space failure |

- CLI9. `--quiet` suppresses all non-error stderr; `--verbose`/`-v` (repeatable) raises log level; `NO_COLOR` and non-TTY detection disable ANSI automatically.

## 6. Configuration

- CLI10. `~/.config/drakkar/config.toml`, overlaid by env (`DRAKKAR_*`) and per-invocation flags (flags > env > file > defaults). Keys include: `server.host/port/api_key`, `models.default`, `storage.path/import_hf_cache`, `kv_cache.disk/bits/disk_budget_gib`, `runtime.keep_alive`, `scheduler.max_concurrency`, `telemetry = off`.
- CLI11. `drakkar config set` validates types and ranges and writes atomically; `drakkar doctor` flags stale or invalid keys.

## 7. Daemon and lifecycle

- CLI12. `drakkar serve --daemon` installs/uses a launchd agent (`~/Library/LaunchAgents`); `drakkar serve --stop|--status|--logs` manage it; logs go to `~/.drakkar/logs` with rotation.
- CLI13. A background daemon and a foreground `run` share the model store and (optionally) the SSD KV tier but hold independent memory contracts (RFC-0001 A5 applies only within one process).

## 8. First-run and errors

- CLI14. First invocation prints a one-screen orientation (store location, privacy stance: no telemetry, how to get help) once, then never again (state flag in the store).
- CLI15. Errors follow a fixed shape: what failed, why in domain terms, and the single most useful next action (a command or flag). Network, space, fit, and format errors each have templated remedies. Panics are caught at the top level and rendered as exit-6 with a bug-report hint; stack traces only under `--verbose`.
- CLI16. No telemetry, ever, without explicit opt-in; `doctor --check-update` is the only network call not initiated by an explicit model/serve action, and it is on-demand.

## 9. Acceptance criteria

- AC1. `run`, `fit`, `pull`, `ls`, `ps`, `rm`, `bench`, `doctor` each produce schema-valid `--json` and the documented exit codes across success and each failure class (integration matrix).
- AC2. Piping works: `drakkar fit <ref> --json | jq .verdict` yields the verdict with nothing else on stdout.
- AC3. Cold `run` on a Comfortable model reaches the REPL with the report card shown first, in one command, no prompts when `--yes`.
- AC4. `NO_COLOR=1` and non-TTY stdout both yield plain output; TTY yields colored progress.
- AC5. Every error path prints a remedy line; no raw Rust panic reaches the user without the exit-6 wrapper.

## Open questions

1. Ship a `drakkar` TUI (multi-pane: models, requests, metrics) in v0.3, or keep `ps`/`bench` line-oriented until the desktop app? (Leaning line-oriented.)
2. Alias namespace collisions between shipped aliases and user aliases: user always wins, or require a `my/` prefix for user aliases? (Leaning user-wins with a warning.)

## References

- clap (Rust CLI), NO_COLOR convention, launchd/LaunchAgents documentation
- Ollama and mlx-serve CLI surfaces (`run`/`ps`/`pull`/name:tag) as prior art; Docker `vllm` CLI (`serve`/`chat`/`bench`) for the serve/bench split
- OpenAI/Anthropic client expectations for localhost base URLs (RFC-0007)
