//! `drakkar-grammar` — the structured-output engine (layer 0).
//!
//! JSON-schema and grammar-constrained decoding via `llguidance`
//! ([RFC-0002 D1]). It compiles grammars and advances constraint state per
//! sampled token, entirely in Rust, and depends only on `drakkar-core` (DEP2).
//!
//! Skeleton established by the workspace scaffold (issue #120); the `llguidance`
//! integration lands in #266.
//!
//! [RFC-0002 D1]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0002-stack-selection.md
#![forbid(unsafe_code)]
