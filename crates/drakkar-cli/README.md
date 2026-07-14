# drakkar-cli

The `drakkar` command-line binary and CLI framework (RFC-0008, layer 4). This
crate owns command parsing, the dual human/`--json` output framework,
TTY/`NO_COLOR` color gating, and the exit-code path. Individual command bodies
plug into this framework in their own issues.

## Global flags

Accepted on every command (public-API §2.2, RFC-0008 CLI6–CLI9):

| Flag | Meaning |
| --- | --- |
| `--json` | Emit a single machine-readable JSON object on stdout, `schema` first. |
| `--stream-json` | Emit JSON Lines streaming events (streaming commands only). |
| `--quiet`, `-q` | Suppress non-error progress on stderr. |
| `--verbose`, `-v` | Raise the stderr log level; repeat (`-vv`) for more detail. |
| `--yes`, `-y` | Assume "yes" to interactive confirmations. |
| `--force` | Override a `Won't fit` verdict and proceed (the exit-4 path). |

Output discipline (RFC-0008 AC2): stdout carries machine output only under
`--json`; logs and progress always go to stderr, so
`drakkar fit <ref> --json | jq .verdict` works with nothing else on stdout.
`NO_COLOR` or a non-TTY stdout disables ANSI automatically (CLI9).

Configuration precedence (LD23): flags > `DRAKKAR_*` env >
`~/.config/drakkar/config.toml` > built-in defaults.

## Exit codes

Owned by RFC-0008 CLI8 / public-API §2.3; the category→code mapping lives in
`drakkar-core::error::mapping` (RFC-0011 ER2) and is never re-mapped here.
Append-only and never renumbered (API3).

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 2 | Usage error (bad flags/args) |
| 3 | Model or reference not found |
| 4 | Won't fit (feasibility failure without `--force`) |
| 5 | Download/network failure |
| 6 | Engine/runtime failure (load, Metal, inference); also the top-level panic wrapper |
| 7 | Disk/space failure |

Code 1 is deliberately unassigned and never emitted intentionally; observing
exit 1 indicates a defect in the panic wrapper.

## Command surface

`drakkar --help` lists the full surface, including milestone-gated commands
(`ps`, `bench`, `convert` in v0.2), so `--help` and completions are honest about
what the shipped binary will grow into.
