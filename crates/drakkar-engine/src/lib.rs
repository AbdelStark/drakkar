//! `drakkar-engine` — the backend seam and the engine actor (layer 1).
//!
//! Defines the `InferenceBackend` trait ([architecture §6]) and the `KvPool`
//! interface ([RFC-0005]), and implements the engine actor: the dedicated
//! thread, its message loop, channel plumbing, and memory-contract enforcement.
//! It depends on `drakkar-core` and `drakkar-fit` (contract verification at
//! load) and contains **zero** backend-specific code — backends depend on this
//! crate, never the reverse (DEP3).
//!
//! Skeleton established by the workspace scaffold (issue #120); the trait and
//! seam types land in #190 and the actor in #191.
//!
//! [architecture §6]: https://github.com/AbdelStark/drakkar/blob/main/docs/spec/01-architecture.md
//! [RFC-0005]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0005-kv-cache.md
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod kv;

pub use kv::{
    BlockRef, CachedRun, ContiguousKvPool, EvictPolicy, EvictReport, HashChain, KvPool, KvStats,
    PoolError, Rejection, Reservation,
};
