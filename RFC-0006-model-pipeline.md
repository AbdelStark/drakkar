# RFC-0006: Model Acquisition and Format Pipeline

**Status:** Draft
**Author:** A. Bakhta
**Created:** 2026-07-14
**Requires:** RFC-0001, RFC-0004
**Related:** RFC-0003 (formats), RFC-0008 (CLI)

## 1. Summary

"Run a model by pasting a Hugging Face link" is the headline promise. This RFC specifies reference resolution, artifact selection across the format zoo, downloading, integrity, storage, and the conversion/quantization pipeline that turns any reasonable HF repo into a servable local artifact, with the feasibility engine in the loop before bytes move.

## 2. Reference resolution

- MP1. Accepted forms, normalized to `(repo_id, revision)`: full URLs (`https://huggingface.co/Qwen/Qwen3-8B`, `hf.co/...`, including `/tree/<rev>` and file deep links), bare `org/repo`, `org/repo@revision`, curated aliases (`qwen3:8b`, `gpt-oss:20b`) from a shipped, user-extensible alias table, and local paths (directory with `config.json` + safetensors, or a `.gguf` file).
- MP2. Resolution fetches only metadata first (model card header, `config.json`, file listing, safetensors index): enough for the fit preflight (FE1) without downloading weights. Gated/private repos use the standard HF token discovery (env, `~/.huggingface`, keychain); a gated repo without a token produces a named error with the acceptance URL.
- MP3. Sibling discovery: given a base repo, the resolver locates known-good quantized siblings (same model, mlx-community and original-org quant repos, GGUF repos) via a shipped mapping plus HF search, so remedies in the fit report ("use the official 4-bit") are one keypress, not a research project.

## 3. Artifact selection policy

- MP4. Preference order for Backend A (MLX): (1) MLX-format repo at the fit-recommended bits (mlx-community hosts ~4,800 conversions), (2) original safetensors bf16/fp16 if it fits directly, (3) original safetensors + on-device quantization (§6). Backend B path: (4) GGUF repo at the closest quant when no MLX/safetensors route exists or the architecture is unsupported in A.
- MP5. The selector is fit-driven: it asks `drakkar-fit` for the target bpw and picks the artifact whose effective bpw is closest without exceeding the plan. `--quant` / `--format` override. The chosen route is always displayed (provenance is part of "honest speed").
- MP6. Multi-file GGUF splits, sharded safetensors, and consolidated single files are all handled; `.pth`/pickle checkpoints are rejected (security, A11) with a pointer to conversion guidance.

## 4. Download

- MP7. Parallel ranged downloads with per-file resume; Xet/CDN-aware via the hf-hub crate; default 4 connections, saturating typical links without starving the machine. Progress UX per RFC-0008 (bytes, ETA, post-download step preview).
- MP8. Integrity: verify per-file sizes and ETag/sha256 from the hub where offered; safetensors headers parsed defensively (bounded header size, no allocations from untrusted lengths); GGUF metadata parsed with the same discipline.
- MP9. Disk preflight: required = download + (conversion workspace if any) + output; refuse with a clear number when the volume lacks space, before starting.

## 5. Storage layout

- MP10. Content-addressed store: `~/.drakkar/models/blobs/sha256-*` with human-readable manifests `~/.drakkar/models/manifests/<org>/<repo>/<rev>.json` mapping names to blobs (Ollama-style, proven). Identical tensors shared across revisions dedupe by construction.
- MP11. HF cache interop: if `HF_HOME`'s hub cache already holds needed files, they are **hard-linked or reflinked (APFS clonefile)** into the store rather than re-downloaded; DRAKKAR never mutates the HF cache. `storage.import_hf_cache = "clone" | "copy" | "off"`.
- MP12. `drakkar ls` lists installed models with size, format, quant, last-used; `drakkar rm` removes manifests and garbage-collects unreferenced blobs; `drakkar prune` reports reclaimable space first.

## 6. Conversion and on-device quantization

- MP13. Converter runs in-process (Rust orchestration, backend kernels for quantize): safetensors(bf16) → MLX affine at recipe bits/group (FE6 recipes shared with the estimator). Streaming per-tensor (RFC-0003 IC8): peak memory ≈ one shard + output tensor; a 70B fp16 → 4-bit conversion MUST succeed on a 48 GB machine.
- MP14. Throughput target: conversion bounded by SSD read + quantize compute; ≥ 1 GB/s of input on M4-class (est., benchmark in RFC-0009); progress and cancellation supported; output lands atomically (temp + rename).
- MP15. Calibration-based schemes (AWQ/GPTQ-style import, activation-aware low-bit) are v1.x: v1 imports pre-quantized artifacts of those families where the backend supports the layout, and otherwise recommends official quants (the honest answer beats a bad on-device 2-bit).
- MP16. `drakkar convert <model> --bits B --group G [--recipe R]` exposes the pipeline directly; converted artifacts register in the store like downloads and may be pushed back to the hub by the user with standard tools (out of scope to upload for them in v1).

## 7. Chat templates, tokenizers, tool formats

- MP17. Tokenizers load via the HF tokenizers crate from `tokenizer.json` (fast path) with sentencepiece fallback; tokenizer hash feeds cache keys (KV12).
- MP18. Chat templates: the repo's Jinja template is executed by a sandboxed minijinja environment with the standard HF template API surface; a curated override table patches known-broken templates per model+revision (versioned, tested). Tool-call and reasoning-block dialects are declared per model family in the model-def layer and drive both prompt rendering and stream parsing (RFC-0007 §5).
- MP19. `drakkar run` prints which template and tool dialect are active at debug verbosity; mismatch bugs are the top silent-quality-killer in local serving and MUST be observable.

## 8. Acceptance criteria

- AC1. Each of these resolves and runs with zero flags on a suitable machine: a full HF URL to an mlx-community 4-bit repo; `Qwen/Qwen3-8B` (bf16 source, auto-quantized); a GGUF repo URL; a local GGUF file; an alias.
- AC2. Kill during download at 60%, rerun: completes with no re-downloaded completed files.
- AC3. HF-cache interop: pre-seeded hub cache yields zero network bytes for weights and clone-level disk cost.
- AC4. 70B bf16 → 4-bit on 48 GB machine: succeeds within the IC8 memory envelope; output bpw matches the estimator within 1%.
- AC5. Pickle checkpoint: rejected with the documented error and remedy text.

## Open questions

1. Alias table governance: ship-in-binary (stale risk) vs fetched manifest (network dependency); leaning ship-in-binary + `drakkar alias update` explicit refresh.
2. Default store location on external volumes (large models on small internal SSDs): support `storage.path` day one, or v0.2?

## References

- huggingface/hf-hub and tokenizers crates; safetensors format spec
- mlx-community organization scale (~4,800 models, WWDC26 figure); mlx_lm.convert flow (Apple MLR post)
- Ollama blob/manifest store design (prior art); mistral.rs ISQ (in-situ quantization) precedent
- ggml-org GGUF spec; LM Studio model-folder discovery precedent (mlx-serve README)
