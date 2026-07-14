# RFC-0010: Backend FFI and C ABI

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.1

## Summary

This RFC specifies the `dk_*` C ABI: the complete contract between the Rust control plane
and the C++17 shim that links the vendored MLX core. RFC-0002 D2
([Stack Selection](RFC-0002-stack-selection.md#proposed-design)) decided that this boundary
exists; this RFC decides what it is, function by function, byte by byte. The ABI is a
C89-compatible header `dk.h` with four opaque handle types, 36 functions grouped into eight
families, size-prefixed versioned structs, a single `dk_status` return convention, explicit
refcount ownership rules, a one-thread mutation contract matching the engine actor
(RFC-0001 A2, [Architecture](RFC-0001-architecture.md#proposed-design)), and a build
pipeline that compiles the shim and the pinned MLX submodule from `build.rs`. The same
header becomes the public embedder ABI for the v1.0 desktop app.

## Motivation

The FFI boundary is the highest-risk surface in the codebase. Everything on the far side is
C++ with manual memory management, exceptions, and MLX's per-thread stream semantics;
everything on the near side is safe Rust that must stay safe. A use-after-free, a C++
exception unwinding through an `extern "C"` frame, or a second thread touching an MLX array
is undefined behavior that no test suite reliably catches after the fact. The PRD demands a
crash-free rate above 99.9% (PRD M6, [PRD](../../PRD.md#7-success-metrics)) and a 24-hour
zero-leak soak (PRD P14); neither is achievable if the boundary's ownership and threading
rules are discovered incrementally during implementation.

The boundary is also load-bearing for the product roadmap twice over. First, RFC-0002 §6
promises that the shim "isolates 100% of MLX API surface behind ~40 C functions" — that
isolation only holds if the function set is closed and versioned, not accreted ad hoc.
Second, the v1.0 "Harbor" milestone ships a SwiftUI menu-bar app "over the engine's C ABI"
(PRD §8, [PRD](../../PRD.md#8-roadmap)): the header specified here is that embedder surface,
so its shape is a v1.0 product commitment being made now. No backend code lands before this
document is accepted.

## Goals

- Specify every exported symbol of `dk.h`: name, signature, ownership, thread affinity,
  error behavior, and the milestone in which it ships.
- Make the Rust wrapper layer (`drakkar-mlx`) able to enforce memory safety mechanically:
  RAII on every handle, no raw pointer escapes above the sys crate.
- Guarantee that no C++ exception ever crosses into Rust and no Rust panic ever crosses
  into C++.
- Version the ABI and every struct so fields are additive and mismatches fail loudly at
  load, never silently at call time.
- Define a reproducible build: pinned MLX, embedded metallib, committed bindings, a
  from-clean `cargo build` that needs only the Xcode Command Line Tools and CMake.
- Keep scheduling, caching, and eviction policy out of the shim entirely (RFC-0001 A6):
  the ABI moves bytes and runs kernels; Rust decides everything else.

## Non-Goals

- The Rust `InferenceBackend` trait and engine actor protocol (RFC-0001 §5; the trait is
  implemented on top of this ABI, not defined by it).
- The `drakkar-gguf` backend FFI: llama.cpp ships its own C API and `drakkar-gguf` binds it
  directly; the `dk_*` ABI is MLX-shim-only.
- KV policy (prefix hashing, CoW refcounts, eviction scoring): RFC-0005 owns policy; this
  ABI exposes only physical block storage and kernels
  ([KV Cache](RFC-0005-kv-cache.md#proposed-design)).
- ABI stability before v1.0 (the surface is explicitly experimental through v0.x, AB8).
- Multi-process or dynamic-plugin loading in v0.x; the shim is statically linked into the
  `drakkar` binary until v1.0 ships embedder artifacts.
- Bindings for languages other than Rust before v1.0 (the header is designed so Swift and
  others bind it without changes, but nothing is shipped or tested for them earlier).

## Proposed Design

### ABI style and core types

- AB1. The ABI is a single hand-written header `dk.h` with C89-compatible declarations:
  no `//` comments, no `inline`, no designated initializers, no variadic macros in the
  shipped header. Fixed-width integer types come from `<stdint.h>` and `<stddef.h>`
  (present on every supported toolchain; macOS 15+ arm64 is the only target platform,
  PRD N6). The header MUST compile cleanly as C89, C11, C++17, and under
  `bindgen`'s libclang parse. All handles are opaque forward-declared structs. Every
  function returns `dk_status`; all results travel through out-parameters. No function
  uses varargs, bitfields, or passes structs by value except `dk_string` (two words,
  register-classed on arm64 AAPCS).

- AB9. `dk_status` is `int32_t`. `DK_OK == 0`; every failure code is positive, stable
  forever once shipped (codes are never renumbered or reused, only appended), and maps —
  through a single total function, not 1:1 — onto a registered code in the taxonomy
  ([Error Taxonomy](RFC-0011-error-taxonomy.md#proposed-design), ER8). The mapping
  partitions across three prefixes: `abi.*` for ABI-contract faults, `backend.*` for
  compute faults, and `internal.*` for memory-pressure statuses (several statuses may share
  a code — e.g. any memory-pressure fault below the contract is an `internal` invariant
  violation, since admission control, not the backend, owns memory limits per RFC-0001 I2).
  `dk_status_name(dk_status)` returns the mapped taxonomy string for logging; an
  unmapped/unknown code returns `"internal.invariant"` rather than `NULL`.

| `dk_status` | Value | Taxonomy code (RFC-0011) | Typical cause |
| --- | --- | --- | --- |
| `DK_OK` | 0 | — | success |
| `DK_ERR_INVALID_ARGUMENT` | 1 | `abi.invalid_argument` | null out-param, bad shape, bad enum value |
| `DK_ERR_STRUCT_SIZE` | 2 | `abi.struct_size_mismatch` | `struct_size` larger than the shim knows (AB13) |
| `DK_ERR_ABI_MISMATCH` | 3 | `abi.version_mismatch` | `dk_abi_version()` ≠ expected at load (AB3) |
| `DK_ERR_UNSUPPORTED_ARCH` | 4 | `backend.capability_absent` | `config_json` names an architecture the shim lacks |
| `DK_ERR_BAD_WEIGHTS` | 5 | `engine.load_failed` | safetensors parse failure, shape/dtype mismatch vs graph |
| `DK_ERR_OOM_BUDGET` | 6 | `internal.budget_breach` | allocation would breach the declared contract (RFC-0001 I2) |
| `DK_ERR_OOM_SYSTEM` | 7 | `internal.invariant` | Metal/OS allocation failure below the contract (should not happen if RFC-0004 is right) |
| `DK_ERR_KV_EXHAUSTED` | 8 | `internal.invariant` | `dk_kv_alloc_blocks` with zero free blocks — an admission-control invariant breach, never surfaced as `kv.pool_exhausted` (ER8) |
| `DK_ERR_IO` | 9 | `backend.io` | mmap/open failure on a weight path |
| `DK_ERR_METAL` | 10 | `backend.metal_fault` | command-buffer error, shader load failure |
| `DK_ERR_THREAD_VIOLATION` | 11 | `abi.thread_violation` | mutating call from a non-owner thread (debug builds, AB18) |
| `DK_ERR_UNSUPPORTED` | 12 | `backend.capability_absent` | feature gated off by `Capabilities` (e.g. 4-bit KV pre-v0.2) |
| `DK_ERR_INTERNAL` | 13 | `engine.inference_failed` | any caught exception not classified above (AB4) |
| (unmapped/unknown) | — | `internal.invariant` | a `dk_status` the Rust mapper does not recognize (fails compilation if the enum grew; runtime fallback for foreign shims) |

- AB10. All strings crossing the boundary are UTF-8, never NUL-dependent: inbound strings
  are `(const char*, uint64_t len)` pairs; outbound shim-allocated strings are `dk_string`
  (pointer + length, plus a guaranteed trailing NUL for C convenience) and MUST be
  released with `dk_string_free`. `dk_string_free` on a zeroed `dk_string` is a no-op.

Core of the header (normative excerpt; the full `dk.h` is the deliverable of the first
v0.1 implementation issue and must match this RFC):

```c
/* dk.h — DRAKKAR backend ABI (excerpt). Apache-2.0. */

#define DK_ABI_VERSION 1u            /* AB3: bumped on any breaking change */

typedef int32_t dk_status;           /* AB9: codes in the table above */

typedef struct dk_ctx     dk_ctx;    /* device + stream + allocator scope */
typedef struct dk_model   dk_model;  /* built graph + mapped weights */
typedef struct dk_array   dk_array;  /* refcounted array handle (AB5) */
typedef struct dk_kv_pool dk_kv_pool;/* paged KV block storage (v0.2) */

typedef int32_t dk_dtype;
enum { DK_DTYPE_U8 = 0, DK_DTYPE_I32 = 1, DK_DTYPE_I64 = 2, DK_DTYPE_U32 = 3,
       DK_DTYPE_F16 = 4, DK_DTYPE_BF16 = 5, DK_DTYPE_F32 = 6 };

typedef struct dk_string { const char* ptr; uint64_t len; } dk_string;
void dk_string_free(dk_string s);

/* AB15: the only callback in v1. MUST NOT unwind. Called only on the ctx thread. */
typedef void (*dk_log_callback)(int32_t level, const char* utf8_msg,
                                uint64_t len, void* user);

typedef struct dk_ctx_config {
    uint64_t struct_size;            /* AB13: sizeof(dk_ctx_config) as compiled */
    uint64_t memory_budget_bytes;    /* RFC-0001 I2 contract; MLX hard limit */
    uint64_t cache_limit_bytes;      /* MLX allocator cache cap (RFC-0003 IC25) */
    int32_t  enable_aux_stream;      /* RFC-0003 IC3 low-priority stream, 0/1 */
    int32_t  _reserved0;             /* AB13: reserved fields MUST be zero */
} dk_ctx_config;

dk_status dk_ctx_new(const dk_ctx_config* cfg, dk_ctx** out_ctx);
dk_status dk_ctx_free(dk_ctx* ctx);

typedef struct dk_capabilities {
    uint64_t struct_size;
    uint32_t abi_version;
    int32_t  nax_tensor_ops;         /* functional self-test result (RFC-0003 IC26) */
    uint32_t kv_bits_mask;           /* bit N set => N-bit KV supported (4|8|16) */
    int32_t  verify_step;            /* speculation verification available */
    uint32_t gpu_core_count;
    uint32_t macos_major, macos_minor;
    uint64_t metal_recommended_working_set_bytes;
    char     chip_name[64];          /* NUL-terminated UTF-8 */
} dk_capabilities;

dk_status dk_ctx_capabilities(dk_ctx* ctx, dk_capabilities* out);

typedef struct dk_sample_params {
    uint64_t struct_size;
    uint64_t seq_id;                 /* keys on-device penalty state (RFC-0003 IC14) */
    uint64_t seed;                   /* counter-based RNG: (seed, position) */
    float    temperature, top_p, min_p;
    int32_t  top_k;
    float    repetition_penalty, frequency_penalty, presence_penalty;
    int32_t  penalty_window;
    const int32_t* logit_bias_tokens;
    const float*   logit_bias_values;
    uint64_t logit_bias_len;
    int32_t  top_logprobs;           /* 0 = none; on-GPU top-k (RFC-0003 IC4) */
    int32_t  _reserved0;
    /* --- fields below added in v0.2; absent under a v0.1 struct_size --- */
    dk_array* grammar_mask;          /* u8 bitset over vocab, NULL = unconstrained
                                        (RFC-0003 IC16) */
} dk_sample_params;
```

### Function families

- AB2. The ABI comprises exactly the 36 functions below at the v1.0 freeze, in eight
  families. Adding a function after v1.0 requires an RFC amendment and a `DK_ABI_VERSION`
  review (additive functions do not bump the version; see AB3). "Ships" is the first
  milestone in which the symbol is exported and tested.

| Family | Function | Ships | Contract (one line) |
| --- | --- | --- | --- |
| context | `dk_ctx_new` | v0.1 | create device scope; sets MLX memory/cache limits from `memory_budget_bytes` |
| context | `dk_ctx_free` | v0.1 | destroy; all child handles must already be released (debug builds abort otherwise, AB17) |
| context | `dk_ctx_capabilities` | v0.1 | fill `dk_capabilities` (functional NAX self-test, not version sniffing) |
| context | `dk_ctx_set_log_callback` | v0.1 | register the optional log sink (AB15); `NULL` unregisters |
| context | `dk_ctx_synchronize` | v0.1 | block until all issued GPU work completes (tests, memory-report accuracy) |
| context | `dk_ctx_set_memory_budget` | v0.3 | rebalance a live ctx budget (multi-model pool, RFC-0001 A5) |
| array | `dk_array_from_buffer` | v0.1 | copy host buffer into a device array; returns refcount-1 handle |
| array | `dk_array_retain` | v0.1 | +1 refcount |
| array | `dk_array_release` | v0.1 | −1 refcount; frees at zero |
| array | `dk_array_shape` | v0.1 | out: `int64_t dims[DK_MAX_NDIM=8]`, `uint32_t ndim` |
| array | `dk_array_dtype` | v0.1 | out: `dk_dtype` |
| array | `dk_array_size_bytes` | v0.1 | out: materialized byte size |
| array | `dk_array_copy_to_buffer` | v0.1 | eval if lazy, copy to caller buffer (caller supplies capacity; short-buffer is `DK_ERR_INVALID_ARGUMENT`) |
| array | `dk_array_eval` | v0.1 | force evaluation of a lazy array without readback |
| model | `dk_model_build` | v0.1 | build the graph from `config_json` (HF `config.json` + drakkar extensions: quant recipe, max_seq, kv precision) |
| model | `dk_model_load_weights` | v0.1 | mmap safetensors paths, bind tensors to the graph (RFC-0003 IC24) |
| model | `dk_model_metadata` | v0.1 | `dk_string` JSON: derived dims, layer layout classes (RFC-0005 §3), vocab size, quant actually loaded |
| model | `dk_model_free` | v0.1 | unload; releases weights and internal caches |
| exec | `dk_prefill` | v0.1 | process a token chunk for one sequence; optional last-position logits |
| exec | `dk_decode_step` | v0.1 | one decode step for B sequences; batched signature from day one (AB12), B=1 in v0.1 |
| exec | `dk_verify_step` | v0.2 | target forward over B draft streams of k tokens (RFC-0003 IC19/IC20) |
| kv | `dk_kv_alloc_pool` | v0.2 | carve `pool_bytes` into blocks for a model's layout (RFC-0005 KV1/KV2) |
| kv | `dk_kv_free_pool` | v0.2 | destroy pool; outstanding block ids become invalid |
| kv | `dk_kv_alloc_blocks` | v0.2 | allocate n physical blocks, out block ids; `DK_ERR_KV_EXHAUSTED` when empty |
| kv | `dk_kv_free_blocks` | v0.2 | return blocks to the free list (Rust owns refcounts/CoW; AB11) |
| kv | `dk_kv_quantize_run` | v0.2 | in-place requantize a block run to `target_bits` (RFC-0005 KV14) |
| kv | `dk_kv_gather` | v0.2 | export a block run into one contiguous `dk_array` (SSD-tier serialize, RFC-0005 §6) |
| kv | `dk_kv_scatter` | v0.3 | restore a gathered array into freshly allocated blocks (SSD-tier restore) |
| kv | `dk_kv_pool_stats` | v0.2 | out struct: blocks total/free, bytes by precision |
| sample | `dk_sample` | v0.1 | fused on-GPU sampling for B logit rows with per-row params (RFC-0003 IC14/IC15) |
| introspection | `dk_memory_report` | v0.1 | out struct: budget vs actual, MLX active/cache/peak, Metal working set, wired limit (RFC-0003 IC25) |
| introspection | `dk_abi_version` | v0.1 | returns `DK_ABI_VERSION` the shim was compiled with |
| introspection | `dk_last_error_message` | v0.1 | thread-local UTF-8 detail for the last failure on this thread (AB14) |
| introspection | `dk_status_name` | v0.1 | stable taxonomy string for a status code (AB9) |
| introspection | `dk_build_info` | v0.1 | `dk_string` JSON: MLX tag, metallib hash, compiler, build date |
| string | `dk_string_free` | v0.1 | free a shim-allocated string |

- AB11. Policy/mechanism split: the shim is stateless with respect to scheduling and
  caching policy. It never decides which sequence runs, which block is evicted, or what a
  prefix hash means. Block refcounts, CoW splits, the radix index, and eviction scoring
  live in Rust (`KvPool` trait, RFC-0005 KV22); the shim stores block bytes and runs
  gather/quantize/attention kernels against caller-supplied block tables. This is the
  ABI-level enforcement of RFC-0001 A6 and invariant I5.

- AB12. Execution signatures are batched from v0.1 even though v0.1 serves one request at
  a time: `dk_decode_step` takes `batch`, `seq_ids[B]`, `input_tokens[B]`,
  `positions[B]` and returns `(B, vocab)` logits; `dk_sample` takes `n_params` parameter
  structs, one per row. v0.2 continuous batching changes call sites, not signatures.
  Block-table fields on `dk_prefill_args`/`dk_decode_args` are v0.2 additive struct
  fields (`kv_pool == NULL` means the model-internal contiguous cache used in v0.1).

### Versioning

- AB3. `DK_ABI_VERSION` is a `uint32_t` constant starting at 1, bumped on any breaking
  change: a removed or re-typed function, a changed field meaning, a renumbered enum.
  Additive changes (new functions, new trailing struct fields behind `struct_size`, new
  enum values) do not bump it. The Rust wrapper calls `dk_abi_version()` in
  `DkContext::new` before any other symbol and compares against the
  `bindgen`-imported constant; a mismatch is the named fatal error
  `abi.version_mismatch` ([Error Taxonomy](RFC-0011-error-taxonomy.md#proposed-design))
  and the process refuses to serve. While the shim is statically linked (all of v0.x)
  the check is a belt-and-braces guard against stale committed bindings; once v1.0 ships
  a dynamic embedder artifact it becomes the load-time compatibility gate, with the
  v1.0 rule: equal `DK_ABI_VERSION` required, additive growth discovered per-struct via
  `struct_size`.

- AB13. Every struct crossing the boundary, in either direction, begins with
  `uint64_t struct_size`, set by the caller to `sizeof` the struct as the caller
  compiled it. The shim accepts any `struct_size` ≥ the first shipped layout of that
  struct and ≤ the size the shim was compiled with; fields beyond the caller's
  `struct_size` take documented defaults (in-params) or are not written (out-params).
  `struct_size` greater than the shim's compiled size means the caller is newer than the
  shim: the call fails with `DK_ERR_STRUCT_SIZE` and no partial work. `_reserved*`
  fields MUST be zero; a nonzero reserved field is `DK_ERR_INVALID_ARGUMENT` (this keeps
  reserved space actually usable later). Fields are therefore strictly additive and
  append-only; no field is ever reordered, resized, or repurposed.

### Error propagation

- AB4. Every exported function body is wrapped in a catch-all boundary:
  `catch (const dk::budget_error&)` → `DK_ERR_OOM_BUDGET`, `catch (const std::bad_alloc&)`
  → `DK_ERR_OOM_SYSTEM`, `catch (const std::exception&)` → classified by shim-internal
  exception type else `DK_ERR_INTERNAL`, `catch (...)` → `DK_ERR_INTERNAL` with message
  `"non-standard exception"`. No exception of any kind may escape an `extern "C"` frame;
  the shim is compiled with `-fno-exceptions` NOT set (MLX throws), so the catch-all is
  the enforcement point and is present in a single `DK_API_GUARD` macro used by every
  function. Symmetrically, Rust never unwinds into C++: the wrapper crate is compiled
  with `panic = "abort"` in release; in dev profiles every extern boundary the shim can
  re-enter (only the log callback, AB15) wraps its body in `catch_unwind` and aborts on
  panic.

- AB14. `dk_last_error_message()` returns a thread-local, NUL-terminated UTF-8 pointer
  with human-readable detail for the most recent failing call on the calling thread. It
  never returns `NULL` (empty string when no failure has occurred). The pointer is valid
  until the next failing `dk_*` call on the same thread; the Rust wrapper copies it into
  an owned `String` immediately, inside the same wrapper call that observed the failure,
  and attaches it to the taxonomy error as the `detail` field. Successful calls do not
  clear the buffer (so the wrapper never races itself), and the message is diagnostic
  only: code paths MUST branch on `dk_status`, never on message text.

- AB15. The log callback is the only function pointer in the v1 ABI. Constraints, all
  MUST: it is invoked only from the ctx-owning thread (so the Rust closure needs `Send`
  but not `Sync`); it must not call back into any `dk_*` function; it must not unwind
  (the Rust implementation is an `extern "C"` fn whose body is
  `catch_unwind(..).unwrap_or_else(|_| abort())`); the shim treats it as `noexcept` and
  a violation is process-fatal by design. No other Rust-to-C++ callbacks exist in v1;
  any future callback (e.g. an allocation hook) requires an RFC amendment.

### Ownership rules

- AB5. Ownership is allocation-follows-allocator. The caller owns every buffer it passes
  in (`tokens`, `logit_bias_*`, weight path strings, out-param structs) and the shim
  never stores a caller pointer beyond the call: `dk_array_from_buffer` copies into
  device memory before returning, so caller buffers need only outlive the call itself.
  (Weights are the deliberate exception to copying: `dk_model_load_weights` takes
  paths, not buffers, and the shim owns the mmap for the model lifetime, RFC-0003 IC24.
  This keeps the zero-copy load path entirely inside the shim where its lifetime is
  controlled.) Shim-allocated returns are exactly: `dk_string` (freed by
  `dk_string_free`), handles (freed by their `*_free`), and `dk_array` (refcounted).

- AB16. `dk_array` handles are refcounted: creation functions (`dk_array_from_buffer`,
  `dk_prefill`/`dk_decode_step`/`dk_sample` outputs, `dk_kv_gather`) return refcount 1
  owned by the caller; `dk_array_retain`/`dk_array_release` adjust it; the device memory
  is released when the count reaches zero and all lazy consumers have evaluated (the
  shim holds internal references while an array participates in an unevaluated graph, so
  releasing an input immediately after a call is always safe). Passing an array into a
  call never transfers ownership. Release-after-zero and use-after-free are undefined
  behavior at the C level; the Rust layer makes them unrepresentable: `drakkar-mlx-sys`
  exposes raw bindings, and `drakkar-mlx` wraps every handle in an RAII newtype
  (`Clone` = retain, `Drop` = release, `!Send + !Sync` on mutating handles per AB6) so
  no code above the wrapper crate can violate the discipline (RFC-0002 LD24 crate split).

- AB17. Debug builds of the shim (`DK_DEBUG_HANDLES=1`, on in CI sanitizer jobs and the
  fuzz harness) keep a live-handle registry: every handle carries a magic word and a
  generation counter; operations on freed, foreign, or double-released handles abort
  with a report instead of corrupting memory. `dk_ctx_free` aborts if child handles are
  still live, printing their creation backtraces. Release builds omit the registry
  (zero-cost); the fuzz suite (see Testing Strategy) runs exclusively against debug
  shims so violations are caught structurally, not probabilistically.

### Threading contract

- AB6. All mutating calls on a `dk_ctx` and every handle descended from it MUST come from
  a single thread: the engine actor that owns the model (RFC-0001 A2,
  [Architecture](RFC-0001-architecture.md#proposed-design)). The only thread-safe
  entry points are `dk_abi_version` (pure constant), `dk_status_name` (pure lookup), and
  `dk_last_error_message` (thread-local by construction). This is not a simplification
  the shim could relax later: MLX arrays are not thread-safe and MLX binds its default
  stream per thread, so the one-thread rule is load-bearing for correctness of every
  kernel launch, and the architecture already guarantees it (one actor thread owns all
  GPU state, invariant I1). The Rust wrapper encodes it as `!Send + !Sync` on
  `DkContext` and all child handle types; the engine actor owns them by construction.

- AB18. Debug shims record the first mutating thread id per `dk_ctx` and return
  `DK_ERR_THREAD_VIOLATION` (with the offending and owning thread ids in
  `dk_last_error_message`) on any mutating call from another thread. Release shims do
  not check (the Rust type system already prevents it for Rust callers; v1.0 embedder
  documentation states the rule as a hard requirement for foreign callers).

- AB19. The rule is per-`dk_ctx`. From v0.3, multiple contexts may live in one process,
  each owned by its own engine actor thread (RFC-0001 A5, LD12 strict per-engine
  isolation), and calls on distinct contexts may proceed concurrently. Because MLX's
  allocator limit is process-global, the shim maintains per-ctx accounting and sets the
  global MLX limit to the sum of live ctx budgets, adjusting it inside
  `dk_ctx_new`/`dk_ctx_free`/`dk_ctx_set_memory_budget` under an internal mutex (the
  only lock in the shim; it guards limit arithmetic, never GPU work).

### Build system and bindings

- AB7. The shim builds from `drakkar-mlx-sys/build.rs` using the `cmake` crate (version
  pinned in `Cargo.lock`; MLX itself is a CMake project, so driving one CMake
  configure-and-build of `shim/CMakeLists.txt`, which pulls MLX in via
  `add_subdirectory`, beats reimplementing MLX's build under `cc`). Requirements:
  CMake ≥ 3.24 (MLX's floor), Apple Clang from the Xcode Command Line Tools with the
  macOS 15 SDK (`MACOSX_DEPLOYMENT_TARGET=15.0`, PRD P15), C++17. The build produces
  `libdrakkar_shim.a` (shim + MLX objects) which `build.rs` emits as static link
  directives; no dynamic MLX library exists at runtime.

- AB20. MLX is vendored as a git submodule at `backend/mlx`, pinned to an exact release
  tag recorded in three places that CI cross-checks: the submodule commit, a
  `MLX_PINNED_TAG` constant in `build.rs`, and the `dk_build_info` JSON. Upgrades are a
  reviewed PR bumping all three plus a full conformance/bench run; cadence target is
  within two MLX releases (RFC-0002 D5). No network access during build: `cargo build`
  after `git submodule update --init` is hermetic.

- AB21. Metal shaders ship inside the binary: the CMake build compiles MLX's kernels to
  `mlx.metallib`, `build.rs` embeds it via `include_bytes!` into a dedicated section,
  and the shim loads it at `dk_ctx_new` with `newLibraryWithData:` — never from a file
  path, never compiled at runtime, so no Metal compiler toolchain is required on user
  machines (RFC-0002 D5). `dk_build_info` reports the metallib SHA-256 so a mismatched
  library is diagnosable from `drakkar doctor`.

- AB22. `bindgen` generates `drakkar-mlx-sys/src/bindings.rs` from `dk.h` with an
  allowlist of `dk_.*` items. Bindings are committed (so `cargo build` from a source
  tarball needs no libclang) and a CI job regenerates them with the pinned `bindgen`
  version and fails on any diff, guaranteeing header and bindings never drift. The same
  job compiles `dk.h` standalone as C89, C11, and C++17 to enforce AB1.

### Embedder ABI (v1.0)

- AB8. `dk.h` is the public embedder ABI at v1.0: the SwiftUI menu-bar app (PRD §8
  "Harbor") consumes it directly, and third-party embedders get the same surface
  (RFC-0002 §6 promised this dividend). v1.0 releases add an embedder artifact —
  `dk.h` plus `libdrakkar.a` and a signed `libdrakkar.dylib` with the metallib embedded —
  published per [Release Engineering](RFC-0012-release-engineering.md#proposed-design).

- AB23. Stability promise: before v1.0 the ABI is experimental — breaking changes are
  allowed between minor versions with a `DK_ABI_VERSION` bump and a changelog entry, and
  the header carries a comment stating so. From v1.0, the surface freezes: no breaking
  changes within major 1 (only additive functions and additive struct fields per AB13);
  a breaking change means `DK_ABI_VERSION` 2 and a major product version. The embedder
  documentation ships the threading contract (AB6), the ownership rules (AB5/AB16), and
  the conformance suite as the normative compatibility definition.

## Alternatives Considered

**mlx-c (the official MLX C API).** Rejected. mlx-c historically lags core features by
weeks — exactly the features DRAKKAR exists to exploit first (tensor-op paths, batch
primitives); the PRD carries this as a named risk row ([PRD](../../PRD.md#9-risks-and-mitigations)).
It is also a second abstraction layer under our own C ABI: we would translate
`dk_*` → `mlx_*` → C++ instead of `dk_*` → C++, adding surface without removing any
obligation (we still need our own header for the embedder promise and for struct
versioning). Linking MLX C++ directly means new core features are consumable the day they
land (RFC-0002 D2).

**cxx / autocxx Rust–C++ interop crates.** Rejected. Generated bridges are excellent for
project-internal boundaries but poor foundations for a stable, multi-language embedder
ABI: the bridge module is the interface, its mangling and layout are crate-version
artifacts, and Swift or any non-Rust consumer would still need a hand-written C layer on
top. A plain C header is the lingua franca that serves bindgen, Swift's C interop, and
anything else identically, and it makes the versioning rules (AB3/AB13) expressible in
the artifact itself rather than in a toolchain.

**mlx-rs (third-party Rust bindings) as the foundation.** Rejected as a base, valuable as
evidence: the mlxrs work demonstrates Rust-over-MLX is tractable and documents the
threading constraints this RFC adopts (AB6). But building the product on third-party
bindings imports their maintenance cadence as our critical path, they trail core the same
way mlx-c does, and they do not give us the C embedder surface. Tracked alongside
RFC-0002 R1 as a revisit trigger, not a v1 dependency.

**Vtable ABI (one struct of function pointers).** Rejected. A `dk_api_v1` vtable earns
its keep when multiple implementations are loaded dynamically behind one symbol; the shim
is statically linked with exactly one implementation through all of v0.x. The vtable adds
an indirection on every call, complicates bindgen output (function-pointer fields instead
of plain externs), and moves versioning into a bespoke negotiation protocol when
`dk_abi_version()` + size-prefixed structs already solve it. If v1.x ever needs runtime
backend selection, a vtable can be layered over these functions without breaking them.

## Drawbacks

- `dk.h` is hand-written and hand-maintained: every shim change needs a matching header
  edit, and the compiler cannot prove header and implementation agree the way a
  generated binding would. Mitigated by AB22's regenerate-and-diff CI and the conformance
  suite exercising every symbol, but the maintenance tax is real and permanent.
- Refcount discipline across the FFI is fuzz-tested, not compiler-proven. Rust's
  guarantees stop at `drakkar-mlx-sys`; a bug in the wrapper crate or in a future foreign
  embedder can still double-release. AB17's debug registry converts corruption into
  aborts, which is containment, not prevention.
- CMake-inside-build.rs makes cold builds slow (MLX plus shim is minutes of C++
  compilation) and couples `cargo build` to a correctly installed CMake and Xcode CLT.
  Mitigation: a prebuilt shim artifact cache keyed on (MLX tag, shim source hash,
  SDK version) in CI, and `sccache`-style local reuse; see Open Questions.
- Batched-from-day-one signatures (AB12) mean v0.1 carries argument plumbing it does not
  yet exercise at B>1; the cost is small and buys signature stability, but B>1 paths are
  genuinely untested until v0.2's conformance additions land.
- The one-thread contract pushes complexity to embedders at v1.0: a Swift caller must
  reproduce the actor discipline that Rust gets from the type system, with only debug
  checks (AB18) and documentation to catch mistakes.

## Migration / Rollout

- **v0.1 "First light."** Header lands with all types and the 26 v0.1 functions
  (families: context, array, model, prefill/decode single-sequence via the
  model-internal contiguous cache, sample, introspection, string). `DK_ABI_VERSION = 1`.
  Conformance suite, sanitizer CI, refcount fuzz, and the bindgen freshness gate are all
  v0.1 exit criteria — the safety net exists before the surface grows.
- **v0.2 "Convoy."** Additive: the seven KV pool functions, `dk_verify_step`, the
  `grammar_mask` field on `dk_sample_params`, and `kv_pool`/block-table fields on
  prefill/decode args (all behind `struct_size`, no version bump). Batched decode
  exercised at B up to 16 by the conformance suite. `dk_kv_quantize_run` gated by
  `dk_capabilities.kv_bits_mask`.
- **v0.3 "Fleet."** Additive: `dk_kv_scatter` (SSD-tier restore), `dk_ctx_set_memory_budget`
  (pool rebalancing), and multi-ctx-per-process support hardened (AB19 accounting,
  TSan CI job covering concurrent contexts).
- **v1.0 "Harbor."** ABI freeze per AB23; embedder artifacts (`dk.h`, `libdrakkar.a`,
  signed dylib) join the release train (RFC-0012); embedder documentation and the
  conformance suite published as the compatibility definition. Any breaking change found
  necessary before freeze happens in v0.x with a version bump — after freeze it waits
  for major 2.

## Testing Strategy

- **ABI conformance suite** (`drakkar-mlx-sys/tests/abi_conformance.rs`, v0.1): table-driven,
  one entry per exported function covering the happy path and every documented
  `dk_status` for that function (null out-params → `DK_ERR_INVALID_ARGUMENT`, oversize
  `struct_size` → `DK_ERR_STRUCT_SIZE`, unsupported kv bits → `DK_ERR_UNSUPPORTED`, and
  so on). CI fails if a header symbol has no conformance entry.
- **Sanitizer builds** (CI jobs `shim-asan`, `shim-ubsan`, `shim-tsan`): the shim's C++
  unit tests and the conformance suite run under ASan+LSan and UBSan on every PR; TSan
  runs the multi-ctx tests (v0.3) and the thread-violation tests.
- **Refcount fuzz** (`fuzz_array_refcount`, proptest driving the real FFI against a debug
  shim): random interleavings of `from_buffer`/`retain`/`release`/`eval`/`copy_to_buffer`
  checked against a reference counting model; the AB17 registry must report zero live
  handles at sequence end, and LSan must report zero leaks.
- **KV lifecycle fuzz** (`fuzz_kv_blocks`, v0.2): random
  `alloc_blocks`/`free_blocks`/`quantize_run`/`gather`/`scatter` sequences; invariants:
  pool accounting (`dk_kv_pool_stats`) always consistent, no block id served twice while
  live, exhaustion always reports `DK_ERR_KV_EXHAUSTED` and never over-allocates the pool
  (RFC-0001 I2).
- **Exception-to-status tests**: the shim compiles with `DK_TEST_THROW_INJECTION` hooks
  that force `std::bad_alloc`, `std::runtime_error`, a shim `budget_error`, and a
  non-`std::exception` foreign throw at each family's injection point; tests assert the
  mapped `dk_status`, a non-empty `dk_last_error_message`, and that the handle involved
  is still in a defined state (freeable).
- **ABI version-mismatch integration test**: a test build of the shim compiled with
  `DK_ABI_VERSION + 1`; the wrapper's context construction must fail with the named fatal
  `abi.version_mismatch` and a message carrying both versions, before any other symbol is
  invoked.
- **Struct-size evolution test**: calls `dk_sample` and `dk_prefill` with the recorded
  v0.1 `struct_size` values (golden constants committed at v0.1 release) asserting
  acceptance with documented defaults, and with `sizeof + 8` asserting
  `DK_ERR_STRUCT_SIZE`; a static assertion table pins every struct's field offsets so an
  accidental reorder fails compilation, not review.
- **Thread-violation test**: mutating call from a spawned thread against a debug shim
  returns `DK_ERR_THREAD_VIOLATION`; `dk_abi_version`/`dk_status_name`/
  `dk_last_error_message` called concurrently from 8 threads under TSan report clean.
- **Miri** on the wrapper layer: `drakkar-mlx` unit tests run under Miri with the sys
  layer replaced by a mock implementation of the extern symbols (Miri cannot execute the
  real FFI), verifying the RAII wrappers themselves are UB-free.
- **Golden decode fixture**: build + load a 4-layer test model, greedy prefill+decode of
  32 tokens compared against committed token ids and logit checksums; ties this suite to
  the RFC-0003 AC1 parity harness and catches silent numeric drift on MLX upgrades.
- **Soak hook**: the 24 h mixed-load soak (PRD P14, RFC-0009) runs a debug-handle shim
  weekly; RSS drift < 2% and a zero live-handle report at shutdown are pass criteria.

## Open Questions

1. Prebuilt-shim caching strategy: exact cache key composition, storage (CI artifact
   store vs release-attached archives), and whether developer machines consume the cache
   by default or opt in. Owner: abdelstark. Resolution: decided in the v0.1 CI
   implementation PR that introduces the `shim-build` job; the ABI itself is unaffected
   by the outcome.

## References

- RFC-0001 A2/A5/A6, I1/I2/I5 ([Architecture](RFC-0001-architecture.md#proposed-design)) — engine actor, backend seam, invariants this ABI enforces
- RFC-0002 D2/D5, §6, R1/R2 ([Stack Selection](RFC-0002-stack-selection.md#proposed-design)) — the decision that the shim and this ABI exist; pinning and metallib policy
- RFC-0003 IC3/IC4/IC14–IC16/IC24–IC26 ([Inference Core](RFC-0003-inference-core.md#proposed-design)) — execution, sampling, and platform semantics the functions expose
- RFC-0005 KV1–KV4, KV13/KV14, KV22 ([KV Cache](RFC-0005-kv-cache.md#proposed-design)) — block model behind the `dk_kv_*` family
- RFC-0011 ([Error Taxonomy](RFC-0011-error-taxonomy.md#proposed-design)) — taxonomy names for every `dk_status`
- RFC-0012 ([Release Engineering](RFC-0012-release-engineering.md#proposed-design)) — embedder artifact publication and signing
- [PRD](../../PRD.md#9-risks-and-mitigations) §8 roadmap, §9 risk rows (MLX churn, mlx-c lag), M6/P14/P15
- ml-explore/mlx — C++ API, per-thread stream and array thread-safety documentation, memory/cache limit APIs, CMake ≥ 3.24 requirement, offline metallib build
- ml-explore/mlx-c — the lag-behind-core history motivating direct C++ linkage
- oxideai/mlx-rs and the mlxrs crate — documented `!Send` array semantics adopted by AB6
- rust-lang/rust-bindgen; the `cmake` crate — binding generation and build orchestration
- dtolnay/cxx and google/autocxx — the interop-crate alternative evaluated and rejected
