# RFC-0005: KV Cache Subsystem

**Status:** Draft
**Author:** A. Bakhta
**Created:** 2026-07-14
**Requires:** RFC-0001, RFC-0003, RFC-0004
**Related:** RFC-0007 (scheduler)

## 1. Summary

The KV cache is where agentic workloads are won: a coding agent replays a near-identical multi-kilotokens scaffold dozens of times per session, and sub-agents fan out over a shared context. This RFC specifies a paged, quantization-aware, prefix-sharing KV pool with an optional SSD persistence tier. Goals, in order: (1) never breach the memory contract, (2) make warm TTFT proportional to the *new* tokens only, (3) keep decode reads cheap at long context, (4) survive process restarts for expensive prefixes.

Prior art acknowledged and built upon: vLLM's PagedAttention block model, SGLang's radix prefix reuse, mlx-lm's prompt cache, oMLX's two-tier RAM/SSD design with copy-on-write, and mlx-vlm's block prefix caching and KV quantization findings.

## 2. Block pool

- KV1. KV state lives in fixed-size **blocks** of 32 tokens (build-time constant; 16/64 evaluated in RFC-0009 ablation). A block stores K and V for all layers of its layout class, contiguous per layer for coalesced reads.
- KV2. The pool is allocated up-front from the model's memory contract at load: `pool_bytes = usable − weights − activation_watermark − fixed` (RFC-0004), carved into blocks; growth beyond the pool is impossible by construction (invariant I2). A `--kv-pool` override may shrink it.
- KV3. Sequences map logical positions to physical blocks via per-sequence block tables; attention kernels read through the table (RFC-0003 IC10). Table metadata cost is charged by the fit engine (FE12).
- KV4. Block states: `free`, `active(seq)`, `cached(prefix, refcount)`. Refcounted sharing enables CoW: a shared block splits only when a sequence writes into it (partial tail block), copying once.

## 3. Layout classes (architecture-aware)

- KV5. **Global attention layers**: paged as above.
- KV6. **Sliding-window layers** (Gemma-class hybrids, gpt-oss alternates): fixed ring buffers of `window` tokens per sequence, allocated per-sequence outside the paged pool (their size does not scale with context; paging them wastes table overhead). Attention-sink slots (first tokens pinned) supported per model config.
- KV7. **MLA layers** (latent KV): paged, but blocks store the shared `(c_kv + d_rope)` latent per token; the attention path consumes the latent form directly. Accounting per FE10.
- KV8. **Recurrent/SSM state**: constant per-sequence tensors owned alongside the ring buffers; snapshot/restore supported for prefix reuse at segment boundaries only.

## 4. Prefix sharing

- KV9. Prefix identity is a rolling hash chain over `(model_id, tokenizer_hash, chat_template_hash, token_ids)` computed at block granularity; the index is a radix-tree keyed by hash-chain segments, mapping to cached block runs.
- KV10. On admission, the scheduler queries the longest cached prefix; prefill starts at the first uncached token. Partial-block matches reuse up to the block boundary (tail recomputed). Target effect: a repeated 8k-token scaffold that costs a cold prefill of tens of seconds on M-class laptops collapses to seconds; parity with mlx-lm's warm path is the floor, cross-conversation and cross-restart reuse is the differentiator.
- KV11. Sharing works across concurrent sequences (fan-out agents attach to the same blocks with refcounts) and across time (completed sequences donate their prefix run to `cached` state instead of freeing, subject to §8 policy).
- KV12. Correctness keys: any change in model revision, tokenizer, template, KV precision, or rope scaling invalidates the affected subtree; keys are content hashes, never names.

## 5. KV quantization

- KV13. Supported precisions per model: fp16 (default), 8-bit, and 4-bit group-quantized (group 64, per-head scales); configured at load (`--kv-bits`) and reported by `fit` (FE20 shows the context each precision buys).
- KV14. Quantization is applied at block granularity on write; attention kernels consume quantized blocks natively (dequant fused in the kernel), so there is no resident fp16 shadow copy.
- KV15. Sensitivity defaults: the final full-attention layer stays fp16 when `--kv-bits <= 8` on deep models (empirical quality cliff documented by mlx-vlm); model recipes may extend the exempt set. Mixed schemes (for example 3-bit keys / 4-bit values) are a v1.x evaluation item, not v1.
- KV16. Expected capacity effect (uniform-attention models): ~2x tokens at 8-bit, ~3.5x at 4-bit net of scale overhead; hybrid models see less because SWA terms dominate (fit engine reports the true number, FE9).

## 6. SSD persistence tier

- KV17. Optional (`kv_cache.disk = true`, default on for `serve`, off for one-shot `run`): evicted `cached` prefix runs are serialized to `~/.drakkar/kv-cache/` as safetensors blocks with an index sidecar; restored runs skip prefill after process restarts (oMLX precedent).
- KV18. Writes are async on the blocking pool (RFC-0001 A4), throttled, and crash-safe (write-temp + rename); restore streams at SSD bandwidth and MUST beat recompute by ≥ 3x for the block run to be eligible (cost model: bytes/ssd_bw vs tokens/prefill_tps, using calibrated numbers).
- KV19. Disk budget: default 8 GiB, LRU within it; `drakkar cache ls|clear` manages it. Cache files are mode 0600; contents are user prompts and MUST be treated as sensitive (documented, excluded from any diagnostics bundle by default).

## 7. Eviction and retention

- KV20. RAM `cached` blocks are reclaimed under a cost-aware LRU: score = recompute_cost × recency_decay ÷ bytes; system-prompt-class prefixes (high reuse count) resist eviction. TTL default 30 min, configurable.
- KV21. Reclaim order under pool pressure: expired TTL → lowest-score cached → (never) active blocks. If active demand alone exceeds the pool, admission control blocks new requests (FE18); running sequences are never preempted in v1 (preemption/offload is a v1.x scheduler feature).

## 8. Interfaces and observability

- KV22. `KvPool` trait surface (consumed by scheduler and backend): `admit(seq, tokens_needed) -> Reservation | Rejection{max_admissible}`, `lookup_prefix(hash_chain) -> CachedRun`, `append(seq, block)`, `seal(seq) -> donate`, `evict(policy)`, `stats()`.
- KV23. Metrics (Prometheus, RFC-0007 §9): pool occupancy by state, prefix hit rate (tokens served from cache / prompt tokens), warm-vs-cold TTFT histograms, evictions by reason, disk tier hit rate and restore bandwidth.
- KV24. `drakkar ps` shows per-model pool occupancy and hit rate; `--json` includes the full stats struct.

## 9. Acceptance criteria

- AC1. Prefix hit correctness: byte-identical generations (greedy) with and without cache across the template/tool-call corpus.
- AC2. Warm TTFT for an 8k cached prefix + 64 new tokens ≤ 1.15 × (64-token prefill + c1) on the reference fleet.
- AC3. 4-way fan-out over a shared 8k prefix allocates the prefix blocks once (verified by pool accounting) and sustains per-stream ITL within the RFC-0009 concurrency targets.
- AC4. Kill -9 during serve, restart, re-issue: disk-tier restore path yields ≥ 3x speedup vs cold prefill on the 8k scaffold fixture.
- AC5. Fuzzed invalidation: mutations to template/tokenizer/revision never yield a stale hit (property test on KV12 keys).

## Open questions

1. Block size 32 vs 16: 16 improves partial-prefix capture for chat turns; 32 halves table overhead and helps coalescing. Decide on RFC-0009 ablation data.
2. Should `seal/donate` be opt-out per request (`cache: false` in API) for privacy-sensitive callers even in RAM? (Leaning yes, cheap to honor.)
3. Cross-model sharing for identical tokenizer+prefix (draft and target in speculation share nothing today; the draft's KV is small enough that this may not matter).

## References

- Kwon et al., "Efficient Memory Management for LLM Serving with PagedAttention" (vLLM), 2023; SGLang RadixAttention, 2024
- jundot/omlx: two-tier hot/cold KV with CoW and prefix sharing on MLX (2026); Blaizzy/mlx-vlm: automatic prefix caching, KV quantization schemes and last-layer sensitivity (2026)
- mlx-lm prompt cache and server caching behavior (measured parity discussion, Mac O'Clock benchmark, Jun 2026)
- vllm-metal unified paged varlen Metal kernel (v0.2.0, Apr 2026)
