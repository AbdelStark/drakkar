//! `drakkar-core` — the bottom of the DRAKKAR dependency graph (layer 0).
//!
//! This crate owns the shared vocabulary types (`GenerationRequest`,
//! `ModelArtifact`, `MemoryBudget`, `MemoryReport`, `Capabilities`,
//! `SamplerParams`, token/usage types), the error taxonomy
//! ([RFC-0011]), the config schema, the versioned JSON schema definitions
//! (invariant I4), and the tracing conventions. It performs no I/O and pulls in
//! no async runtime; everything in the workspace depends on it and it depends on
//! no other workspace crate (DEP2).
//!
//! This is the skeleton established by the workspace scaffold (issue #120).
//! The shared types land in #121 and the error taxonomy in #123.
//!
//! [RFC-0011]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0011-error-taxonomy.md
#![forbid(unsafe_code)]
