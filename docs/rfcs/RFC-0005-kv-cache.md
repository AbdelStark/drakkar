# RFC-0005: KV Cache Subsystem

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.2

## Summary

The KV cache is where agentic workloads are won: a coding agent replays a near-identical multi-kilotoken scaffold dozens of times per session, and sub-agents fan out over a shared context. This RFC specifies a paged, quantization-aware, prefix-sharing KV pool with an optional SSD persistence tier. The paged pool, copy-on-write prefix sharing, and KV quantization land in v0.2; the SSD tier lands in v0.3 (see [Migration / Rollout](#migration--rollout)). v0.1 ships a contiguous per-sequence interim implementation behind the same `KvPool` trait so the v0.2 swap is invisible to everything above the seam.

Prior art acknowledged and built upon: vLLM's PagedAttention block model, SGLang's radix prefix reuse, mlx-lm's prompt cache, oMLX's two-tier RAM/SSD design with copy-on-write, and mlx-vlm's block prefix caching and KV quantization findings.

## Motivation

The PRD names the problem directly. [PRD §2.3](../../PRD.md#23-where-existing-tools-fall-short) identifies "KV cache amnesia across agent loops" as a top gap in the existing ecosystem: default mlx-lm and Ollama recompute long shared prefixes on every turn; block-level prefix reuse plus persistence exists only in newer niche servers (oMLX, mlx-vlm APC). [PRD P6](../../PRD.md#51-functional) makes the fix a product requirement: the engine MUST reuse KV state across requests sharing a prefix (system prompts, conversation history, agent scaffolds) automatically, in RAM and optionally on SSD.

The success bar is quantified by [PRD M4](../../PRD.md#7-success-metrics): 4 concurrent Claude-Code-style sessions against one 30B-A3B model on M4 Max sustain per-stream ITL under 45 ms with warm-prefix TTFT under 500 ms. That number is unreachable if each session pays a cold prefill for the shared scaffold; it is comfortably reachable if the scaffold's KV blocks are computed once and attached four times. The design below exists to make that the default behavior, not a tuning exercise.

The subsystem also carries half of the memory contract (RFC-0001 I2, [Architecture](RFC-0001-architecture.md#proposed-design)): KV is the only memory term that grows at runtime, so the pool allocator is where "never exceed the declared budget" is enforced by construction rather than by monitoring.

## Goals

In priority order (ties break toward the earlier goal):

- G1. **Never breach the memory contract.** The pool is fixed-size, carved from the budget at load; KV growth beyond it is impossible by construction (KV2), verified by pool accounting in the soak suite.
- G2. **Warm TTFT proportional to new tokens only.** For a request whose prompt shares an N-token cached prefix, prefill cost scales with `prompt_len − N`, not `prompt_len` (KV10, AC2).
- G3. **Keep decode reads cheap at long context.** Block layout is contiguous per layer for coalesced reads (KV1); quantized blocks are consumed natively with no fp16 shadow copy (KV14).
- G4. **Survive process restarts for expensive prefixes.** Evicted prefix runs persist to SSD and restore at SSD bandwidth, skipping prefill after a restart (KV17, AC4) — v0.3.

## Non-Goals

- **Cross-model KV sharing.** No sharing between models in v1, even for identical tokenizer + prefix; draft and target models in speculative decoding share nothing. The draft's KV is small enough that the win is marginal against the correctness-key complexity. Revisit post-v1 only with profiling evidence.
- **Preemption.** Running sequences are never preempted or offloaded to reclaim KV in v1 (KV21); preemption/offload is a v1.x scheduler feature. Admission control (FE18) is the v1 pressure valve.
- **Mixed-precision K/V schemes** (for example 3-bit keys / 4-bit values): a v1.x evaluation item, not v1 (KV15).
- **Cache sharing across users or machines.** The cache is local, per-user state under `~/.drakkar/`.

## Proposed Design

### Invariants

- **I-KV1 (pool bound).** `blocks_free + blocks_active + blocks_cached = pool_blocks` at all times; no allocation path can mint blocks outside the pool.
- **I-KV2 (refcount conservation).** Every physical block's refcount equals the number of block-table entries referencing it; a block reaches `free` exactly when its refcount reaches zero.
- **I-KV3 (no stale hit).** A cache lookup never returns blocks computed under a different model revision, tokenizer, chat template, KV precision, or rope scaling (KV12).
- **I-KV4 (write isolation).** A sequence never writes into a block with refcount > 1; CoW split precedes the write (KV4).

### 2. Block pool

- KV1. KV state lives in fixed-size **blocks** of 32 tokens (build-time constant; 16 evaluated against 32 in the RFC-0009 ablation — see [Open Questions](#open-questions)). A block stores K and V for all layers of its layout class, contiguous per layer for coalesced reads.
- KV2. The pool is allocated up-front from the model's memory contract at load: `pool_bytes = usable − weights − activation_watermark − fixed` (RFC-0004 FE16, [Feasibility Engine](RFC-0004-feasibility-engine.md#proposed-design)), carved into blocks; growth beyond the pool is impossible by construction (RFC-0001 invariant I2). A `--kv-pool` override may shrink it.
- KV3. Sequences map logical positions to physical blocks via per-sequence block tables; attention kernels read through the table (RFC-0003 IC10, [Inference Core](RFC-0003-inference-core.md#proposed-design)). Table metadata cost is charged by the fit engine (FE12).
- KV4. Block states: `free`, `active(seq)`, `cached(prefix, refcount)`. Refcounted sharing enables CoW: a shared block splits only when a sequence writes into it (partial tail block), copying once.

### 3. Layout classes (architecture-aware)

- KV5. **Global attention layers**: paged as above.
- KV6. **Sliding-window layers** (Gemma-class hybrids, gpt-oss alternates): fixed ring buffers of `window` tokens per sequence, allocated per-sequence outside the paged pool (their size does not scale with context; paging them wastes table overhead). Attention-sink slots (first tokens pinned) supported per model config.
- KV7. **MLA layers** (latent KV): paged, but blocks store the shared `(c_kv + d_rope)` latent per token; the attention path consumes the latent form directly. Accounting per RFC-0004 FE10.
- KV8. **Recurrent/SSM state**: constant per-sequence tensors owned alongside the ring buffers; snapshot/restore supported for prefix reuse at segment boundaries only.

### 4. Prefix sharing

- KV9. Prefix identity is a rolling hash chain over `(model_id, tokenizer_hash, chat_template_hash, token_ids)` computed at block granularity; the index is a radix-tree keyed by hash-chain segments, mapping to cached block runs.
- KV10. On admission, the scheduler queries the longest cached prefix; prefill starts at the first uncached token. Partial-block matches reuse up to the block boundary (tail recomputed). Target effect: a repeated 8k-token scaffold that costs a cold prefill of tens of seconds on M-class laptops collapses to seconds; parity with mlx-lm's warm path is the floor, cross-conversation and cross-restart reuse is the differentiator.
- KV11. Sharing works across concurrent sequences (fan-out agents attach to the same blocks with refcounts) and across time (completed sequences donate their prefix run to `cached` state instead of freeing, subject to the retention policy in KV20-KV21). Donation honors a per-request opt-out: a request carrying `cache: false` (or the equivalent header, RFC-0007, [API Server](RFC-0007-api-server.md#proposed-design)) is never donated to `cached` state — not in RAM and not on disk — and its blocks are freed on completion. The opt-out is cheap to honor (skip the `seal` donate path) and exists for privacy-sensitive callers.
- KV12. Correctness keys: any change in model revision, tokenizer, template, KV precision, or rope scaling invalidates the affected subtree; keys are content hashes, never names (invariant I-KV3).

### 5. KV quantization

- KV13. Supported precisions per model: fp16 (default), 8-bit, and 4-bit group-quantized (group 64, per-head scales); configured at load (`--kv-bits` / `kv_cache.bits`) and reported by `fit` (FE20 shows the context each precision buys).
- KV14. Quantization is applied at block granularity on write; attention kernels consume quantized blocks natively (dequant fused in the kernel), so there is no resident fp16 shadow copy.
- KV15. Sensitivity defaults: the final full-attention layer stays fp16 when `--kv-bits <= 8` on deep models (empirical quality cliff documented by mlx-vlm); model recipes may extend the exempt set. Mixed schemes (for example 3-bit keys / 4-bit values) are a v1.x evaluation item, not v1.
- KV16. Expected capacity effect (uniform-attention models): ~2x tokens at 8-bit, ~3.5x at 4-bit net of scale overhead (est., verified by the capacity fixtures in [Testing Strategy](#testing-strategy)); hybrid models see less because SWA terms dominate (the fit engine reports the true number, FE9).

### 6. SSD persistence tier

- KV17. Optional (`kv_cache.disk = true`, default on for `serve`, off for one-shot `run`; v0.3): evicted `cached` prefix runs are serialized to `~/.drakkar/kv-cache/` as safetensors blocks with an index sidecar; restored runs skip prefill after process restarts (oMLX precedent). The sidecar carries a schema version (`drakkar.kvdisk/1`) and the full KV12 correctness key; a version or key mismatch discards the entry silently and falls back to recompute.
- KV18. Writes are async on the blocking pool (RFC-0001 A4), throttled, and crash-safe (write-temp + rename); restore streams at SSD bandwidth and MUST beat recompute by ≥ 3x for the block run to be eligible (cost model: `bytes / ssd_bw` vs `tokens / prefill_tps`, using calibrated numbers from RFC-0009 PB15, [Performance](RFC-0009-performance.md#proposed-design)).
- KV19. Disk budget: default 8 GiB, LRU within it; `drakkar cache ls|clear` manages it. Cache files are mode 0600; contents are user prompts and MUST be treated as sensitive (documented, excluded from any diagnostics bundle by default). Requests that opted out via KV11 never reach this tier.

### 7. Eviction and retention

- KV20. RAM `cached` blocks are reclaimed under a cost-aware LRU: `score = recompute_cost × recency_decay ÷ bytes`; system-prompt-class prefixes (high reuse count) resist eviction. TTL default 30 min, configurable.
- KV21. Reclaim order under pool pressure: expired TTL → lowest-score cached → (never) active blocks. If active demand alone exceeds the pool, admission control blocks new requests (FE18; surfaced as `429 kv_pool_exhausted` per RFC-0007 AS8); running sequences are never preempted in v1 (preemption/offload is a v1.x scheduler feature).

### 8. Interfaces and observability

- KV22. `KvPool` trait surface (consumed by scheduler and backend; the `kv()` accessor on `InferenceBackend` returns it, RFC-0001 A6):

```rust
pub trait KvPool {
    /// Reserve blocks covering prompt + max_tokens for a sequence, or reject
    /// with the largest admissible max_tokens at current occupancy (FE18).
    fn admit(&mut self, seq: SeqId, tokens_needed: usize)
        -> Result<Reservation, Rejection>;      // Rejection { max_admissible: usize }

    /// Longest cached run matching a prefix hash chain (KV9-KV10).
    fn lookup_prefix(&self, chain: &HashChain) -> Option<CachedRun>;

    /// Append a filled block to a sequence's table (CoW split if shared, I-KV4).
    fn append(&mut self, seq: SeqId, block: BlockRef) -> Result<(), PoolError>;

    /// Sequence finished: donate its prefix run to cached state (KV11),
    /// or free everything when donate == false (per-request opt-out).
    fn seal(&mut self, seq: SeqId, donate: bool);

    /// Run one reclaim pass under the given policy (KV20-KV21).
    fn evict(&mut self, policy: EvictPolicy) -> EvictReport;

    /// Occupancy by state, hit rates, per-sequence tables (KV23-KV24).
    fn stats(&self) -> KvStats;
}
```

- KV23. Metrics (Prometheus, RFC-0007 AS21): pool occupancy by state, prefix hit rate (tokens served from cache / prompt tokens), warm-vs-cold TTFT histograms, evictions by reason, disk tier hit rate and restore bandwidth.
- KV24. `drakkar ps` shows per-model pool occupancy and hit rate; `--json` includes the full stats struct.

### Failure modes

| Failure | Response |
| --- | --- |
| Pool exhausted at admission | Structured rejection with `max_admissible` (KV22); scheduler surfaces `429 kv_pool_exhausted` with `retry_after_ms` (RFC-0007 AS8) |
| Active demand exceeds pool with nothing reclaimable | Admission blocks; running sequences unaffected (KV21) |
| Disk write fails (I/O error, budget full) | Entry dropped, RAM state unaffected, warning logged; serving continues |
| Crash during disk serialization | Temp file never renamed; startup scan deletes orphaned `*.tmp` files (KV18) |
| Disk entry fails key or checksum validation on restore | Entry discarded, prefill recomputes; counted in `evictions{reason="corrupt"}` |
| Refcount underflow/overflow | Debug builds assert (I-KV2); release builds quarantine the block as leaked, log, and continue — the leak detector test class exists to keep this path theoretical |

### External dependencies

No new external dependencies beyond the workspace baseline (RFC-0002, [Stack Selection](RFC-0002-stack-selection.md#proposed-design)). The disk tier uses the safetensors container format for block files (same format the model store already depends on) so cached blocks are inspectable and versionable with standard tooling; hashing uses the workspace's existing content-hash primitive (RFC-0006 store, [Model Pipeline](RFC-0006-model-pipeline.md#proposed-design)).

## Alternatives Considered

**Contiguous per-sequence KV (no paging).** One contiguous allocation per sequence, sized for `prompt + max_tokens`. Rejected as the end state: under continuous batching it fragments the pool (allocations of wildly different sizes churning at different lifetimes), and it structurally prevents cross-sequence sharing — a fan-out of 4 agents over a shared 8k scaffold would hold 4 full copies. It is, however, exactly right for v0.1's single-request engine, where there is one sequence and nothing to share: v0.1 ships contiguous KV behind the same `KvPool` trait (see [Migration / Rollout](#migration--rollout)).

**Radix tree of tokens (SGLang-style) instead of a hash chain of blocks.** SGLang's RadixAttention indexes cached prefixes in a token-level radix tree, which captures reuse at arbitrary token boundaries. Chosen instead: a rolling hash chain at block granularity (KV9). Rationale: lookups hash `prompt_len / block_size` segments instead of walking a per-token trie; matches are inherently block-aligned, which is what the paged allocator can actually reuse (a token-level match still rounds down to a block boundary, KV10); and content-keyed invalidation (KV12) composes naturally with hash keys. The radix structure is not discarded — the index is a radix tree keyed over hash-chain segments — but the unit of identity is the block, not the token. The cost is losing sub-block reuse granularity, bounded at `block_size − 1` recomputed tokens per match.

**Full-cache serialization (mlx-lm prompt-cache files).** mlx-lm persists an entire prompt cache to a file and reloads it. Rejected as the primary mechanism: it is coarse-grained (one artifact per conversation, no partial matching), supports no cross-sequence sharing, and duplicates storage for overlapping prefixes. Retained as an idea for a future import path: a one-shot converter from an mlx-lm prompt-cache file into a donated block run is cheap to add and eases migration for existing users. Not scheduled for v1.

**OS-pager-backed KV (mmap the pool, let macOS page cold blocks).** Superficially attractive — free tiering with zero eviction code. Rejected: the pager's eviction decisions are invisible and unpredictable, which breaks the memory contract (RFC-0001 I2 requires the engine, not the OS, to know what is resident) and would let Metal discover memory pressure mid-generation — the exact failure mode admission control exists to prevent. GPU-wired allocations do not page benignly on macOS, and a page fault in a decode-critical read path would produce ITL spikes no scheduler policy could compensate. Explicit tiering with a modeled cost function (KV18) keeps every byte accounted for.

## Drawbacks

- **Partial-block waste.** Every sequence tail wastes on average half a block per layout class; at 32-token blocks and 144 KiB/token (Qwen3-8B fp16, FE8) that is ~2.3 MiB per live sequence, and the same waste is frozen into every donated cached run. This is the direct cost of block granularity and the reason the 16-vs-32 ablation stays open.
- **Refcount/CoW complexity.** Shared mutable block tables with copy-on-write are a classic aliasing-bug breeding ground. The design answers with named invariants (I-KV1..I-KV4) and a dedicated property-test class ([Testing Strategy](#testing-strategy)), but the complexity is real and permanent.
- **The disk tier is a privacy surface.** Serialized KV blocks are a lossy-but-real encoding of user prompts at rest. KV19 mitigates (0600 permissions, sensitive-by-default documentation, exclusion from diagnostics bundles, `drakkar cache clear`, per-request opt-out per KV11), but the surface exists and is documented rather than hidden.
- **Table indirection costs until the fused kernel lands.** The v0.2 gather-based paged read path (RFC-0003 IC10, LD20 in the corpus decision registry) pays a gather penalty versus contiguous reads; the fused paged varlen kernel is a named v0.2 performance milestone, not a prerequisite.

## Migration / Rollout

| Milestone | KV subsystem state |
| --- | --- |
| v0.1 "First light" | Contiguous per-sequence KV, single sequence, no sharing — but implemented **behind the `KvPool` trait** (KV22) with `lookup_prefix` returning `None` and `seal` always freeing. Everything above the seam (scheduler, backend, admission arithmetic) is written against the trait from day one, so the v0.2 swap changes no caller. |
| v0.2 "Convoy" | Paged block pool (KV1-KV4), layout classes (KV5-KV8), prefix sharing with CoW (KV9-KV12), KV quantization (KV13-KV16), eviction policy (KV20-KV21), full metrics (KV23-KV24). `--kv-bits` / `kv_cache.bits` becomes operative. |
| v0.3 "Fleet" | SSD persistence tier (KV17-KV19). `kv_cache.disk` defaults to `true` for `serve`, `false` for one-shot `run`. Disk sidecar schema `drakkar.kvdisk/1`; any future layout change bumps the schema version and old entries are discarded, never migrated (the cache is reconstructible by definition, RFC-0001 A8). |
| v1.x | Preemption/offload under pool pressure; mixed-precision K/V evaluation; possible mlx-lm prompt-cache import path. |

Config surface: `kv_cache.bits` (`fp16` default | `8` | `4`), `kv_cache.disk` (bool), `kv_cache.disk_budget_gib` (default 8), `kv_cache.ttl_min` (default 30), CLI overrides `--kv-bits`, `--kv-pool`. Flags > env > file > defaults per the corpus-wide config precedence (RFC-0008, [CLI and UX](RFC-0008-cli-ux.md#proposed-design)).

## Testing Strategy

Acceptance criteria (release-gating, on the RFC-0009 Tier-1 fleet):

- AC1. **Prefix hit correctness**: byte-identical generations (greedy) with and without cache across the template/tool-call corpus.
- AC2. **Warm TTFT**: for an 8k cached prefix + 64 new tokens, TTFT ≤ 1.15 × (64-token prefill + c1) on the reference fleet (c1 per RFC-0004 FE23).
- AC3. **Fan-out sharing**: 4-way fan-out over a shared 8k prefix allocates the prefix blocks once (verified by pool accounting) and sustains per-stream ITL within the RFC-0009 workload-C targets ([Performance](RFC-0009-performance.md#proposed-design)).
- AC4. **Restart survival** (v0.3): kill -9 during serve, restart, re-issue — the disk-tier restore path yields ≥ 3x speedup vs cold prefill on the 8k scaffold fixture.
- AC5. **Fuzzed invalidation**: mutations to template/tokenizer/revision/KV-precision/rope-scaling never yield a stale hit (property test over the KV12 key components; I-KV3).

Additional named test classes:

- **Property — CoW non-aliasing** (`kv_cow_no_alias`): after any CoW split, writes through one block table are never observable through any other table that shared the block (I-KV4); driven by randomized share/split/write sequences.
- **Property — refcount leak detector** (`kv_refcount_conservation`): fuzzed interleavings of `admit`/`append`/`seal`/`evict` across many sequences; after full quiesce, `blocks_free == pool_blocks` and every intermediate step satisfies I-KV1/I-KV2. Runs in CI per commit and as a long-fuzz job nightly.
- **Simulation — eviction policy** (`kv_evict_sim`): the KV20 scoring function replayed against synthetic access traces (agent scaffold reuse, chat-turn churn, one-shot noise) with fixtures asserting that system-prompt-class prefixes outlive one-shot prompts and that TTL expiry precedes score-based reclaim.
- **Crash consistency — disk tier** (`kv_disk_crash`): kill the process at randomized points during the write-temp + rename sequence (KV18); on restart, assert no torn entry is ever restored and orphaned temp files are removed.
- **Fixture — capacity effect** (`kv_capacity_quant`): measured token capacity at fp16/8-bit/4-bit on uniform and hybrid reference models must match the KV16 multipliers within tolerance, and must match what `fit` reports (FE9/FE20) — one truth between predictor and pool.
- **Fixture — opt-out honored** (`kv_optout`): a `cache: false` request leaves zero `cached` blocks and zero disk entries after completion (KV11, KV19).
- **Soak**: the 24 h mixed-load soak (PRD P14) runs with pool-accounting assertions enabled; any I-KV1..I-KV4 violation fails the run.

## Open Questions

1. **Block size 32 vs 16.** 16 improves partial-prefix capture for chat turns; 32 halves table overhead and helps read coalescing. Default stays 32. Owner: abdelstark. Resolution path: named 16-vs-32 ablation in the RFC-0009 `bench` workload matrix, decided on measured warm-TTFT and table-overhead data. Target: v0.2.

## References

- Kwon et al., "Efficient Memory Management for LLM Serving with PagedAttention" (vLLM), 2023; SGLang RadixAttention, 2024
- jundot/omlx: two-tier hot/cold KV with CoW and prefix sharing on MLX (2026); Blaizzy/mlx-vlm: automatic prefix caching, KV quantization schemes and last-layer sensitivity (2026)
- mlx-lm prompt cache and server caching behavior (measured parity discussion, Mac O'Clock benchmark, Jun 2026)
- vllm-metal unified paged varlen Metal kernel (v0.2.0, Apr 2026)
- [PRD](../../PRD.md) §2.3, P6, M4; [RFC-0001](RFC-0001-architecture.md) (invariants I2, A4); [RFC-0003](RFC-0003-inference-core.md) (IC10-IC13); [RFC-0004](RFC-0004-feasibility-engine.md) (FE8-FE12, FE18, FE20, FE23); [RFC-0007](RFC-0007-api-server.md) (AS8, AS21); [RFC-0009](RFC-0009-performance.md) (workloads, calibration, Tier-1 targets)
