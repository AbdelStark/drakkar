//! `drakkar-mlx` — backend A (layer 2).
//!
//! A safe Rust wrapper over `drakkar-mlx-sys` implementing the `InferenceBackend`
//! and `KvPool` traits from `drakkar-engine`; model-graph construction is
//! config-driven per [RFC-0002 D3]. It depends on `drakkar-core`,
//! `drakkar-engine`, and `drakkar-mlx-sys`, and exposes exactly one public
//! constructor plus a capability probe — all MLX-typed items are `pub(crate)` so
//! no FFI type crosses the backend seam (DEP5, invariant I5). `unsafe` lives here
//! and in `drakkar-mlx-sys` only.
//!
//! Skeleton established by the workspace scaffold (issue #120); the RAII handle
//! wrappers land in #172 and the forward graph in #194.
//!
//! [RFC-0002 D3]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0002-stack-selection.md
