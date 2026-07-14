//! The `KvPool` trait surface and the v0.1 interim contiguous pool (RFC-0005
//! KV22, Migration/Rollout v0.1 row).
//!
//! Everything above the KV seam — the scheduler, the backend, the admission
//! arithmetic — is written against the [`KvPool`] trait from day one, so the
//! v0.2 paged-pool swap changes no caller. The `kv()` accessor on
//! `InferenceBackend` returns this trait (RFC-0001 A6). The v0.1 implementation
//! ([`ContiguousKvPool`]) is single-sequence with no sharing: `lookup_prefix`
//! returns `None` and `seal` always frees.
//!
//! The KV interface lives in `drakkar-engine` per the architecture crate map
//! (docs/spec/01-architecture.md §3); there is no separate `drakkar-kv` crate in
//! the frozen 11 (LD24).

use std::collections::BTreeMap;

use drakkar_core::{PrefixHashChain, SeqId};

/// A prefix hash chain used for cache lookup (KV9). Aliases `drakkar-core`'s
/// [`PrefixHashChain`] so there is one definition (DM1).
pub type HashChain = PrefixHashChain;

/// An opaque reference to a filled KV block appended to a sequence (KV3). The
/// v0.1 contiguous pool does not page, so this is a marker for the frozen trait
/// shape; the paged pool (#27) gives it meaning.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BlockRef(pub u64);

/// A successful KV reservation covering `prompt + max_tokens` for a sequence
/// (KV22/FE18).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reservation {
    /// The sequence the reservation is for.
    pub seq: SeqId,
    /// Tokens reserved (`prompt + max_tokens`).
    pub tokens_reserved: usize,
}

/// A rejected admission, carrying the largest admissible token count at current
/// occupancy (KV22/FE18) — the value the scheduler puts in a `429
/// kv.pool_exhausted` body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rejection {
    /// The largest `max_tokens` that would be admitted right now.
    pub max_admissible: usize,
}

/// The longest cached run matching a prefix hash chain (KV9–KV10). The v0.1
/// pool never returns one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CachedRun {
    /// Number of prompt tokens served from cache.
    pub matched_tokens: usize,
}

/// A KV reclaim policy (KV20–KV21).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvictPolicy {
    /// Evict runs past their TTL.
    Ttl,
    /// Evict the lowest-scored runs under memory pressure.
    Pressure,
}

/// The outcome of one [`KvPool::evict`] pass (KV20–KV21). The v0.1 pool reclaims
/// nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EvictReport {
    /// Tokens of cached state reclaimed.
    pub reclaimed_tokens: usize,
    /// Cached runs evicted.
    pub evicted_runs: usize,
}

/// Pool occupancy and hit-rate statistics (KV23–KV24).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KvStats {
    /// Total token capacity of the pool.
    pub tokens_total: usize,
    /// Tokens currently reserved by active sequences.
    pub tokens_reserved: usize,
    /// Tokens free for new admissions.
    pub tokens_free: usize,
    /// Number of active sequences.
    pub sequences_active: usize,
    /// Cumulative prompt tokens served from cache.
    pub prefix_hit_tokens: u64,
    /// Cumulative prompt tokens seen.
    pub prompt_tokens: u64,
}

/// An error appending to a sequence's KV table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolError {
    /// The sequence was never admitted (or was already sealed).
    SequenceNotAdmitted(SeqId),
}

impl std::fmt::Display for PoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolError::SequenceNotAdmitted(seq) => {
                write!(f, "sequence {} was not admitted to the KV pool", seq.0)
            }
        }
    }
}

impl std::error::Error for PoolError {}

/// The KV pool interface consumed by the scheduler and backend (KV22). The
/// pool never grows beyond its load-time byte contract; `admit` is the only
/// rejection point and always returns the computable `max_admissible`
/// (INV-NO-GROWTH).
pub trait KvPool {
    /// Reserve capacity covering `tokens_needed` (`prompt + max_tokens`) for a
    /// sequence, or reject with the largest admissible `max_tokens` at current
    /// occupancy (FE18).
    fn admit(&mut self, seq: SeqId, tokens_needed: usize) -> Result<Reservation, Rejection>;

    /// The longest cached run matching a prefix hash chain (KV9–KV10).
    fn lookup_prefix(&self, chain: &HashChain) -> Option<CachedRun>;

    /// Append a filled block to a sequence's table (KV3; CoW split if shared,
    /// I-KV4).
    fn append(&mut self, seq: SeqId, block: BlockRef) -> Result<(), PoolError>;

    /// The sequence finished: donate its prefix run to cached state (KV11), or
    /// free everything when `donate == false` (per-request opt-out, LD8).
    fn seal(&mut self, seq: SeqId, donate: bool);

    /// Run one reclaim pass under the given policy (KV20–KV21).
    fn evict(&mut self, policy: EvictPolicy) -> EvictReport;

    /// Occupancy by state, hit rates, per-sequence tables (KV23–KV24).
    fn stats(&self) -> KvStats;
}

/// The v0.1 interim KV pool: one contiguous reservation per sequence, sized
/// `prompt + max_tokens`, carved from the model memory contract at load
/// (`pool_bytes` from the fit memory model). Single-sequence with no sharing:
/// `lookup_prefix` returns `None`, `seal` always frees, and `evict` is a no-op.
/// The trait shape is frozen so the v0.2 paged swap changes no caller.
#[derive(Debug)]
pub struct ContiguousKvPool {
    pool_bytes: u64,
    bytes_per_token: u64,
    reserved: BTreeMap<SeqId, usize>,
}

impl ContiguousKvPool {
    /// Carve a fixed pool of `pool_bytes`, billing `bytes_per_token` per reserved
    /// token (the uniform KV rate from the fit memory model). The pool never
    /// allocates beyond `pool_bytes`.
    #[must_use]
    pub fn new(pool_bytes: u64, bytes_per_token: u64) -> Self {
        ContiguousKvPool {
            pool_bytes,
            bytes_per_token,
            reserved: BTreeMap::new(),
        }
    }

    fn reserved_tokens(&self) -> usize {
        self.reserved.values().copied().sum()
    }

    fn reserved_bytes(&self) -> u64 {
        (self.reserved_tokens() as u64).saturating_mul(self.bytes_per_token)
    }

    fn token_capacity(&self) -> usize {
        if self.bytes_per_token == 0 {
            usize::MAX
        } else {
            (self.pool_bytes / self.bytes_per_token) as usize
        }
    }
}

impl KvPool for ContiguousKvPool {
    fn admit(&mut self, seq: SeqId, tokens_needed: usize) -> Result<Reservation, Rejection> {
        if self.bytes_per_token == 0 {
            self.reserved.insert(seq, tokens_needed);
            return Ok(Reservation {
                seq,
                tokens_reserved: tokens_needed,
            });
        }
        let free_bytes = self.pool_bytes.saturating_sub(self.reserved_bytes());
        let needed_bytes = (tokens_needed as u64).saturating_mul(self.bytes_per_token);
        if needed_bytes <= free_bytes {
            self.reserved.insert(seq, tokens_needed);
            Ok(Reservation {
                seq,
                tokens_reserved: tokens_needed,
            })
        } else {
            Err(Rejection {
                max_admissible: (free_bytes / self.bytes_per_token) as usize,
            })
        }
    }

    fn lookup_prefix(&self, _chain: &HashChain) -> Option<CachedRun> {
        // v0.1: no prefix sharing (Migration/Rollout v0.1 row).
        None
    }

    fn append(&mut self, seq: SeqId, _block: BlockRef) -> Result<(), PoolError> {
        if self.reserved.contains_key(&seq) {
            // The contiguous reservation already covers the sequence; nothing to
            // page in v0.1.
            Ok(())
        } else {
            Err(PoolError::SequenceNotAdmitted(seq))
        }
    }

    fn seal(&mut self, seq: SeqId, _donate: bool) {
        // v0.1: always free, regardless of `donate` (no cached state to donate).
        self.reserved.remove(&seq);
    }

    fn evict(&mut self, _policy: EvictPolicy) -> EvictReport {
        // v0.1: nothing to reclaim.
        EvictReport::default()
    }

    fn stats(&self) -> KvStats {
        let tokens_total = self.token_capacity();
        let tokens_reserved = self.reserved_tokens();
        KvStats {
            tokens_total,
            tokens_reserved,
            tokens_free: tokens_total.saturating_sub(tokens_reserved),
            sequences_active: self.reserved.len(),
            prefix_hit_tokens: 0,
            prompt_tokens: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1000-token pool at 100 bytes/token = 100_000 bytes.
    fn pool() -> ContiguousKvPool {
        ContiguousKvPool::new(100_000, 100)
    }

    #[test]
    fn admit_succeeds_within_budget() {
        let mut p = pool();
        let r = p.admit(SeqId(1), 400).unwrap();
        assert_eq!(r.tokens_reserved, 400);
        assert_eq!(p.stats().tokens_reserved, 400);
        assert_eq!(p.stats().sequences_active, 1);
    }

    #[test]
    fn admit_rejects_with_max_admissible() {
        let mut p = pool();
        p.admit(SeqId(1), 700).unwrap(); // 700 of 1000 used
        let rej = p.admit(SeqId(2), 400).unwrap_err(); // only 300 free
        assert_eq!(rej.max_admissible, 300);
    }

    #[test]
    fn lookup_prefix_returns_none_in_v0_1() {
        let p = pool();
        assert!(p.lookup_prefix(&HashChain::default()).is_none());
    }

    #[test]
    fn seal_frees_regardless_of_donate() {
        for donate in [true, false] {
            let mut p = pool();
            p.admit(SeqId(1), 500).unwrap();
            p.seal(SeqId(1), donate);
            assert_eq!(p.stats().tokens_reserved, 0);
            assert_eq!(p.stats().sequences_active, 0);
            // The freed capacity is fully re-admissible.
            assert!(p.admit(SeqId(2), 1000).is_ok());
        }
    }

    #[test]
    fn never_allocates_beyond_pool_bytes() {
        let mut p = pool();
        assert!(p.admit(SeqId(1), 1000).is_ok()); // exactly full
        assert!(p.admit(SeqId(2), 1).is_err()); // one token over the contract
        assert!(p.reserved_bytes() <= p.pool_bytes);
    }

    #[test]
    fn append_requires_admission_and_evict_is_a_noop() {
        let mut p = pool();
        assert_eq!(
            p.append(SeqId(9), BlockRef(0)),
            Err(PoolError::SequenceNotAdmitted(SeqId(9)))
        );
        p.admit(SeqId(1), 100).unwrap();
        assert!(p.append(SeqId(1), BlockRef(0)).is_ok());
        assert_eq!(p.evict(EvictPolicy::Ttl), EvictReport::default());
    }
}
