//! `drakkar-models` — the acquisition pipeline (layer 1, [RFC-0006]).
//!
//! Reference resolution and aliases, the HF hub client (`hf-hub`), resumable
//! verified downloads, safetensors/GGUF inspection, convert/quantize
//! orchestration, the content-addressed store, HF-cache interop (LD4), and
//! tokenizer loading (`tokenizers`). Depends on `drakkar-core`.
//!
//! Skeleton established by the workspace scaffold (issue #120); reference
//! resolution lands in #72 and the store in #76.
//!
//! [RFC-0006]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0006-model-pipeline.md
#![forbid(unsafe_code)]
