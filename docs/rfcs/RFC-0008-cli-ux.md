# RFC-0008: CLI and UX Specification

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.1 (daemon lifecycle: v0.3)

## Summary

The CLI is the whole product in v0.x, and it has two users of equal weight: a human at a
terminal and an agent driving a subprocess. Every command therefore renders to both a clean
human view and a stable `--json` view from the same data, and every command returns a
deterministic exit code. This RFC fixes the command surface, the one-command run experience,
the interactive REPL, the JSON/exit-code agent contract, configuration, and daemon lifecycle.

The core invariants, named for reference throughout this document:

- **UX-I1 (report-before-bytes):** no multi-gigabyte action starts before the fit report card
  is shown (CLI1).
- **UX-I2 (stdout is machine-owned):** under `--json`/`--stream-json`, stdout carries exactly
  one JSON document or a JSON Lines stream; all logs and progress go to stderr (CLI6).
- **UX-I3 (dual rendering from one struct):** the human view and the JSON view of every
  command serialize from the same Rust struct; there is no separate human-only data path.
- **UX-I4 (exit codes are API):** the exit-code table is a versioned contract; codes never
  change meaning within a major version (CLI8).

## Motivation

The PRD makes the CLI the delivery vehicle for the product's two headline promises:

- [PRD G1](../../PRD.md#goals-v10-horizon): one-command run — `drakkar run <hf-link-or-alias>`
  from cold start to interactive chat, with a feasibility preflight before any download. That
  entire flow is specified here (section
  [The one-command run experience](#2-the-one-command-run-experience)).
- [PRD P7](../../PRD.md#51-functional): every command MUST offer `--json` machine-readable
  output with a stable schema and deterministic exit codes. That is the agent contract
  (section [Agent contract](#4-agent-contract-json-and-exit-codes)).

Two of the PRD's target users drive the design directly. The **agent builder**
([PRD §3, user 1](../../PRD.md#3-target-users)) runs coding agents and MCP tool servers that
shell out to `drakkar` as a subprocess: they need parseable stdout, JSON Lines streaming, and
exit codes they can branch on without scraping human prose. The **AI engineer / power user**
([PRD §3, user 2](../../PRD.md#3-target-users)) evaluates open-weight releases weekly and needs
`drakkar fit <ref> --json | jq .verdict` to work in a shell script the first time. The PRD's
differentiation table ([PRD §6](../../PRD.md#6-differentiation-summary)) lists "Agent JSON
contract on every CLI command" as a capability no incumbent ships completely; this RFC is
where that row is made true.

## Goals

- Every command in the surface table produces both a human rendering and a schema-versioned
  `--json` rendering from the same struct (UX-I3), verifiable by the schema harness in
  [Testing Strategy](#testing-strategy).
- `drakkar run <ref>` on a cold machine reaches an interactive REPL or a completed one-shot in
  one command with no other tool involved, showing the fit report card before any download
  (CLI1, PRD G1).
- Deterministic, documented exit codes covering every failure class, stable within a major
  version (CLI8, UX-I4).
- Full non-interactive operation: every interactive step has a flag equivalent (`--yes`,
  `--force`, stdin one-shot), so a CI job or agent never blocks on a prompt (CLI2).
- Configuration precedence that is total, documented, and property-tested:
  flags > env (`DRAKKAR_*`) > `~/.config/drakkar/config.toml` > built-in defaults (CLI10, LD23).
- Errors that always carry a remedy: what failed, why in domain terms, and the single most
  useful next action (CLI15).
- Zero telemetry without explicit opt-in; exactly one on-demand network call
  (`doctor --check-update`) outside model/serve actions (CLI16, PRD P13).

## Non-Goals

- **No TUI in v0.x.** No multi-pane terminal UI for models/requests/metrics; `ps` and `bench`
  stay line-oriented until the v1.0 desktop app (LD15, resolved from this RFC's original open
  question 1). Rationale: a TUI is a second rendering surface with its own bug class, and the
  desktop app (v1.0 "Harbor") covers the visual-monitoring need on the same engine C ABI.
- **No GUI.** The menu-bar desktop app is a separate v1.0 deliverable built over the engine's
  C ABI ([RFC-0001](RFC-0001-architecture.md#proposed-design)); nothing in this RFC ships
  graphical surfaces.
- **No interactive-only features.** Any capability reachable only through the REPL and not
  through flags/stdin/serve is out of scope by construction (CLI2).
- **No localization** of CLI output in v0.x; output is English, but the JSON contract is
  locale-independent by design so localization later cannot break agents.
- **No plugin or extension system** for the CLI in v0.x; the surface is closed and versioned.

## Proposed Design

### 1. Command surface

| Command | Purpose | Milestone |
| --- | --- | --- |
| `drakkar run <ref> [prompt]` | Fit-check, acquire if needed, load, then interactive REPL or one-shot if `prompt`/stdin given | v0.1 |
| `drakkar serve [<ref>]` | Start the HTTP server ([RFC-0007](RFC-0007-api-server.md#proposed-design)); optionally preload a model; foreground or `--daemon` | v0.1 (foreground); `--daemon` v0.3 |
| `drakkar fit <ref>` | Feasibility report only, no download (RFC-0004 FE25, [Feasibility Engine](RFC-0004-feasibility-engine.md#proposed-design)) | v0.1 |
| `drakkar pull <ref>` | Acquire + prepare (convert/quantize) without running | v0.1 |
| `drakkar ls` | Installed models: size, format, quant, last-used | v0.1 |
| `drakkar ps` | Running models: residency, pool occupancy, hit rate, throughput | v0.1 (single model); pool metrics v0.3 |
| `drakkar rm <ref>` / `prune` | Remove a model / GC unreferenced blobs (reports reclaimable first) | v0.1 |
| `drakkar convert <ref> --bits B` | On-device quantization to the store ([RFC-0006 MP12–MP14](RFC-0006-model-pipeline.md#proposed-design)) | v0.2 |
| `drakkar bench <ref>` | Benchmark + optional `--calibrate` (RFC-0009 PB8, [Performance](RFC-0009-performance.md#proposed-design)) | v0.2 |
| `drakkar cache ls\|clear` | Manage the SSD KV tier (RFC-0005 KV19, [KV Cache](RFC-0005-kv-cache.md#proposed-design)) | v0.3 |
| `drakkar doctor` | Environment report: chip, budget, wired limit, macOS, NAX status, disk, config sanity | v0.1 |
| `drakkar config get\|set\|path` | Read/write config | v0.1 |
| `drakkar alias ls\|update` | List the shipped+user alias table; explicitly refresh the shipped set ([RFC-0006](RFC-0006-model-pipeline.md#proposed-design), LD3) | v0.1 (`ls`); `update` v0.2 |
| `drakkar completions <shell>` | Shell completions (bash, zsh, fish) | v0.1 |

Model references accepted everywhere a `<ref>` appears follow RFC-0006 MP1: full HF URLs,
`org/repo[@revision]`, curated aliases (`qwen3:8b`), and local paths. **Alias collisions**
resolve user-defined-wins: when a user-defined alias shadows a shipped alias, the user's
definition is used and a one-line warning naming both targets is printed to stderr (LD16,
resolved from this RFC's original open question 2). The warning is suppressible with
`--quiet` and never appears on stdout.

Argument parsing, help text, and completions generate from one declarative command
definition, so `--help`, the completions, and this table cannot drift independently.

External dependencies for the CLI layer (crate `drakkar-cli`, LD24):

| Dependency | Constraint | Reason |
| --- | --- | --- |
| `clap` | `>=4.5, <5` | Declarative arg parsing; derive API keeps command definitions adjacent to the structs they populate (UX-I3) |
| `clap_complete` | matching `clap` | Generates bash/zsh/fish completions from the same definition |
| `rustyline` | `>=14, <15` | REPL line editing, history, bracketed-paste detection (CLI5) |
| `serde` / `serde_json` | `>=1, <2` | Single-struct dual rendering; JSON and JSON Lines emission (CLI6, CLI7) |
| `toml` | `>=0.8, <1` | Config file parse/serialize with span-preserving errors for `doctor` diagnostics (CLI11) |

### 2. The one-command run experience

`drakkar run qwen3:8b` on a cold machine:

1. Resolve reference and fetch metadata only (RFC-0006 MP2): model card header,
   `config.json`, file listing, safetensors index — no weight bytes.
2. Print a **fit report card** ([RFC-0004](RFC-0004-feasibility-engine.md#proposed-design)):
   verdict, memory breakdown, chosen artifact and quant with provenance, max context at the
   default KV precision, estimated cold TTFT and decode speed with confidence tiers.
3. If not Comfortable, show the ranked remedy list and the exact command/flag for each; if
   Won't fit, name the nearest sibling that fits and stop (exit 4) unless `--force`.
4. On proceed (auto if Comfortable and stdout is a TTY; `--yes` for scripts): download with
   progress (bytes, rate, ETA, and the next step previewed), convert/quantize if needed, load
   (SSD-bound, progress shown).
5. Enter the REPL, or run the one-shot prompt and exit. First line of the REPL states model,
   quant, context ceiling, and the server-free local mode.

- CLI1. The report card is shown before any multi-gigabyte action, always. A user never
  discovers infeasibility after a download. (Invariant UX-I1.)
- CLI2. Everything after resolution is skippable via flags for automation; nothing requires
  an interactive terminal.

`run` hosts the engine **in-process** (RFC-0001 A1): no server, no daemon, no port is
required for local generation. A one-shot prompt may arrive as an argument or on stdin
(`echo "prompt" | drakkar run qwen3:8b`); when stdin is not a TTY and carries data, `run`
treats it as a one-shot and exits after the completion, honoring `--json`/`--stream-json`.

### 3. Interactive REPL

- CLI3. Streaming output; `Ctrl-C` cancels the current generation (one decode step latency,
  RFC-0003 IC-level cancellation) without exiting; `Ctrl-D` exits.
- CLI4. Meta-commands: `/model <ref>` hot-swap, `/context` show token usage vs ceiling,
  `/stats` last-turn TTFT/ITL/tokens, `/system <text>` set system prompt, `/save` and `/load`
  a conversation, `/fit` re-show the report, `/tools <file>` load tool definitions for a
  local tool-call test loop.
- CLI5. Multi-line input via bracketed paste or a `"""` fence; history persisted per model
  under `~/.drakkar/`.

The CLI4 list is exhaustive for v0.x: the REPL is a thin loop over the same generation path
the server uses, and new meta-commands require amending CLI4 in this RFC first. `/save` and
`/load` use a plain JSON conversation file (`role`/`content` message array plus the system
prompt) so saved conversations are scriptable inputs, not an opaque format.

### 4. Agent contract: JSON and exit codes

- CLI6. Every command accepts `--json`; output is a single JSON object (or JSON Lines for
  streaming subcommands) on stdout, logs and progress on stderr, so stdout is always
  machine-parseable (UX-I2). Schemas are versioned (`drakkar.<cmd>/1`) and **additive-only
  within a major version**: fields may be added, never removed, renamed, or retyped; a
  removal or retype requires bumping to `drakkar.<cmd>/2` and is a breaking release event.
- CLI7. Streaming commands (`run`, `bench`) support `--stream-json` emitting JSON Lines
  events (`token`, `stats`, `done`, `error`) for programmatic consumption.
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

Exit code 1 is reserved (conventional shell "generic failure") and is never intentionally
emitted; observing it indicates a bug. Codes map one-to-one onto the top-level error classes
of the error taxonomy ([RFC-0011](RFC-0011-error-taxonomy.md#proposed-design)), so a
structured error object and the process exit code can never disagree.

- CLI9. `--quiet` suppresses all non-error stderr; `--verbose`/`-v` (repeatable) raises log
  level; `NO_COLOR` and non-TTY detection disable ANSI automatically.

Every `--json` object carries its schema identifier as the first field:

```json
{
  "schema": "drakkar.fit/1",
  "verdict": "comfortable",
  "memory": { "weights_gib": 4.3, "kv_gib": 1.6, "overhead_gib": 0.9, "budget_gib": 21.3 },
  "max_context": 32768,
  "estimates": { "ttft_ms": 780, "decode_tps": 62.0, "confidence": "modeled" }
}
```

(Field detail beyond `schema` is normative in the owning RFC — here RFC-0004 FE25; this RFC
owns the envelope rule: `schema` first, one object, stdout only.)

`--stream-json` events are one JSON object per line, discriminated by `event`:

```json
{"event":"token","schema":"drakkar.run.stream/1","text":" fjord","index":41}
{"event":"stats","schema":"drakkar.run.stream/1","ttft_ms":812,"itl_ms_p50":16.2,"tokens":42}
{"event":"done","schema":"drakkar.run.stream/1","finish_reason":"stop","usage":{"prompt_tokens":128,"completion_tokens":42}}
{"event":"error","schema":"drakkar.run.stream/1","code":"engine_failure","exit_code":6,"message":"Metal device lost during decode","remedy":"drakkar doctor"}
```

An `error` event is always the final line when emitted, and the process exit code equals its
`exit_code` field. Committed JSON Schema files, one per `drakkar.<cmd>/<major>`, live in the
repo under `crates/drakkar-cli/schemas/` and gate CI (see
[Testing Strategy](#testing-strategy)).

### 5. Configuration

- CLI10. `~/.config/drakkar/config.toml`, overlaid by env (`DRAKKAR_*`) and per-invocation
  flags (flags > env > file > defaults; LD23). Keys include: `server.host/port/api_key`,
  `models.default`, `storage.path/import_hf_cache`, `kv_cache.disk/bits/disk_budget_gib`,
  `runtime.keep_alive`, `scheduler.max_concurrency`, `telemetry = off`.
- CLI11. `drakkar config set` validates types and ranges and writes atomically (temp file +
  rename in the config directory); `drakkar doctor` flags stale or invalid keys.

The full key table with types and defaults (defaults are normative in the owning RFCs; listed
here for the config contract):

| Key | Type | Default | Owner |
| --- | --- | --- | --- |
| `server.host` | string | `"127.0.0.1"` | RFC-0007 AS18 (LD22) |
| `server.port` | u16 | `11711` | RFC-0007 AS18 (LD22) |
| `server.api_key` | string, optional | unset (required if host is non-loopback) | RFC-0007 AS18 |
| `models.default` | string, optional | unset | this RFC |
| `storage.path` | path | `~/.drakkar/models` | RFC-0006 (LD14: custom/external volume from v0.1) |
| `storage.import_hf_cache` | string enum `clone\|off` | `"clone"` (read-only APFS clonefile/hard-link, never mutates the HF cache) | RFC-0006 (LD4) |
| `kv_cache.disk` | bool | `true` for `serve`, `false` for one-shot `run` | RFC-0005 KV17 |
| `kv_cache.bits` | enum `16\|8\|4` | `16` (fp16) | RFC-0005 KV13 |
| `kv_cache.disk_budget_gib` | u32 | `8` | RFC-0005 KV19 |
| `runtime.keep_alive` | duration string | `"30m"` for `serve`, `"0s"` for one-shot | RFC-0007 AS17 |
| `scheduler.max_concurrency` | u32 | `8` | RFC-0007 AS14 |
| `telemetry` | enum `off` | `off` (only value in v0.x; CLI16) | this RFC |

Environment variable mapping is mechanical: dotted key → upper snake with `DRAKKAR_` prefix
(`server.port` → `DRAKKAR_SERVER_PORT`, `kv_cache.disk_budget_gib` →
`DRAKKAR_KV_CACHE_DISK_BUDGET_GIB`). Unknown `DRAKKAR_*` variables and unknown config keys
produce a `doctor` warning, never a silent ignore. State (model store, KV disk tier, logs,
REPL history, first-run flag) lives under `~/.drakkar/`; configuration lives under
`~/.config/drakkar/` — config is user-editable intent, state is engine-owned data, and the
split means backing up or deleting one never touches the other.

Example config file:

```toml
# ~/.config/drakkar/config.toml
[server]
host = "127.0.0.1"
port = 11711

[models]
default = "qwen3:8b"

[storage]
path = "/Volumes/External/drakkar"
import_hf_cache = "clone"

[kv_cache]
disk = true
disk_budget_gib = 16

[runtime]
keep_alive = "30m"

telemetry = "off"
```

### 6. Daemon and lifecycle

- CLI12. `drakkar serve --daemon` installs/uses a launchd agent (`~/Library/LaunchAgents`);
  `drakkar serve --stop|--status|--logs` manage it; logs go to `~/.drakkar/logs` with
  rotation.
- CLI13. A background daemon and a foreground `run` share the model store and (optionally)
  the SSD KV tier but hold independent memory contracts (RFC-0001 A5 applies only within one
  process).

Daemon lifecycle is a v0.3 deliverable (LD25, "Fleet"); in v0.1–v0.2 `serve` is
foreground-only and `--daemon` exits 2 (usage error) with a message naming the milestone.
`serve --status --json` reports `{running, pid, port, models_resident, uptime_s}` under
schema `drakkar.serve.status/1`. The launchd plist is generated, versioned, and owned by
DRAKKAR: `--stop` unloads it, and `rm`-ing the binary plus `drakkar serve --uninstall-daemon`
leaves no LaunchAgents residue.

### 7. First-run and errors

- CLI14. First invocation prints a one-screen orientation (store location, privacy stance: no
  telemetry, how to get help) once, then never again (state flag in the store).
- CLI15. Errors follow a fixed shape: what failed, why in domain terms, and the single most
  useful next action (a command or flag). Network, space, fit, and format errors each have
  templated remedies. Panics are caught at the top level and rendered as exit-6 with a
  bug-report hint; stack traces only under `--verbose`.
- CLI16. No telemetry, ever, without explicit opt-in; `doctor --check-update` is the only
  network call not initiated by an explicit model/serve action, and it is on-demand.

The error shape is a struct, not a convention, shared with the error taxonomy
([RFC-0011](RFC-0011-error-taxonomy.md#proposed-design)):

```json
{
  "schema": "drakkar.error/1",
  "code": "wont_fit",
  "exit_code": 4,
  "what": "Qwen3-32B at 4-bit needs 21.4 GiB; your GPU budget is 16.0 GiB",
  "why": "weights + KV at the requested 8192-token context exceed the wired-memory budget",
  "remedy": "drakkar run qwen3:14b   # nearest sibling that fits Comfortable"
}
```

The human rendering is the same struct through the human formatter: a `what` line, a `why`
line, and a `remedy` line prefixed `try:` — never a bare error string, never a raw backtrace
(CLI15). The first-run orientation (CLI14) is suppressed entirely when stdout is not a TTY or
`--json` is set, so an agent's very first invocation is already clean.

## Alternatives Considered

**A subcommand-less single REPL binary.** Some chat-oriented CLIs ship one binary that drops
straight into a conversational loop, with everything else (model management, serving) as
in-loop commands. Rejected: the agent contract requires addressable verbs. An agent cannot
script "remove this model" or "give me a fit verdict" against a REPL without fragile
expect-style driving; `fit`, `pull`, `ls`, `rm` as first-class subcommands with `--json` and
exit codes are the contract (PRD P7). The REPL remains — as the interactive tail of `run`,
not the product's entry point.

**Human output first, parse it later.** The incumbent pattern: ship human-formatted output,
let scripts scrape it, maybe bolt on JSON per-command afterward. Rejected: scraped output is
an accidental API that breaks on every copy edit. CLI6's rule — `--json` rendered from the
same struct as the human view (UX-I3), schema-versioned, stdout machine-owned — makes the
machine view the contract and the human view a formatter over it. The differentiation table
(PRD §6) shows every incumbent with "Partial" or "No" on this row precisely because they
chose the other order.

**Implicit server dependency for `run`.** The dominant prior art starts a background server
on first use and makes the CLI a thin client, so `run` silently depends on a daemon, a port,
and an installed service. Rejected: `run` hosts the engine in-process (RFC-0001 A1) —
serverless local mode. A one-shot generation must work with no port bound, no LaunchAgent
installed, no state beyond the model store; this is simpler to reason about, removes a whole
failure class (daemon version skew, port conflicts) from the v0.1 surface, and keeps the
daemon an explicit v0.3 opt-in (CLI12) rather than a hidden prerequisite.

**YAML or JSON for the config file.** JSON rejected: no comments, and a hand-edited config
without comments cannot document itself. YAML rejected: indentation-significant parsing and
implicit typing (`off` → boolean) are a support-burden generator for a file users edit by
hand. TOML chosen: comments, explicit types, unambiguous parsing, and it is the Rust
ecosystem's configuration convention, so contributors and the `toml` crate's span-preserving
error reporting (used by `doctor` and `config set` validation, CLI11) come for free.

## Drawbacks

- **Dual rendering costs discipline on every surface.** Every new field in every command
  output must land in the struct, the human formatter, the JSON schema file, and the schema
  harness. UX-I3 makes drift structurally hard but not free: reviewers must reject any patch
  that prints to stdout outside the formatter path, forever.
- **REPL features invite scope creep.** A REPL is where "just one more slash command" goes to
  live. The bound is CLI4: the meta-command list is closed, and extending it requires
  amending this RFC. Without that rule the REPL becomes a second product with an untested
  surface.
- **Exit-code stability constrains refactors.** UX-I4 means an internal error-handling
  refactor that reclassifies a failure (say, a gated-repo auth error moving between "not
  found" and "network") is a breaking change for agents branching on `$?`. The mapping to
  RFC-0011's taxonomy must therefore be settled early and treated as API, which front-loads
  taxonomy work into v0.1.
- **Additive-only schemas accumulate cruft.** A misnamed field shipped in `drakkar.fit/1`
  stays until a major version bump. The mitigation is the schema-review gate in CI, not any
  ability to fix mistakes quietly.

## Migration / Rollout

Per the roadmap milestones (LD25, [PRD §8](../../PRD.md#8-roadmap)):

- **v0.1 "First light":** `run`, `pull`, `ls`, `ps` (single-model), `rm`/`prune`, `fit`,
  `doctor`, `config`, `alias ls`, `completions`, foreground `serve`; REPL v1 (CLI3–CLI5
  complete); the full JSON/exit-code contract (CLI6–CLI9) on every shipped command; config
  precedence chain (CLI10–CLI11); first-run and error shape (CLI14–CLI16). `--daemon`,
  `bench`, `convert`, `cache` exit 2 with a milestone-naming message.
- **v0.2 "Convoy":** `bench` (+ `--calibrate`, RFC-0009 PB8), `convert`, `alias update`
  (LD3); `serve` flag growth tracking RFC-0007's v0.2 surface (Anthropic dialect, tool
  calling, structured output) — new flags, no changed defaults.
- **v0.3 "Fleet":** daemon lifecycle via launchd (CLI12–CLI13), `cache ls|clear` over the SSD
  KV tier (RFC-0005 KV19), `ps` enhancements (multi-model pool occupancy, hit rate, per-model
  residency).
- **v1.0 "Harbor":** no CLI surface growth planned; the desktop app arrives beside, not
  inside, the CLI (LD15).

**Schema evolution rule (CLI6, restated as the rollout contract):** within `drakkar.<cmd>/1`,
changes are additive-only — new optional fields, new enum values only in fields documented as
open enums. Removal, rename, retype, or semantic change of an existing field requires
`drakkar.<cmd>/2`, is release-noted as breaking, and both the old and new schema files stay
committed. Exit codes follow the same rule: new codes may be added; existing codes never
change meaning within a major version. There is no feature-flag mechanism in the CLI itself;
milestone-gated commands are present-but-refusing (exit 2) so that `--help` and completions
are honest about the full surface from v0.1.

## Testing Strategy

Acceptance criteria (from the source RFC, kept verbatim as the release gate):

- AC1. `run`, `fit`, `pull`, `ls`, `ps`, `rm`, `bench`, `doctor` each produce schema-valid
  `--json` and the documented exit codes across success and each failure class (integration
  matrix).
- AC2. Piping works: `drakkar fit <ref> --json | jq .verdict` yields the verdict with nothing
  else on stdout.
- AC3. Cold `run` on a Comfortable model reaches the REPL with the report card shown first,
  in one command, no prompts when `--yes`.
- AC4. `NO_COLOR=1` and non-TTY stdout both yield plain output; TTY yields colored progress.
- AC5. Every error path prints a remedy line; no raw Rust panic reaches the user without the
  exit-6 wrapper.

Named test suites implementing and extending them:

- **T1 — Schema-validation harness (gates AC1, AC2).** Every command's `--json` and
  `--stream-json` output is validated in CI against the committed JSON Schema files under
  `crates/drakkar-cli/schemas/`, for success and for each error class. The harness also
  diffs generated schemas against committed ones, so an output-struct change without a
  schema-file change fails the build (enforces UX-I3 and the additive-only rule).
- **T2 — Exit-code matrix (gates AC1).** Integration tests force each failure class per
  command — unknown flag (2), unresolvable ref (3), oversized model on a constrained machine
  profile (4), fault-injected download (5), fault-injected engine load (6), full-disk
  simulation via a small loopback volume (7) — and assert both the exit code and that the
  final structured error's `exit_code` field matches (UX-I4).
- **T3 — Rendering snapshot tests (gates AC4).** Golden snapshots of human output under three
  environments: TTY (via a PTY harness), non-TTY pipe, and `NO_COLOR=1` TTY. Asserts ANSI
  present only in the first, byte-identical plain output in the other two, and progress on
  stderr never stdout.
- **T4 — REPL script tests (gates AC3, CLI3–CLI5).** Expect-style scripted sessions over a
  PTY: every CLI4 meta-command exercised; `Ctrl-C` mid-generation cancels within one decode
  step and returns to the prompt; `Ctrl-D` exits 0; bracketed paste and `"""` fences produce
  one multi-line turn; history file appears under `~/.drakkar/` (redirected to a temp store).
- **T5 — Config precedence property test.** Property-based generation of random
  (default, file, env, flag) value assignments per config key, asserting the effective value
  always equals the highest-precedence source present (flags > env > file > defaults) and
  that `config get` reports both the value and its source layer. Includes atomic-write
  crash-injection: a killed `config set` never leaves a corrupt or partial file.
- **T6 — Stdin one-shot piping (gates AC2, CLI2).** `echo "prompt" | drakkar run <ref>
  --json` completes non-interactively, emits exactly one JSON document on stdout, and never
  writes a prompt or the first-run banner; the same with `--stream-json` yields parseable
  JSON Lines terminating in a `done` event.
- **T7 — Error remedy audit (gates AC5).** A table-driven test iterating every error
  constructor in the taxonomy mapping asserts a non-empty `remedy`; a top-level
  panic-injection test asserts exit 6, the bug-report hint, and no backtrace without
  `--verbose`.

T1, T2, T3, T5, T6 run on every commit; T4 and the fault-injection halves of T2 run in the
merge gate; the full matrix on real model downloads runs in the release pipeline
([RFC-0012](RFC-0012-release-engineering.md#proposed-design)).

## Open Questions

None kept open. Both questions raised in the draft are resolved:

1. **TUI vs line-oriented (resolved, LD15):** the CLI stays line-oriented through v0.x; no
   TUI ships before the desktop app. `ps` and `bench` remain single-shot, pipeable commands.
2. **Alias namespace collisions (resolved, LD16):** user-defined aliases win over shipped
   ones, with a one-line stderr warning naming both targets; no mandatory `my/` prefix. The
   alias table itself (shipped in-binary, user-extensible, refreshed only by explicit
   `drakkar alias update`) is LD3, owned by
   [RFC-0006](RFC-0006-model-pipeline.md#proposed-design).

## References

- `clap` (Rust argument parsing) and `clap_complete`; `rustyline`; the `NO_COLOR` convention
  (no-color.org); launchd/LaunchAgents documentation (Apple)
- Ollama and mlx-serve CLI surfaces (`run`/`ps`/`pull`/name:tag) as prior art; the vLLM CLI
  (`serve`/`chat`/`bench`) for the serve/bench split
- OpenAI/Anthropic client expectations for localhost base URLs
  ([RFC-0007](RFC-0007-api-server.md#proposed-design))
- [PRD](../../PRD.md): G1, P7, §3 target users, §6 differentiation, §8 roadmap
- [RFC-0001 Architecture](RFC-0001-architecture.md#proposed-design) (A1 in-process `run`, A5
  memory contracts); [RFC-0004 Feasibility Engine](RFC-0004-feasibility-engine.md#proposed-design)
  (FE25 `fit` CLI); [RFC-0005 KV Cache](RFC-0005-kv-cache.md#proposed-design) (KV17/KV19 disk
  tier); [RFC-0006 Model Pipeline](RFC-0006-model-pipeline.md#proposed-design) (MP1/MP2
  resolution, aliases); [RFC-0009 Performance](RFC-0009-performance.md#proposed-design) (PB8
  `bench`); [RFC-0011 Error Taxonomy](RFC-0011-error-taxonomy.md#proposed-design) (exit-code
  mapping); [RFC-0012 Release Engineering](RFC-0012-release-engineering.md#proposed-design)
  (release-pipeline test tiers)
