//! `drakkar-server` — the HTTP layer (layer 3, [RFC-0007]).
//!
//! `axum`/`tokio`/`tower` OpenAI and Anthropic dialect handlers, SSE streaming,
//! `/fit`, and `/v1/models`. A library crate consumed by `drakkar-cli`'s `serve`
//! subcommand. It depends on `drakkar-core`, `drakkar-sched`, `drakkar-engine`,
//! `drakkar-fit`, `drakkar-models`, and `drakkar-grammar`, and names no backend
//! crate (DEP4/DEP5).
//!
//! Skeleton established by the workspace scaffold (issue #120); the axum/tokio
//! server lands in #240.
//!
//! [RFC-0007]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0007-api-server.md
#![forbid(unsafe_code)]
