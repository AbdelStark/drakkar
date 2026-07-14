# 03. Data Model and Schemas

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Scope: canonical in-memory types, on-disk schemas, identifier conventions, schema versioning, and the named invariants every DRAKKAR subsystem assumes.

This document is the single home for the load-bearing types of the system. It defines the
shapes; the RFCs define the behavior around them. Rust sketches give signatures, field types,
and confinement properties — not implementations. Where a type mirrors an on-disk or wire
schema, the two are specified together and MUST NOT drift (see [INV-MIRROR](#5-named-invariants)).

Sources of record: RFC-0001 §5 and §7 ([Architecture](../rfcs/RFC-0001-architecture.md#proposed-design)),
RFC-0004 FE26 ([Feasibility Engine](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)),
RFC-0005 ([KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)),
RFC-0006 MP10 ([Model Pipeline](../rfcs/RFC-0006-model-pipeline.md#proposed-design)),
RFC-0008 CLI10 ([CLI and UX](../rfcs/RFC-0008-cli-ux.md#proposed-design)),
RFC-0009 PB15 ([Performance](../rfcs/RFC-0009-performance.md#proposed-design)),
and the [PRD](../../PRD.md).

---

## 1. Placement and ownership

- DM1. All types in §3 and the `DkError` taxonomy live in the `drakkar-core` crate (workspace
  layout per RFC-0002). Every other crate imports them; no crate redefines, duplicates, or
  wraps them in a parallel hierarchy. A type change is a `drakkar-core` change, reviewed as such.
- DM2. Types in this document are backend-neutral. None of them may name, embed, or leak
  Metal, MLX, or llama.cpp types (RFC-0001 I5). Backend-specific state lives behind opaque
  handles (`ModelHandle`, `BlockTableRef`, `SamplerStateRef`, `MaskRef`) whose contents only
  the owning backend interprets.
- DM3. Fields whose values are memory or performance figures (weight bytes, KV bytes/token,
  context ceilings, throughput estimates) are computed exclusively by `drakkar-fit`
  (RFC-0001 I3). Other crates carry these values; they never recompute them.

## 2. Conventions

### 2.1 Units, identifiers, hashes

- DM4. In-memory sizes are `u64` bytes. JSON schemas report memory in GiB (`*_gib`, f64) and
  per-token KV in KiB (`kv_per_token_kib`), matching RFC-0004 FE26. Timestamps are ISO 8601
  UTC strings on disk, `SystemTime` in memory. Durations in config are suffixed strings
  (`"30m"`, `"90s"`).
- DM5. Content digests are SHA-256, rendered as `sha256-<64 hex chars>`. Blob file names in
  the store are exactly this rendering (RFC-0006 MP10).

```rust
pub struct Sha256(pub [u8; 32]);          // renders as "sha256-<hex>"
pub struct RequestId(pub Ulid);           // unique per process lifetime; appears in logs/traces (AS22)
pub struct SeqId(pub u64);                // process-local, monotonically assigned per admitted sequence
pub struct BlockId(pub u32);              // index into the physical block pool of one engine
pub struct SchemaTag(pub &'static str);   // e.g. "drakkar.fit/1"; parse: name + '/' + major
```

- DM6. Prefix identity (RFC-0005 KV9) is a BLAKE3 hash chain at block granularity:

```text
h_0 = blake3(model_digest || tokenizer_hash || chat_template_hash || kv_bits || rope_params)
h_i = blake3(h_{i-1} || token_ids[block i])       // block i = tokens [i*B, (i+1)*B), B = block_tokens
```

  The seed `h_0` folds in every KV12 correctness key, so any change to model revision,
  tokenizer, template, KV precision, or rope scaling changes every hash in the chain and
  invalidates by construction. The chain is stable across process restarts and DRAKKAR
  versions within a `drakkar.kvcache-index` major (§4.4); `block_tokens` is recorded in the
  sidecar keys so a future block-size change (LD7 ablation) misses cleanly instead of
  corrupting.

```rust
pub struct PrefixHash(pub [u8; 32]);
pub struct PrefixHashChain(pub Vec<PrefixHash>);   // one entry per full block of the prompt
```

### 2.2 Schema versioning (applies to every schema in §4 and every JSON surface)

- DM7. Every schema-bearing file and JSON payload carries a top-level version field:
  `"schema": "drakkar.<name>/<major>"`. For `config.toml` the key is `schema = "drakkar.config/1"`
  and MAY be absent (absence reads as major 1, for files predating the key).
- DM8. Within a major version, evolution is additive only: new optional fields MAY be added;
  existing fields MUST NOT be removed, renamed, retyped, or change meaning. Readers MUST
  ignore unknown fields within a known major. Writers always emit the newest minor shape.
- DM9. Readers MUST reject a file or payload whose major exceeds the newest major they
  implement, rather than silently best-effort parsing it. For the user-owned `config.toml`
  this is the named error `config.invalid_value` (registry:
  [04-error-model.md](04-error-model.md)); the message names the file, the found major, the
  supported major, and the remedy (upgrade DRAKKAR, or regenerate the file with the current
  version). Store-managed schema files (§4.2–§4.4) are reconstructible (DM34), so a
  newer-major store file is regenerated on next write and reported by `drakkar doctor`, not
  surfaced as a hard error. Silent best-effort parsing of a newer major is forbidden either
  way.
- DM10. A major bump is a deliberate migration event: the writer of the new major MUST also
  read the previous major and migrate on first touch (documented per schema in its owning
  RFC's Migration section). Majors are bumped only when additive evolution is impossible.

## 3. Canonical in-memory types

### 3.1 `GenerationRequest` — the normalized, dialect-free request

Both HTTP dialects (RFC-0007 AS1), the CLI REPL, and the desktop shim normalize into this
one struct before anything touches the scheduler. Everything below the session layer is
dialect-blind.

```rust
pub struct GenerationRequest {
    pub id: RequestId,
    pub model: ModelSelector,                 // Resident(name) | Installed(name) | Default (AS3)
    pub prompt: TokenizedPrompt,              // chat template rendered, tools serialized (MP18), then tokenized
    pub prefix_chain: PrefixHashChain,        // computed at tokenize time for cache lookup (KV9, RFC-0001 §6)
    pub sampling: SamplerParams,
    pub limits: RequestLimits,
    pub structured: Option<CompiledGrammar>,  // llguidance-compiled from json_schema/regex/lark (IC16, AS10)
    pub tools: Option<ToolContext>,           // declared tools + family dialect driving stream parsing (AS9)
    pub cache: CachePolicy,                   // see below
    pub priority: Priority,                   // Interactive | Batch (AS15)
    pub stream: bool,
    pub logprobs: Option<u8>,                 // top-k logprobs; None = no logprob readback (IC4)
    pub hide_reasoning: bool,                 // server-level override (AS11)
    pub render: RenderTarget,                 // OpenAiChat | OpenAiLegacy | Anthropic | Cli — response rendering ONLY
}

pub struct SamplerParams {
    pub temperature: f32,                     // 0.0 short-circuits to greedy (IC14)
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
    pub min_p: Option<f32>,
    pub presence_penalty: f32,
    pub frequency_penalty: f32,
    pub repetition_penalty: f32,
    pub seed: Option<u64>,                    // counter-based RNG per request (IC14, LD6 determinism note)
    pub logit_bias: Vec<(TokenId, f32)>,
}

pub struct RequestLimits {
    pub max_tokens: u32,                      // already clamped to model context; clamp sets finish_reason (AS7)
    pub stop_strings: Vec<String>,            // matched across token boundaries in Rust (IC17)
    pub stop_token_ids: Vec<TokenId>,
}

pub struct CachePolicy {
    pub donate: bool,                         // false = never donate blocks to cache, even in RAM (LD8)
    pub hints: Vec<CacheHint>,                // explicit cache_control honored as hints, never required (LD17)
}
```

- DM11. `render` MUST NOT be read by any code below the session/render layer. The scheduler,
  fit engine, KV pool, and backends operate on `GenerationRequest` fields that carry no
  dialect information (enforced by [INV-DIALECT](#5-named-invariants); lint: `render` is
  `pub(crate)` to the server/CLI crates via a newtype re-export, not visible to
  `drakkar-sched`/`drakkar-engine`).
- DM12. `prompt` holds token ids plus the byte spans needed for incremental tool/reasoning
  parsing; the raw untokenized message list does not travel past normalization. Tokenization
  runs on the blocking pool (RFC-0001 A4) before the request reaches admission.

### 3.2 `ModelArtifact` and `ModelHandle`

`ModelArtifact` is the model manager's output (RFC-0006): everything a backend needs to load,
resolved to immutable blobs. `ModelHandle` is the backend's receipt: opaque above the seam,
thread-confined below it.

```rust
pub struct ModelArtifact {
    pub digest: Sha256,                       // digest of the manifest body; identity of the artifact
    pub manifest_path: PathBuf,               // ~/.drakkar/models/manifests/<org>/<repo>/<rev>.json (§4.2)
    pub format: ArtifactFormat,               // MlxSafetensors | Safetensors | Gguf
    pub quant: QuantDesc,                     // scheme, bits, group, bpw_eff, recipe id (FE5/FE6, IC6)
    pub arch: ArchDescriptor,                 // parsed config.json: layers, hidden, heads, kv_heads,
                                              // head_dim, vocab, MoE topology, attention layout classes,
                                              // MLA dims, sliding window layout (FE1)
    pub weights: Vec<BlobRef>,                // ordered for mmap; BlobRef = { digest: Sha256, bytes: u64, name: String }
    pub tokenizer: BlobRef,
    pub tokenizer_hash: Sha256,               // feeds KV12 keys (MP17)
    pub chat_template: BlobRef,
    pub chat_template_hash: Sha256,           // hash AFTER override-table patching (MP18)
    pub tool_dialect: ToolDialect,            // per model family; drives render + stream parse (AS9)
    pub advertised_ctx: u32,
}

pub struct ModelHandle {
    pub instance: u64,                        // unique per load within the process
    pub artifact: Sha256,                     // ties the handle to the exact artifact digest
    pub budget: MemoryBudget,                 // the contract this instance was loaded under
    _confined: PhantomData<*const ()>,        // !Send + !Sync: never leaves the engine thread (RFC-0001 A2)
}
```

- DM13. `ModelHandle` is `!Send`/`!Sync` by construction. All uses occur on the engine actor
  thread; the scheduler holds only `instance` ids in its bookkeeping, never the handle.
- DM14. `ArchDescriptor.layout_classes` enumerates, per layer, one of the four KV layout
  classes of RFC-0005 §3 (`Global`, `SlidingWindow { window, sinks }`, `MlaLatent { c_kv, d_rope }`,
  `Recurrent { state_bytes }`). Both the fit engine (FE8–FE11) and the KV pool key their
  arithmetic and allocation off this single enumeration — the same field, not two parses of
  `config.json`.

### 3.3 `MemoryBudget` and `MemoryReport`

The memory contract (RFC-0001 I2) as data. `MemoryBudget` is declared once at load, computed
by `drakkar-fit`; `MemoryReport` is the backend's measured answer (RFC-0001 §5
`memory_report()`, IC25).

```rust
pub struct MemoryBudget {
    pub declared: u64,                        // total contract; never exceeded (I2)
    pub weights: u64,
    pub kv_pool: u64,                         // carved into blocks up front (KV2)
    pub activation_watermark: u64,            // bounded by chunk size (IC13, FE13)
    pub runtime_overhead: u64,                // FE14: shipped 1.2 GiB default, calibrated floor
    pub draft_model: u64,                     // 0 unless speculation with a draft (IC19)
    pub fragmentation_margin: u64,            // 3% of the above (RFC-0004 §5)
}
// invariant of construction: declared == sum of the component fields

pub struct MemoryReport {
    pub actual: u64,                          // measured resident engine footprint
    pub declared: u64,                        // echo of the contract
    pub breakdown: MemoryBreakdown,           // weights / kv (by state) / activations / allocator cache
    pub metal_recommended_working_set: u64,   // live probe echo (FE15, IC25)
    pub wired_limit_mb: u32,                  // current iogpu.wired_limit_mb (FE17)
}
```

- DM15. `MemoryReport.actual <= MemoryBudget.declared` at every step boundary — this is
  [INV-BUDGET](#5-named-invariants). Debug and soak builds assert it after every
  prefill chunk and decode step (RFC-0004 AC4, RFC-0009 PB4: a breach is a hard failure,
  never a slow result).

### 3.4 `Capabilities`

Filled by the load-time probe (IC26), gates features at runtime (RFC-0001 A7), and feeds the
fit engine's constants.

```rust
pub struct Capabilities {
    pub chip: ChipId,                         // identity + GPU core count (IOKit/sysctl)
    pub bandwidth_gbs: f32,                   // probed or from the FE2 fallback table
    pub macos: (u16, u16),                    // major, minor
    pub nax_tensor_ops: bool,                 // functional self-test result, never version sniffing (IC26)
    pub kv_bits: Vec<u8>,                     // supported KV precisions, e.g. [16, 8, 4] (KV13)
    pub spec_decode: SpecDecodeSupport,       // ngram: bool, draft: bool (IC18/IC19)
    pub paged_attention: PagedPath,           // GatherFallback | FusedVarlen (IC10, LD20)
    pub max_batch: u32,                       // largest decode batch the backend accepts
}
```

- DM16. `Capabilities` is the only sanctioned way a feature learns whether it may run.
  Feature code MUST NOT probe hardware or macOS versions itself; one probe, one struct,
  every consumer (fit constants per A7, scheduler speculation gating per IC21/AS16,
  bench NAX gate per PB14).

### 3.5 `FitReport` — mirror of `drakkar.fit/1`

The FE26 JSON schema, as the Rust struct it serializes from. One struct renders both the
human report card and `--json` (RFC-0008 CLI6); `POST /fit` returns the same body (AS20).

```rust
pub struct FitReport {
    pub schema: SchemaTag,                    // "drakkar.fit/1"
    pub model: FitModel,                      // id, arch, params_total, params_active, quant: QuantDesc
    pub machine: FitMachine,                  // chip, ram_gib, budget_gib, budget_source: Probe|Table,
                                              // bandwidth_gbs, nax, wired_limit_mb
    pub memory: FitMemory,                    // weights_gib, kv_per_token_kib, kv_at_ctx_gib,
                                              // activation_gib, runtime_gib, total_gib, confidence
    pub verdict: Verdict,                     // Comfortable | Tight | NeedsTuning | WontFit (FE19)
    pub headroom_gib: f64,
    pub context: FitContext,                  // requested, max_fp16, max_kv8, max_kv4, advertised (FE20)
    pub performance: FitPerformance,          // decode_tps: Estimate<f64>, ttft_cold_s: Estimate<f64>, load_s: f64
    pub remedies: Vec<Remedy>,                // ranked per FE19; each carries the exact command/flag
}

pub enum Confidence { Measured, Calibrated, Modeled }        // FE24 tiers, printed with every number
pub struct Estimate<T> { pub value: T, pub confidence: Confidence }

pub struct Remedy {
    pub rank: u8,                             // FE19 order: official quant, on-device quant, KV 8-bit,
                                              // reduced context, KV < 8-bit, wired-limit raise (opt-in)
    pub kind: RemedyKind,
    pub command: String,                      // copy-pasteable, e.g. "drakkar pull qwen3:8b --quant 4bit-g64"
    pub effect: String,                       // predicted outcome in domain terms
}
```

- DM17. `FitReport` serializes to exactly the FE26 JSON shape (field names, nesting, units).
  A golden-fixture test round-trips the FE26 example verbatim; any divergence between struct
  and schema is a build break, not a runtime surprise ([INV-MIRROR](#5-named-invariants)).

### 3.6 KV pool types and the `KvPool` trait

The block/pool structures of RFC-0005 §2–§3 and the KV22 trait surface, consumed by both the
scheduler (admission, prefix lookup) and the backend (attention reads through block tables).

```rust
pub const BLOCK_TOKENS: u32 = 32;             // build-time constant (KV1, LD7; 16-vs-32 is an RFC-0009 v0.2 ablation)

pub enum BlockState {
    Free,
    Active(SeqId),
    Cached { prefix: PrefixHash, refcount: NonZeroU32 },     // refcounted CoW sharing (KV4)
}

pub struct BlockTable {                       // per-sequence logical→physical map (KV3)
    pub seq: SeqId,
    pub blocks: Vec<BlockId>,                 // logical position p lives in blocks[p / BLOCK_TOKENS]
    pub tail_len: u32,                        // tokens written into the last block (partial tail, KV10)
}

pub enum LayoutClass {                        // per-layer, from ArchDescriptor (DM14; KV5–KV8)
    Global,                                   // paged
    SlidingWindow { window: u32, sinks: u32 },// per-sequence ring buffer outside the pool (KV6)
    MlaLatent { c_kv: u32, d_rope: u32 },     // paged; blocks store the shared latent (KV7)
    Recurrent { state_bytes: u64 },           // constant per-sequence state (KV8)
}

pub trait KvPool {
    fn admit(&mut self, seq: SeqId, tokens_needed: u32) -> Result<Reservation, Rejection>;
    fn lookup_prefix(&self, chain: &PrefixHashChain) -> CachedRun;
    fn append(&mut self, seq: SeqId, block: BlockId) -> Result<(), DkError>;
    fn seal(&mut self, seq: SeqId, policy: DonatePolicy) -> SealOutcome;   // donate vs free per CachePolicy (KV11, LD8)
    fn evict(&mut self, policy: EvictPolicy) -> EvictOutcome;              // TTL → lowest score → never active (KV20/KV21)
    fn stats(&self) -> KvStats;
}

pub struct Reservation { pub seq: SeqId, pub blocks_reserved: u32 }        // covers prompt + max_tokens (AS14)
pub struct Rejection  { pub max_admissible: u32, pub occupancy: PoolOccupancy }   // feeds 429/413 bodies (FE18, AS8)

pub struct CachedRun {                        // longest cached prefix for a chain (KV10)
    pub matched_blocks: u32,                  // full-block matches only; tail is recomputed
    pub blocks: Vec<BlockId>,                 // to attach with refcount increments
}

pub struct KvStats {                          // KV23/KV24 surface; serializes into `drakkar ps --json`
    pub blocks_total: u32,
    pub blocks_free: u32,
    pub blocks_active: u32,
    pub blocks_cached: u32,
    pub prefix_hit_tokens: u64,               // cumulative tokens served from cache
    pub prompt_tokens: u64,                   // cumulative; hit rate = prefix_hit_tokens / prompt_tokens
    pub evictions: EvictionCounters,          // by reason: ttl, pressure, invalidation
    pub disk: Option<DiskTierStats>,          // hit rate, restore bandwidth (KV23)
}
```

- DM18. The pool never grows: all blocks are carved from `MemoryBudget.kv_pool` at load
  (KV2); `admit` is the only path that can say no, and it says no with the computable
  `max_admissible` — Metal never discovers exhaustion
  ([INV-NO-GROWTH](#5-named-invariants), RFC-0001 design principle 2).
- DM19. Refcount discipline: a `Cached` block's refcount equals the number of attached live
  sequences plus one for cache retention. `seal` with `DonatePolicy::Free` (the LD8 opt-out
  path) MUST NOT leave the block in `Cached` state. After quiesce, pool accounting satisfies
  `blocks_total == blocks_free + blocks_active + blocks_cached` exactly (verified by the
  RFC-0007 AC4 disconnect-storm test).

### 3.7 Execution batch types: `PrefillChunk`, `DecodeBatch`, `TokenOut`

The message vocabulary of the engine actor loop (RFC-0001 A2/§5). Step-granular by design:
policy stays in Rust, only math crosses the seam (A6).

```rust
pub struct PrefillChunk {
    pub seq: SeqId,
    pub tokens: Vec<TokenId>,                 // len <= chunk budget: default 512, adaptive 256..=2048 (IC12, AS13)
    pub position_offset: u32,                 // absolute position of tokens[0]; starts after the cached prefix (KV10)
    pub block_table: BlockTableRef,           // opaque; backend reads K/V placement through it (KV3, IC10)
    pub is_last: bool,                        // final chunk → sequence joins the decode batch (AS12)
}

pub struct DecodeBatch { pub entries: Vec<DecodeEntry> }     // one step, B sequences (RFC-0001 §5)

pub struct DecodeEntry {
    pub seq: SeqId,
    pub last_token: TokenId,
    pub position: u32,
    pub block_table: BlockTableRef,
    pub sampler: SamplerStateRef,             // per-sequence on-device penalty/RNG state (IC14)
    pub grammar_mask: Option<MaskRef>,        // vocab bitset, uploaded only for constrained requests (IC16)
    pub draft: Option<DraftTokens>,           // speculative tokens to verify this step (IC18/IC19)
}

pub struct TokenOut {
    pub seq: SeqId,
    pub tokens: SmallVec<[TokenId; 4]>,       // >1 only when speculation accepts a run (IC18/IC19)
    pub logprobs: Option<Vec<TopLogprob>>,    // present only when requested; top-k computed on-GPU (IC4)
    pub accepted_draft: u8,                   // 0 when speculation off/rejected
    pub finish: Option<FinishReason>,         // Stop | Length | StopString | Eos | Cancelled
}
```

- DM20. Readback per step is `TokenOut` only — sampled ids plus optional top-k logprobs —
  never full logits (IC4). Stop-string matching, grammar advance, and tool-call parsing
  consume `TokenOut` in Rust on the scheduler side (IC17); the backend does not know what a
  stop string is.

### 3.8 `DkError`

The shared error type. The full code registry, category/exit/HTTP mappings, and message
templates are [04-error-model.md](04-error-model.md) (the normative registry) and
[RFC-0011](../rfcs/RFC-0011-error-taxonomy.md) (ER* decision record); this section fixes
the shape. It is a **flat struct**, not an enum-of-variants: the failure class lives in the
`category` field and the specific error in the closed `ErrorCode` enum, so there is no
per-subsystem sub-error hierarchy.

```rust
pub struct DkError {
    pub code: ErrorCode,          // closed enum; the registry (04 §4) IS this enum
    pub category: ErrorCategory,  // Usage | ModelNotFound | Infeasible | Network | Format | Engine | Disk | Internal
    pub message: String,          // domain terms (memory, download, format, engine); no secrets (RFC-0001 principle 6)
    pub remedy: Option<Remedy>,   // error-taxonomy remedy (below); distinct from the FitReport Remedy of §3.5
    pub retry: Retry,             // explicit retry semantics
    pub context: ErrorContext,    // typed per-code key/value fields, additive-only
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>, // cause chain; logs/--verbose only, never serialized
}

impl DkError {
    pub fn code(&self) -> ErrorCode;          // stable dotted code via ErrorCode::as_str(), e.g. "fit.wont_fit"
    pub fn category(&self) -> ErrorCategory;
    pub fn exit_code(&self) -> u8;            // category → CLI exit (04 §2): usage 2, model_not_found 3,
                                              // infeasible 4, network 5, format 6, engine 6, disk 7, internal 6
    pub fn http_status(&self) -> StatusCode;  // category default + per-code override (04 §2/§3)
    pub fn remedy(&self) -> Option<&Remedy>;  // the single most useful next action (CLI15)
}

// The registry (04 §4) is exactly this closed enum; each variant renders to a stable
// &'static str in `subsystem.snake_case` form via as_str(). An unregistered code is
// unrepresentable — the compiler cannot construct it.
pub enum ErrorCode { /* one variant per 04 §4 row, e.g. FitWontFit, KvPoolExhausted, InternalPanic */ }

pub enum Retry { Terminal, AfterBackoff, After { after_ms: u64 } }

// The error-taxonomy remedy — NOT the FitReport::Remedy of §3.5 (which carries rank/kind/
// command/effect for the fit report card). This one is the rendered next action.
pub struct Remedy { pub rendered: String, pub template: &'static str, pub params: ErrorContext }
```

- DM21. Every `DkError` value carries: a stable dotted code (never renamed within a major
  release line), a message that names the cause in domain terms (memory, download, format,
  engine — RFC-0001 design principle 6), and where possible a remedy command. HTTP handlers
  map the same value to the structured bodies of RFC-0007 AS8 (`413 fit.context_exceeded`
  with `max_admissible_tokens`, `429 kv.pool_exhausted` with `retry_after_ms`, `503
  server.model_loading` with progress); the CLI maps it to the CLI8 exit code. One error
  value, three renderings.

## 4. On-disk schemas

### 4.1 Schema registry

- DM22. Every persistent file DRAKKAR writes, and every versioned JSON surface it emits,
  registers here. Adding a schema is a change to this table.

| Schema tag | File / surface | Owner RFC | First ships |
| --- | --- | --- | --- |
| `drakkar.fit/1` | `fit --json`, `POST /fit` | RFC-0004 FE26 | v0.1 |
| `drakkar.manifest/1` | `~/.drakkar/models/manifests/<org>/<repo>/<rev>.json` | RFC-0006 MP10 | v0.1 |
| `drakkar.config/1` | `~/.config/drakkar/config.toml` | RFC-0008 CLI10 | v0.1 |
| `drakkar.<cmd>/1` | `--json` of every CLI command (`ls`, `ps`, `doctor`, ...) | RFC-0008 CLI6 | v0.1 |
| `drakkar.error/1` | error object on any `--json` failure / `--stream-json` `error` event | RFC-0011 | v0.1 |
| `drakkar.calibration/1` | `~/.drakkar/calibration/<chip>.json` | RFC-0009 PB15 | v0.2 |
| `drakkar.bench/1` | `bench --json` result body | RFC-0009 PB8 | v0.2 |
| `drakkar.kvcache-index/1` | `~/.drakkar/kv-cache/index.json` + run sidecars | RFC-0005 KV17 | v0.3 |

All follow the §2.2 rule: `"schema": "drakkar.<name>/<major>"`, additive-only within a
major. A newer-major `config.toml` is rejected with `config.invalid_value` (DM9);
reconstructible store files (§4.2–§4.4) are regenerated rather than error-surfaced. The
`drakkar.error/1` tag names the error JSON *object* (04 §8); it is distinct from
`drakkar.errors/1`, the version of the error-code *registry* contract, which never appears
in a `schema` field.

### 4.2 Model manifest — `drakkar.manifest/1`

Content-addressed store per RFC-0006 MP10: blobs at `~/.drakkar/models/blobs/sha256-*`,
manifests mapping names to blobs. Identical tensors dedupe across revisions by construction.

```json
{
  "schema": "drakkar.manifest/1",
  "repo_id": "Qwen/Qwen3-8B",
  "revision": "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b",
  "source": { "kind": "hf", "url": "https://huggingface.co/Qwen/Qwen3-8B",
              "resolved_at": "2026-07-14T09:30:00Z" },
  "origin": "download",
  "parent": null,
  "format": "mlx_safetensors",
  "quant": { "scheme": "mlx_affine", "bits": 4, "group": 64, "bpw_eff": 4.5,
             "recipe": "qwen3-default/1" },
  "arch": { "name": "qwen3", "config_blob": "sha256-9f...", "params_total": 8190000000,
            "params_active": 8190000000, "advertised_ctx": 131072 },
  "files": [
    { "name": "model-00001-of-00002.safetensors", "blob": "sha256-3c...",
      "bytes": 2415919104, "role": "weights" },
    { "name": "tokenizer.json", "blob": "sha256-7e...", "bytes": 7031357, "role": "tokenizer" },
    { "name": "chat_template.jinja", "blob": "sha256-a1...", "bytes": 4096, "role": "template" }
  ],
  "hashes": { "tokenizer": "sha256-7e...", "chat_template_effective": "sha256-b2...",
              "config": "sha256-9f..." },
  "created": "2026-07-14T09:41:12Z",
  "last_used": "2026-07-14T11:02:44Z"
}
```

- DM23. `revision` is always a resolved commit hash, never a branch or tag name — cache keys
  and dedupe depend on it (KV12: content hashes, never names).
- DM24. `origin` is one of `download`, `hf_clone` (hard-link/clonefile from the HF cache,
  MP11/LD4), `converted` (on-device pipeline, MP13), `local_import`. `converted` manifests
  set `parent` to the source manifest's relative path — provenance is part of "honest speed"
  (MP5): the chosen route is always reconstructible from the store.
- DM25. `hashes.chat_template_effective` is the hash after any curated override patch
  (MP18) — it is the value that feeds KV cache keys, so a template fix invalidates stale
  caches exactly as it must.
- DM26. `drakkar rm` deletes manifests and garbage-collects blobs unreferenced by any
  manifest (MP12). A blob referenced by any manifest MUST NOT be collected; GC verifies
  digest-on-read and quarantines (renames aside, reports via `doctor`) any blob whose content
  does not match its name.

### 4.3 Calibration file — `drakkar.calibration/1`

Written by `drakkar bench --calibrate` (RFC-0009 PB15), read by `drakkar-fit`, which prefers
calibrated values over shipped defaults and labels predictions `calibrated` (FE24).

```json
{
  "schema": "drakkar.calibration/1",
  "chip": "Apple M4 Pro",
  "gpu_cores": 20,
  "macos": "26.2",
  "drakkar_version": "0.2.0",
  "measured_at": "2026-07-14T10:00:00Z",
  "eta_d": { "dense": 0.78, "moe": 0.71 },
  "prefill_anchors": [
    { "arch_class": "dense-8b", "tps": 512.0, "nax": false },
    { "arch_class": "moe-a3b", "tps": 1370.0, "nax": true }
  ],
  "nax_multiplier": 3.4,
  "runtime_overhead_gib": 1.05,
  "activation_watermarks": [
    { "arch_class": "dense-8b", "chunk_tokens": 512, "gib": 0.40 }
  ],
  "spec_crossover_occupancy": 4,
  "ssd_read_gbs": 6.2,
  "repro": { "machine": "Mac16,8", "macos_build": "26C90", "mlx_pin": "0.31.0",
             "model_hashes": ["sha256-3c..."] }
}
```

- DM27. Field semantics bind to their RFC definitions: `eta_d` is FE21's decode kernel
  efficiency per model class; `prefill_anchors` and `nax_multiplier` feed FE22;
  `runtime_overhead_gib` replaces the FE14 shipped default; `activation_watermarks` replace
  FE13 defaults; `spec_crossover_occupancy` is the IC21 batching/speculation crossover the
  scheduler reads (AS16); `ssd_read_gbs` prices the KV disk-tier restore eligibility rule
  (KV18) and load-time estimates (FE23).
- DM28. `repro` is the LD18 reproducibility manifest (machine identifier, macOS build,
  compute-core pin, model hashes). Any published number derived from a calibration file MUST
  cite it. The fit engine ignores (with a `doctor` warning, not an error) a calibration file
  whose `chip` mismatches the probe or whose `drakkar_version` major differs — stale
  calibration silently steering predictions is the failure mode this field exists to prevent.

### 4.4 KV disk-tier sidecar index — `drakkar.kvcache-index/1`

The SSD persistence tier (RFC-0005 KV17–KV19, v0.3): evicted `cached` prefix runs serialize
to `~/.drakkar/kv-cache/blocks/<run-id>.safetensors`, described by one index file.

```json
{
  "schema": "drakkar.kvcache-index/1",
  "block_tokens": 32,
  "budget_gib": 8,
  "runs": [
    {
      "id": "run-01J9ZC3AKQ8Y2M4N6P8R0T2V4X",
      "keys": {
        "model_digest": "sha256-3c...",
        "tokenizer": "sha256-7e...",
        "chat_template": "sha256-b2...",
        "kv_bits": 16,
        "rope": "sha256-c4...",
        "block_tokens": 32
      },
      "chain_head": "1f6a...",
      "chain": ["1f6a...", "9d2e...", "..."],
      "file": "blocks/run-01J9ZC3AKQ8Y2M4N6P8R0T2V4X.safetensors",
      "tokens": 8192,
      "bytes": 1207959552,
      "created": "2026-07-14T10:15:00Z",
      "last_hit": "2026-07-14T11:40:00Z",
      "hits": 12
    }
  ]
}
```

- DM29. `keys` duplicates every KV12 correctness key explicitly, even though the DM6 chain
  seed already folds them in: lookup filters on `keys` first (cheap equality), then matches
  `chain` segments. A run whose `keys` mismatch the resident model is invisible — never a
  candidate, never an error.
- DM30. Write protocol (KV18): block file to a temp name, fsync, rename; then index update
  by write-temp + rename of the whole index. A crash at any point leaves either the old
  index (orphan block file, swept by the next LRU pass) or the new one — never a dangling
  reference. Block files and the index are mode `0600`; contents are user prompts, treated
  as sensitive, excluded from diagnostics bundles by default (KV19).
- DM31. Eviction within the disk budget is LRU on `last_hit` (KV19). Restore eligibility is
  priced per KV18: a run is restored only when `bytes / ssd_read_gbs` beats
  `tokens / prefill_tps` by ≥ 3x, using calibrated numbers (§4.3).

### 4.5 Configuration — `config.toml` (`drakkar.config/1`)

`~/.config/drakkar/config.toml`, overlaid by environment and flags:
flags > `DRAKKAR_*` env > file > built-in defaults (CLI10, LD23). Env mapping is mechanical:
`server.port` ⇔ `DRAKKAR_SERVER_PORT` (uppercase, dots to underscores).

```toml
schema = "drakkar.config/1"        # optional; absent reads as major 1 (DM7)

[server]
host = "127.0.0.1"                  # LD22; non-loopback requires api_key (AS18)
port = 11711
api_key = ""                        # empty = unset
hide_reasoning = false              # AS11
responses_api = false               # /v1/responses flag, v0.3 (LD5)

[models]
default = ""                        # ref used when API `model` = "default" (AS3)

[storage]
path = "~/.drakkar"                 # custom/external volume supported from v0.1 (LD14)
import_hf_cache = "clone"           # "clone" | "copy" | "off" (MP11, LD4)

[kv_cache]
# disk unset = mode default: on for `serve`, off for one-shot `run` (KV17)
# disk = true
bits = 16                           # 16 | 8 | 4 (KV13)
disk_budget_gib = 8                 # KV19
ttl_min = 30                        # RAM cached-block TTL (KV20)

[runtime]
keep_alive = "30m"                  # idle unload for `serve`; one-shot unloads immediately (AS17)

[scheduler]
max_concurrency = 8                 # AS14

telemetry = "off"                   # the only accepted value in v1 (CLI16)
```

- DM32. The CLI10 key set (`server.host/port/api_key`, `models.default`,
  `storage.path/import_hf_cache`, `kv_cache.disk/bits/disk_budget_gib`, `runtime.keep_alive`,
  `scheduler.max_concurrency`, `telemetry`) is normative for v0.1. The additional keys above
  (`server.hide_reasoning`, `server.responses_api`, `kv_cache.ttl_min`) are the config
  bindings of AS11, LD5, and KV20 respectively, and ship with their features' milestones.
- DM33. `drakkar config set` validates type and range before writing, and writes atomically
  (temp + rename, CLI11). Unknown keys are the error `config.invalid_key` (or a `doctor`
  warning on read), not a silent parse failure — additive-only applies to config too.
  Unknown *values* for known keys are the error `config.invalid_value`, because guessing
  intent violates fail-legibly.
- DM34. `config.toml` is the only file under user ownership; everything under
  `storage.path` (default `~/.drakkar/`) is reconstructible and safe to delete
  (RFC-0001 A8, [INV-RECONSTRUCT](#5-named-invariants)). No schema in §4.2–§4.4 may
  acquire a field whose loss is unrecoverable.

## 5. Named invariants

Every invariant below is asserted by at least one named test class (unit, property,
golden-fixture, or soak) in the owning RFC's Testing Strategy.

| Invariant | Statement | Enforced by / verified by |
| --- | --- | --- |
| INV-BUDGET | `MemoryReport.actual <= MemoryBudget.declared` at every prefill-chunk and decode-step boundary. | KV pool allocator + admission control (I2, FE18); debug/soak assertion after every step (RFC-0004 AC4, PB4 non-waivable gate) |
| INV-NO-GROWTH | The KV pool never allocates beyond `MemoryBudget.kv_pool` after load; `admit` is the only rejection point and always returns `max_admissible`. | Pool carved at load (KV2); property test on allocator (DM18) |
| INV-REFCOUNT | After quiesce, `blocks_total == blocks_free + blocks_active + blocks_cached`, and every `Cached` refcount equals attached sequences + 1. | Pool accounting (DM19); RFC-0007 AC4 disconnect-storm test |
| INV-KEYS | A KV cache hit (RAM or disk) requires exact equality on all KV12 keys: model digest, tokenizer hash, effective template hash, kv_bits, rope params, block_tokens. Keys are content hashes, never names. | DM6 chain seed + DM29 explicit key filter; RFC-0005 AC5 fuzzed-invalidation property test |
| INV-DIALECT | `GenerationRequest` is dialect-free below the session layer; only response rendering reads `render`. | Visibility boundary (DM11); dialect-conformance trace suites (RFC-0007 AC1) exercise both dialects against one scheduler path |
| INV-SEAM | No type in this document names or embeds Metal, MLX, or llama.cpp types; opaque refs (`ModelHandle`, `BlockTableRef`, `SamplerStateRef`, `MaskRef`) are the only backend state above the seam. | RFC-0001 I5; compile-time: `drakkar-core` has no backend dependencies (DM1, DM2) |
| INV-CONFINE | `ModelHandle` and all backend-owned state are `!Send`/`!Sync` and never leave the engine actor thread. | Type system (DM13); RFC-0001 A2/I1 |
| INV-ONE-TRUTH | Every memory or performance figure in any struct or schema was computed by `drakkar-fit`. | RFC-0001 I3, DM3; FE7 anchor fixtures within 7% (RFC-0004 AC1) |
| INV-CAS | Blobs are immutable and named by the SHA-256 of their content; a manifest never references a blob that fails digest verification; GC never collects a referenced blob. | DM26; store integrity check in `doctor` and GC path |
| INV-SCHEMA | Every persistent file and JSON surface carries `"schema": "drakkar.<name>/<major>"`; additive-only within a major; a newer-major `config.toml` is rejected with `config.invalid_value` (DM9), reconstructible store files are regenerated. | DM7–DM10; golden-fixture round-trips per schema; a seeded major-bump fixture asserts the named rejection |
| INV-MIRROR | Each Rust type that mirrors a schema serializes to it byte-for-byte-compatibly (field names, nesting, units); the FE26 example is a verbatim round-trip fixture. | DM17, DM22; serialization golden tests in CI |
| INV-RECONSTRUCT | Everything under `storage.path` is reconstructible; deleting it is always safe; `config.toml` is the only user-owned file. | RFC-0001 A8, DM34; integration test: delete store, re-run, converge |

## 6. Cross-references

- Component decomposition and the engine-actor threading model these types assume:
  [01-architecture.md](01-architecture.md).
- Full error taxonomy, code registry, and HTTP/exit-code mappings for `DkError`:
  [04-error-model.md](04-error-model.md) and [RFC-0011](../rfcs/RFC-0011-error-taxonomy.md).
- Behavior specifications: memory formulas and verdicts
  ([RFC-0004](../rfcs/RFC-0004-feasibility-engine.md#proposed-design)), pool policy and
  persistence ([RFC-0005](../rfcs/RFC-0005-kv-cache.md#proposed-design)), store and
  conversion pipeline ([RFC-0006](../rfcs/RFC-0006-model-pipeline.md#proposed-design)),
  scheduler and dialects ([RFC-0007](../rfcs/RFC-0007-api-server.md#proposed-design)),
  CLI JSON contract ([RFC-0008](../rfcs/RFC-0008-cli-ux.md#proposed-design)),
  calibration loop ([RFC-0009](../rfcs/RFC-0009-performance.md#proposed-design)).
- Backend ABI types below the seam (the C-side twins of `PrefillChunk`/`DecodeBatch`):
  [RFC-0010](../rfcs/RFC-0010-backend-abi.md).
