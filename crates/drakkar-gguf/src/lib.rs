//! `drakkar-gguf` — backend B (layer 2, [RFC-0002 D4]).
//!
//! llama.cpp embedded via FFI, implementing the same `InferenceBackend` trait as
//! `drakkar-mlx` with reduced `Capabilities`. It depends on `drakkar-core` and
//! `drakkar-engine` and exposes the same single-constructor public surface.
//!
//! The backend is gated behind the `gguf` cargo feature, which is on by default
//! (and therefore in release builds) and is named only by the composition root
//! `drakkar-cli` (DEP4).
//!
//! Skeleton established by the workspace scaffold (issue #120); the GGUF
//! artifact-selection route lands in #82.
//!
//! [RFC-0002 D4]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0002-stack-selection.md
