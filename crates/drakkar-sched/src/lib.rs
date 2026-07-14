//! `drakkar-sched` — the scheduler (layer 2, [RFC-0007]).
//!
//! Admission control (calling `drakkar-fit`), continuous batching, chunked
//! prefill, ITL-protection interleaving, per-request limits, and prefix-hash
//! computation for cache lookup. It speaks to engine actors over the channel
//! protocol `drakkar-engine` defines, and depends on `drakkar-core`,
//! `drakkar-fit`, and `drakkar-engine` (DEP1). It names no backend crate (DEP4).
//!
//! Skeleton established by the workspace scaffold (issue #120); the continuous
//! batching step loop lands in #256.
//!
//! [RFC-0007]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0007-api-server.md
#![forbid(unsafe_code)]
