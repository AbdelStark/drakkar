# Supported architectures, formats, and dialects

Before you pull a model, this page tells you whether DRAKKAR can serve it: which
architectures the native model-def layer covers, which weight formats and quant
schemes are accepted, and which chat-template / tool dialect applies.

## Architecture support

Backend A (MLX) implements each architecture natively, config-driven from the
model's `config.json` (RFC-0002 D3, RFC-0003). Anything not in the native set is
served by Backend B (llama.cpp) from a GGUF artifact when one exists.

| Architecture family | Backend A (MLX) | Notes |
| --- | --- | --- |
| Llama family (Llama 3.x, and derivatives) | ✅ | uniform GQA |
| Qwen3 / Qwen3.5 — dense and MoE | ✅ | MoE bills active params for decode |
| Gemma family (hybrid sliding-window ↔ global) | ✅ | SWA layers billed at the window (FE9) |
| gpt-oss (alternating attention, MXFP4) | ✅ | native MXFP4 pass-through |
| Mistral family | ✅ | |
| DeepSeek lineage (MLA latent KV) | ✅ | latent-vector KV accounting (FE10) |
| Anything else with a GGUF build | via Backend B | reduced `Capabilities`; use `--format gguf` |

New architectures are added on a **rolling (roughly weekly) cadence**. A model
whose architecture this build does not cover fails with
[`models.unsupported_architecture`](error-codes.md#models) (exit 6) — the report
suggests a GGUF sibling (`drakkar pull <ref> --format gguf`, Backend B) or an
upgrade.

## Weight formats and quantization

**Accepted formats:** `safetensors` (MLX-native quantized or upstream
bf16/fp16) and `GGUF`. **`.pth` / pickle checkpoints are rejected** — pickle
executes code on load, and DRAKKAR treats artifacts as data, never code
(RFC-0001 A11, MP6). Multi-file GGUF splits, sharded safetensors, and single
consolidated files are all handled.

Quantization is described by a `QuantDesc` (`scheme`, `bits`, `group`,
`bpw_eff`). Native Backend A formats (IC6):

| Scheme | Effective bpw | Source |
| --- | --- | --- |
| MLX affine, group 64 | 4.5 / 5.5 / 6.5 / 8.5 | mlx-community repos or on-device convert (default serving format) |
| MLX affine, group 32 | +0.5 bpw vs g64 | quality bump for small (< 4B) models |
| MXFP4 | 4.25 | native checkpoints (gpt-oss), pass-through |
| bf16 / fp16 | 16 | HF safetensors, served directly when it fits |

Backend B serves the GGUF quant zoo (K-quants, i-quants) unmodified; the fit
engine carries per-family bpw tables for both formats.

## Artifact selection (MP4/MP5)

The model manager is **fit-driven**: it asks `drakkar-fit` for the target bits
per weight and picks the artifact whose effective bpw is closest without
exceeding the plan. The chosen route is always displayed — provenance is part of
"honest speed". Preference order:

1. An **MLX-format repo** at the fit-recommended bits (Backend A).
2. **Original safetensors** bf16/fp16, if it fits directly (Backend A).
3. **Original safetensors + on-device quantization** to the recommended
   bits/group (Backend A).
4. A **GGUF repo** at the closest quant when no MLX/safetensors route exists or
   the architecture is unsupported in A (Backend B).

`--quant` and `--format` override the automatic choice: `--format gguf` forces
Backend B; `--quant 4bit-g64` pins the quantization.

## Chat templates and tool dialects

The repo's Jinja chat template is executed in a **sandboxed** environment (no
filesystem, network, or process access; bounded recursion and output size,
INV-MP-NOCODE, MP18). A curated **override table** patches known-broken templates
per `(repo_id, revision)`; it ships in the binary and refreshes only via
`drakkar alias update` (the same channel as the alias table, LD3).

The **tool-call and reasoning-block dialect** is declared per model family in the
model-def layer and drives both prompt rendering and stream parsing:

| Family | Tool dialect |
| --- | --- |
| Nous / Hermes-style models | Hermes (`<tool_call>` JSON blocks) |
| Qwen family | Qwen |
| Mistral family | Mistral (`[TOOL_CALLS]`) |
| Llama 3 family | Llama |
| DeepSeek family | DeepSeek |
| gpt-oss | gpt-oss |
| families with no native convention | none |

**Fail-visible (MP19):** `drakkar run` prints which template and tool dialect are
active at debug verbosity (`-v`) — template/dialect mismatch is the top silent
quality-killer in local serving, so it must be observable. A template render
error surfaces as a named error, never a silently wrong prompt.
