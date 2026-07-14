# Troubleshooting and `drakkar doctor`

This is the task-oriented companion to the
[error-code reference](error-codes.md): find your symptom, read the stable code
and exit code it maps to, and apply the fix. Every DRAKKAR failure carries a
stable code — read it from `--json` output rather than scraping messages.

## By symptom

### "Won't fit" — the model is too big for this machine

- **Code:** [`fit.wont_fit`](error-codes.md#fit) · **CLI exit:** 4 · **HTTP:** 422
- **What happened:** even at the floor plan (lowest sane quant, 4k context,
  8-bit KV, max safe wired limit) the model exceeds the usable GPU budget.
- **Fix:** run `drakkar fit <ref>` to see the ranked remedies — a smaller
  official quant, on-device quantization, 8-bit KV, a reduced context, or a
  wired-limit raise. The report also names the nearest sibling model that fits.
  `--force` downgrades the verdict to a warning and proceeds at your own risk.

### "Prompt + max_tokens exceeds the admissible context"

- **Code:** [`fit.context_exceeded`](error-codes.md#fit) · **HTTP:** 413
- **Fix:** reduce the request, or reload with `--kv-bits 8` (or 4). `drakkar fit
  <ref>` prints the context ceiling per KV precision.

### Download or network failures

- **Codes:** [`download.network_failed`](error-codes.md#download) (exit 5),
  [`download.hub_unreachable`](error-codes.md#download) (exit 5),
  [`download.integrity_mismatch`](error-codes.md#download) (exit 6).
- **What happened:** the Hugging Face hub was unreachable, the transfer was
  interrupted, or a downloaded blob failed integrity verification.
- **Fix:** network failures are retryable — re-run the same command to resume;
  completed files are never re-fetched. If integrity mismatches persist, pin a
  known-good revision with `@<rev>`. Installed models keep working offline
  (`drakkar ls`).

### Out of disk space

- **Code:** [`download.no_space`](error-codes.md#download) · **CLI exit:** 7 ·
  **HTTP:** 507
- **Fix:** the report states how much space is needed (download + conversion
  workspace + output) and how much is free. `drakkar prune` reclaims blobs
  unreferenced by any manifest, or set `storage.path` to another volume.

### Metal / GPU initialization failure

- **Code:** [`engine.metal_init_failed`](error-codes.md#engine) · **CLI exit:** 6
- **Fix:** run `drakkar doctor` — it reports the GPU, the macOS version (macOS
  15+ is required), and the wired-limit status. A Metal fault that recurs on the
  same input is a bug worth reporting.

### Gated or unsupported models

- **Gated repo:** [`models.gated_repo_no_token`](error-codes.md#models) (exit 3)
  — accept the license on the model page, then provide a token via `HF_TOKEN`,
  `~/.huggingface`, or the keychain.
- **Unsupported architecture:**
  [`models.unsupported_architecture`](error-codes.md#models) (exit 6) — try a
  GGUF artifact (`drakkar pull <ref> --format gguf`), or upgrade: architectures
  are added on a rolling cadence.

## Reading `drakkar doctor`

`drakkar doctor` prints an environment and self-diagnosis report; `drakkar
doctor --json` emits the same as a `drakkar.doctor/1` object for scripts. Key
fields:

| Field | Meaning |
| --- | --- |
| `chip` / `gpu_cores` | The Apple Silicon chip identity and GPU core count (from IOKit/sysctl). |
| `budget_gib` | The GPU memory budget — the live `recommendedMaxWorkingSetSize`, or a table value in `--machine` mode. |
| `wired_limit_mb` | The current `iogpu.wired_limit_mb`; `doctor` states whether it is safe for the resident model and never changes it automatically. |
| `macos` | The macOS version; feature paths (e.g. the Neural Accelerator) are gated on it. |
| `nax` | Whether the Metal 4 tensor-op (Neural Accelerator) self-test passed — never inferred from a version number. |
| `disk` | Free space on the store volume and reclaimable blob bytes. |
| `config` | Config-file sanity: unknown keys or out-of-range values are reported here, and world/group-readable secret files are flagged. |

Run `drakkar doctor --check-update` to explicitly check for a newer release — it
is the only network call `doctor` makes, and only on request.

## Reporting a bug

When you file a GitHub issue, attach (matching
[CONTRIBUTING](../../CONTRIBUTING.md#reporting-bugs)):

1. The output of `drakkar doctor --json`.
2. The exact command you ran.
3. The **error code** from the failure (it is stable; include it verbatim).
4. Your hardware and macOS version.

For security issues, do **not** open a public issue — see
[SECURITY.md](../../SECURITY.md).
