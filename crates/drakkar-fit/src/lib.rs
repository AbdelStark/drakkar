//! `drakkar-fit` — the feasibility engine ([RFC-0004]).
//!
//! A pure library (layer 0): the memory model, hardware profiles, context
//! ceilings, TTFT/decode estimates, and the calibration-store reader. It depends
//! only on `drakkar-core` (DEP2). Its purity is what lets CLI preflight, the
//! `/fit` endpoint, and scheduler admission control share one implementation, and
//! it is the *single source of truth* for memory math (invariant I3).
//!
//! Skeleton established by the workspace scaffold (issue #120); the library and
//! its core I/O types land in #224.
//!
//! [RFC-0004]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0004-feasibility-engine.md
#![forbid(unsafe_code)]
