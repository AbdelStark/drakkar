# Implementation Tracker — 2026-07-14

Generated from the specification corpus in [PR #1](https://github.com/AbdelStark/drakkar/pull/1). Every implementable unit of work in the corpus is filed as a GitHub issue below. Each issue is independently shippable; cross-issue dependencies are recorded in each issue body and summarized at the end of this document.

Totals: 284 implementation issues across 13 subsystems and 4 milestones, plus 13 tracking issues.

## Milestone v0.1  (153 issues)

| # | Title | Area | Priority | Effort | Spec |
|---|-------|------|----------|--------|------|
| #162 | backend-mlx: author dk.h C89 backend ABI header (AB1, AB9, AB13) | backend-mlx | p0 | m | rfc-0002 rfc-0010 |
| #163 | backend-mlx: build.rs + CMake shim build with pinned MLX and embedded metallib | backend-mlx | p0 | l | rfc-0002 rfc-0010 |
| #164 | backend-mlx: drakkar-mlx-sys bindgen bindings + committed-freshness CI gate | backend-mlx | p0 | m | rfc-0010 |
| #165 | backend-mlx: DK_API_GUARD catch-all + thread-local last-error + dk_status_name | backend-mlx | p0 | m | rfc-0010 rfc-0011 |
| #166 | backend-mlx: implement context family — ctx | backend-mlx | p0 | m | rfc-0003 rfc-0010 |
| #167 | backend-mlx: implement array family — refcounted handles + host copy/eval | backend-mlx | p0 | m | rfc-0003 rfc-0010 |
| #168 | backend-mlx: implement model family — build/load_weights/metadata/free | backend-mlx | p0 | m | rfc-0003 rfc-0010 |
| #169 | backend-mlx: implement dk_prefill + dk_decode_step | backend-mlx | p0 | m | rfc-0003 rfc-0010 |
| #170 | backend-mlx: implement dk_sample fused on-GPU sampling (AB12, IC14/IC15) | backend-mlx | p0 | m | rfc-0003 rfc-0010 |
| #171 | backend-mlx: implement dk_memory_report, dk_abi_version, dk_build_info | backend-mlx | p1 | s | rfc-0003 rfc-0010 |
| #172 | backend-mlx: drakkar-mlx RAII handle wrappers with !Send/!Sync and load-time | backend-mlx | p0 | m | rfc-0002 rfc-0010 |
| #173 | backend-mlx: exhaustive dk_status to taxonomy total function (AB9, ER8) | backend-mlx | p1 | s | rfc-0010 rfc-0011 |
| #174 | backend-mlx: table-driven ABI conformance suite covering every symbol | backend-mlx | p0 | m | rfc-0010 |
| #175 | backend-mlx: shim ASan/UBSan/TSan test targets wired into the sanitizer CI | backend-mlx | p0 | m | rfc-0010 |
| #176 | backend-mlx: proptest refcount leak fuzz against a debug shim (Testing Strategy) | backend-mlx | p1 | m | rfc-0010 |
| #177 | backend-mlx: struct-size evolution + ABI version-mismatch tests (AB13, AB3) | backend-mlx | p1 | s | rfc-0010 |
| #178 | backend-mlx: exception-to-status injection tests (DK_TEST_THROW_INJECTION) | backend-mlx | p1 | s | rfc-0010 |
| #179 | backend-mlx: golden decode fixture (4-layer model, 32-token parity) | backend-mlx | p1 | m | rfc-0003 rfc-0010 |
| #97 | cli: build clap command tree, global flags, dual-render + exit-code framework | cli | p0 | m | rfc-0008 |
| #98 | run: orchestrate fit-check -> acquire -> load -> REPL/one-shot (CLI1-CLI2) | cli | p0 | m | rfc-0008 |
| #99 | fit: render feasibility report card + --json (FE25) | cli | p0 | m | rfc-0004 rfc-0008 |
| #100 | doctor: environment report + config sanity + --check-update (CLI16) | cli | p1 | m | rfc-0008 |
| #101 | repl: streaming loop, Ctrl-C cancel, meta-commands, history (CLI3-CLI5) | cli | p1 | l | rfc-0008 |
| #102 | pull: acquire and prepare a model without running | cli | p1 | m | rfc-0008 |
| #103 | ls: list installed models with size, format, quant, last-used | cli | p1 | s | rfc-0008 |
| #104 | rm/prune: remove a model and GC unreferenced blobs | cli | p1 | m | rfc-0008 |
| #105 | ps: show running model residency and throughput (single-model) | cli | p1 | s | rfc-0008 |
| #110 | config: get\|set\|path with validation, atomic write, precedence (CLI10-CLI11) | cli | p1 | m | rfc-0008 |
| #111 | completions: generate bash/zsh/fish completions from the command tree | cli | p1 | s | rfc-0008 |
| #112 | alias ls: render the shipped+user alias table with collision warning | cli | p2 | s | rfc-0006 rfc-0008 |
| #113 | serve: foreground HTTP server launch; --daemon refuses with milestone message | cli | p1 | m | rfc-0007 rfc-0008 |
| #115 | first-run: one-screen orientation shown once, suppressed for agents (CLI14) | cli | p1 | s | rfc-0008 |
| #116 | error: render what/why/remedy shape and top-level panic wrapper (CLI15) | cli | p1 | m | rfc-0008 rfc-0011 |
| #117 | stream: --stream-json JSON Lines event framework for run (CLI7) | cli | p1 | m | rfc-0008 |
| #118 | test: --json schema-validation harness + exit-code matrix (AC1, T1/T2) | cli | p1 | l | rfc-0008 |
| #119 | test: TTY/non-TTY/NO_COLOR rendering snapshots (AC4, T3) | cli | p1 | m | rfc-0008 |
| #120 | core: scaffold the eleven-crate Cargo workspace with layered dependency graph | core | p0 | m | rfc-0001 rfc-0002 |
| #121 | core: define the shared vocabulary types | core | p0 | m | rfc-0001 rfc-0003 rfc-0004 |
| #122 | core: define engine-actor execution value types | core | p1 | s | rfc-0001 rfc-0003 |
| #123 | core: implement DkError, closed ErrorCode registry, and the total exit/HTTP | core | p0 | m | rfc-0011 |
| #124 | core: add exhaustive-match + golden tuple snapshot CI gate for the error | core | p1 | s | rfc-0011 |
| #125 | core: implement the drakkar.<name>/<major> schema-version reader/writer helper | core | p1 | s | rfc-0001 |
| #126 | core: implement config.toml load/merge with flags>env>file>defaults precedence | core | p1 | m | rfc-0008 |
| #127 | core: implement the secret-redaction layer for all tracing sinks | core | p1 | s | rfc-0001 rfc-0007 |
| #128 | core: set up tracing framework, request ids, spans, file sink and rotation | core | p1 | m | rfc-0007 rfc-0008 |
| #129 | core: add cargo-tree/cargo-deny/public-api CI gate enforcing DEP1-7 and I5 | ci | p1 | s | rfc-0001 rfc-0002 |
| #130 | core: implement the metrics registry and Prometheus exposition for v0.1 metric | core | p1 | m | rfc-0003 rfc-0007 |
| #2 | docs: write README and CLI quickstart (install, run, fit, serve) | docs | p1 | m | rfc-0004 rfc-0008 |
| #3 | docs: generate error-code reference page from the drakkar.errors/1 registry | docs | p1 | m | rfc-0011 |
| #8 | docs: write supported models and aliases page (LD3 alias table) | docs | p2 | s | rfc-0006 |
| #11 | docs: expand CONTRIBUTING with spec-first workflow and RFC template | docs | p2 | s | rfc-0001 rfc-0012 |
| #190 | engine: define InferenceBackend trait and seam types (A6-A7) | engine | p0 | m | rfc-0001 |
| #191 | engine: implement dedicated-thread engine actor with EngineMsg loop (A2, ER5) | engine | p0 | m | rfc-0001 rfc-0011 |
| #192 | backend-mlx: implement capability probe with NAX functional self-test (IC26) | backend-mlx | p0 | m | rfc-0003 |
| #193 | backend-mlx: mmap safetensors to MLX arrays and set memory limits from budget | backend-mlx | p0 | m | rfc-0001 rfc-0003 |
| #194 | backend-mlx: build config-driven Llama-family forward graph (IC5, IC10) | backend-mlx | p0 | m | rfc-0003 |
| #195 | backend-mlx: lazy-graph decode step with async eval and prefill path (IC1-IC4) | backend-mlx | p0 | m | rfc-0003 |
| #196 | backend-mlx: on-GPU sampler pipeline with counter-based RNG (IC14-IC15) | backend-mlx | p1 | m | rfc-0003 |
| #197 | backend-mlx: compiled-graph shape-bucket cache (IC2) | backend-mlx | p1 | m | rfc-0003 |
| #198 | backend-mlx: serve the weight-format matrix (IC6) | backend-mlx | p0 | m | rfc-0003 |
| #199 | backend-mlx: chunked prefill on the NAX tensor-op route with bounded watermark | backend-mlx | p0 | m | rfc-0003 rfc-0009 |
| #200 | backend-mlx: integrate grammar-mask stage into the sampler (IC16) | backend-mlx | p1 | m | rfc-0003 |
| #201 | backend-mlx: Qwen3 dense architecture graph | backend-mlx | p1 | m | rfc-0003 |
| #202 | backend-mlx: Mistral architecture graph | backend-mlx | p2 | s | rfc-0003 |
| #203 | backend-mlx: per-architecture logit-parity golden fixtures vs mlx-lm | backend-mlx | p1 | m | rfc-0003 |
| #204 | backend-mlx: greedy and seeded determinism tests (LD6) | backend-mlx | p1 | s | rfc-0003 |
| #205 | backend-mlx: sampler stage-order and chi-square distribution tests (IC14-IC15) | backend-mlx | p1 | m | rfc-0003 |
| #206 | backend-mlx: chunked vs single-shot prefill equivalence test (IC12-IC13) | backend-mlx | p1 | s | rfc-0003 |
| #224 | fit: scaffold drakkar-fit pure library and core I/O types | fit | p0 | s | rfc-0004 |
| #225 | fit: implement Apple Silicon hardware probe and fallback table (FE2, FE15) | fit | p0 | m | rfc-0003 rfc-0004 |
| #226 | fit: parse config.json and safetensors index into ModelDescriptor (FE1) | fit | p0 | m | rfc-0004 rfc-0006 |
| #227 | fit: implement weights, everything-else, and GQA KV memory model | fit | p0 | m | rfc-0004 |
| #228 | fit: architecture-aware KV for SWA-hybrid, MLA, SSM, paged (FE9-FE12) | fit | p0 | m | rfc-0004 rfc-0005 |
| #229 | fit: implement ctx_max context solver per KV precision (FE20) | fit | p0 | m | rfc-0004 |
| #230 | fit: verdict tiers, ranked remedies, and wired-limit guidance (FE17, FE19) | fit | p0 | m | rfc-0004 |
| #231 | fit: decode roofline, prefill anchors, TTFT, confidence tiers (FE21-FE24) | fit | p1 | m | rfc-0004 rfc-0009 |
| #232 | fit: implement drakkar.fit/1 JSON schema surface (FE26) | fit | p0 | s | rfc-0004 |
| #235 | fit: FE7 Apple MLX footprint anchor golden fixtures | fit | p0 | s | rfc-0004 |
| #236 | fit: hand-computed kv(ctx) fixtures per layout class (AC2) | fit | p0 | m | rfc-0004 |
| #237 | fit: section-9 worked-example golden fixtures | fit | p1 | m | rfc-0004 |
| #238 | fit: verdict-monotonicity property tests | fit | p1 | m | rfc-0004 |
| #239 | fit: fuzz config.json and safetensors-index parsers | fit | p1 | s | rfc-0004 rfc-0011 |
| #26 | kv-cache: define KvPool trait and interim contiguous pool (KV22) | kv-cache | p0 | m | rfc-0001 rfc-0005 |
| #72 | models: implement reference resolution grammar, alias table, and sibling | models | p0 | l | rfc-0006 |
| #73 | models: metadata-only fetch and HF token discovery (MP2) | models | p0 | m | rfc-0006 |
| #74 | models: fit-driven artifact selection for MLX routes 1-3 (MP4, MP5) | models | p0 | m | rfc-0006 |
| #75 | models: reject pickle-only checkpoints with named error (MP6, SEC11, AC5) | models | p0 | s | rfc-0006 |
| #76 | models: content-addressed blob/manifest store with HF-cache clone interop and | models | p0 | l | rfc-0006 |
| #77 | models: parallel ranged resumable download with integrity and disk preflight | models | p0 | l | rfc-0006 |
| #78 | models: per-manifest-key advisory pull lock for concurrent pulls | models | p1 | m | rfc-0006 |
| #79 | models: streaming on-device safetensors-to-MLX quantizer and convert command | models | p0 | l | rfc-0006 |
| #80 | models: tokenizer load with sentencepiece fallback and stable hash (MP17) | models | p0 | m | rfc-0006 |
| #81 | models: sandboxed chat-template execution, override table, and dialect | models | p0 | l | rfc-0006 |
| #86 | models: resolver table-test suite for every MP1 form (AC1 grammar) | models | p1 | m | rfc-0006 |
| #87 | models: AC2 kill-during-download resume integration test | models | p1 | s | rfc-0006 |
| #88 | models: AC3 HF-cache interop zero-network clone-cost test | models | p1 | m | rfc-0006 |
| #89 | models: AC4 70B-to-4bit conversion memory and bpw test | models | p1 | m | rfc-0006 |
| #90 | models: integrity-failure injection integration test (INV-MP-ATOMIC) | models | p1 | m | rfc-0006 |
| #91 | models: disk-preflight boundary tests (MP9) | models | p1 | s | rfc-0006 |
| #92 | models: template-override golden regression corpus with CI gate (MP18) | models | p1 | m | rfc-0006 |
| #93 | models: tokenizer-hash stability golden test (KV12) | models | p1 | s | rfc-0006 |
| #94 | models: store GC property test (INV-MP-GC) | models | p1 | m | rfc-0006 |
| #132 | ci: implement PR correctness pipeline on hosted macOS arm64 (RE15) | ci | p0 | m | rfc-0012 |
| #133 | ci: add shim ASan/UBSan/TSan sanitizer job for the FFI boundary (RE15.4) | ci | p1 | m | rfc-0002 rfc-0010 rfc-0012 |
| #134 | ci: build schema-registry additive-only checker for --json and HTTP schemas | ci | p1 | m | rfc-0012 |
| #135 | ci: record release-profile binary size and post delta against RE2 budget | ci | p1 | s | rfc-0012 |
| #136 | release: add Keep a Changelog CHANGELOG.md scaffold (RE25) | release | p1 | s | rfc-0012 |
| #137 | ci: enforce changelog entry on PRs with a user-facing area label (RE15.8) | ci | p1 | s | rfc-0012 |
| #138 | release: pin Rust toolchain and document MSRV stable-minus-two policy (RE6, RE7) | release | p0 | s | rfc-0012 |
| #139 | release: commit Cargo.lock and pin MLX/llama.cpp submodules to exact hashes | release | p0 | m | rfc-0002 rfc-0012 |
| #140 | ci: run cargo-audit and cargo-deny nightly with a license allowlist (RE8, RE16) | ci | p1 | s | rfc-0012 |
| #141 | release: scaffold cargo xtask crate with preflight and --dry-run (RE18, RE24) | release | p1 | m | rfc-0012 |
| #142 | release: emit build-time version identity in drakkar.version/1 schema (RE5) | release | p1 | m | rfc-0012 |
| #143 | ci: assert dylib allowlist, embedded metallib, and version identity (RE1, RE5) | ci | p1 | m | rfc-0012 |
| #41 | core: add Secret<String> redacting wrapper for tokens and API keys (SEC27) | core | p1 | s | rfc-0001 rfc-0011 |
| #42 | models: bounded, allocation-safe safetensors/GGUF header parsing (SEC12/MP8) | models | p0 | m | rfc-0001 rfc-0006 rfc-0011 |
| #43 | models: reject pickle checkpoints and enforce no-trust_remote_code | models | p1 | s | rfc-0001 rfc-0006 rfc-0011 |
| #44 | models: validate repo-supplied path components against traversal (SEC13/MP10) | models | p1 | s | rfc-0006 rfc-0011 |
| #45 | models: enforce sandbox and resource bounds on chat templates (SEC14/MP18) | models | p1 | m | rfc-0006 rfc-0011 |
| #46 | models: HF token discovery order with never-logged discipline (SEC26/A12/MP2) | models | p1 | s | rfc-0001 rfc-0006 |
| #48 | server: loopback-default bind, constant-time key compare, CORS opt-in | server | p0 | m | rfc-0001 rfc-0007 rfc-0011 |
| #49 | server: validate Host header against DNS rebinding (SEC6) | server | p1 | s | rfc-0007 rfc-0011 |
| #50 | server: metadata-only request logging with --log-bodies sensitive flag | server | p1 | s | rfc-0007 rfc-0011 |
| #52 | core: write config.toml 0600 atomically with api-key precedence (SEC20/SEC28) | core | p1 | s | rfc-0001 rfc-0008 |
| #53 | cli: doctor warns on world/group-readable secret files (SEC20) | cli | p1 | s | rfc-0001 rfc-0008 |
| #54 | ci: supply-chain threat-mitigation policy (Cargo.lock pin, deny/audit) (SEC25) | ci | p1 | s | rfc-0002 rfc-0012 |
| #56 | server: verify only HF-hub and check-update outbound connections (SEC10/A10) | server | p2 | s | rfc-0001 rfc-0011 |
| #57 | docs: SECURITY.md with private vulnerability reporting process (SEC29/SEC30) | docs | p1 | s | rfc-0001 rfc-0012 |
| #240 | server: stand up axum/tokio server with /health, /v1/models, /metrics | server | p0 | m | rfc-0007 |
| #241 | server: normalize OpenAI/Anthropic requests into GenerationRequest with | server | p0 | m | rfc-0007 |
| #242 | server: implement SSE renderer with 10s heartbeats and disconnect-cancel | server | p0 | m | rfc-0007 |
| #243 | server: implement /v1/chat/completions streaming + non-streaming (AS1, AS4-AS7) | server | p0 | l | rfc-0007 |
| #244 | server: assemble usage accounting with x_drakkar extensions on every completion | server | p1 | m | rfc-0007 |
| #245 | server: map honored sampling parameters onto the engine pipeline (AS7) | server | p1 | s | rfc-0007 |
| #246 | server: enforce fail-loud unsupported fields with closed ignorable allowlist | server | p1 | m | rfc-0007 |
| #247 | server: enforce bind/api-key/host/CORS network-edge posture (AS18, SEC3-SEC7) | server | p0 | m | rfc-0007 |
| #248 | server: metadata-only request logging with opt-in --log-bodies (AS19) | server | p1 | s | rfc-0007 |
| #249 | server: wire request_id and the per-request span hierarchy (AS22, OBS5-OBS6) | server | p1 | m | rfc-0007 |
| #250 | server: emit the server/request/latency Prometheus metric families (AS21) | server | p1 | m | rfc-0007 |
| #251 | server: render OpenAI-dialect error envelopes with status mapping (AS8) | server | p0 | m | rfc-0007 |
| #252 | server: property test that no secret leaks to any log sink (OBS11) | server | p1 | s | rfc-0007 |
| #253 | server: golden stream-envelope test for the OpenAI dialect | server | p1 | s | rfc-0007 |
| #254 | server: unit suite for field classification, max-tokens clamp | server | p1 | m | rfc-0007 |
| #255 | server: statistical constant-time test for the API-key compare (AS18) | server | p1 | s | rfc-0007 |
| #58 | test: build hub-sim recorded Hugging Face hub responder (TS2, §9.1) | ci | p1 | m | rfc-0006 |
| #59 | test: versioned corpora layout, MANIFEST schema, shrinkage guard (TS7, §2.3) | ci | p1 | m | rfc-0003 |
| #60 | test: proptest quick/extended profiles and shared strategies (TS9) | core | p1 | s | rfc-0005 rfc-0011 |
| #61 | test: insta snapshot framework for schemas and error envelopes (TS18, TS19) | core | p1 | m | rfc-0004 rfc-0011 |
| #62 | cli: build command × outcome-class integration harness + fixtures (RFC-0008 AC1) | cli | p1 | m | rfc-0008 |
| #63 | server: recorded-trace replay engine + OpenAI strict-mode cassettes (TS20, TS21) | server | p1 | m | rfc-0007 |
| #69 | ci: pin and fetch the content-addressed CI model set (TS6, §2.2) | ci | p1 | m | rfc-0003 |
| #70 | ci: PR/nightly/release test-tiering config, quarantine + speed budget (TS3, §10) | ci | p1 | s | rfc-0012 |
| #71 | ci: traceability-check enforcing AC→test→job matrix + envelope snapshots | ci | p1 | m | rfc-0011 |

## Milestone v0.2  (109 issues)

| # | Title | Area | Priority | Effort | Spec |
|---|-------|------|----------|--------|------|
| #180 | backend-mlx: implement KV pool lifecycle — alloc/free pool, alloc/free blocks | backend-mlx | p1 | m | rfc-0005 rfc-0010 |
| #181 | backend-mlx: implement dk_kv_quantize_run + dk_kv_gather (KV14, RFC-0005 §6) | backend-mlx | p1 | m | rfc-0005 rfc-0010 |
| #182 | backend-mlx: enable B>1 decode + block-table prefill/decode args (AB12) | backend-mlx | p1 | m | rfc-0005 rfc-0010 |
| #183 | backend-mlx: implement dk_verify_step for speculative decoding (IC19/IC20) | backend-mlx | p1 | m | rfc-0003 rfc-0010 |
| #184 | backend-mlx: add grammar_mask sampling field + vocab-bitset upload (IC16) | backend-mlx | p1 | s | rfc-0003 rfc-0010 |
| #185 | backend-mlx: KV lifecycle fuzz harness (fuzz_kv_blocks) | backend-mlx | p1 | m | rfc-0005 rfc-0010 |
| #15 | bench: implement drakkar bench harness with workloads, variance, result schema | bench | p0 | l | rfc-0009 |
| #16 | bench: pin deterministic A-E workload fixtures and add drift guard (PB9) | bench | p1 | m | rfc-0009 |
| #17 | bench: instrument TTFT cold/warm, ITL/TPOT p50/p95, prefill+decode throughput | bench | p0 | m | rfc-0009 |
| #18 | bench: instrument peak-memory-vs-contract and powermetrics energy with no-root | bench | p1 | m | rfc-0009 |
| #19 | bench: instrument concurrency scaling and sustained-vs-burst (PB6-PB7) | bench | p1 | m | rfc-0009 |
| #20 | bench: add Apple M5/MLX golden anchor fixtures and comparison (PB10) | bench | p1 | s | rfc-0004 rfc-0009 |
| #21 | bench: emit and enforce the LD18 reproducibility manifest on every record | bench | p0 | m | rfc-0009 |
| #22 | bench: implement bench --calibrate writing per-chip calibration store (PB15) | bench | p0 | l | rfc-0004 rfc-0009 |
| #23 | bench: seeded decode-regression and NAX-disable gate drills (AC4) | bench | p0 | m | rfc-0003 rfc-0009 |
| #24 | bench: nightly harness self-test for variance stability (AC1) | bench | p1 | s | rfc-0009 |
| #25 | bench: cross-engine baseline harness for drift detection (PB17) | bench | p2 | m | rfc-0003 rfc-0009 |
| #107 | convert: on-device quantization to the store (MP12-MP14) | cli | p1 | m | rfc-0006 rfc-0008 |
| #108 | bench: thin CLI wrapper over the benchmark harness (+ --calibrate) | cli | p1 | m | rfc-0008 rfc-0009 |
| #131 | core: add scheduler, KV pool, and energy metric families to the registry | core | p1 | s | rfc-0005 rfc-0007 |
| #4 | docs: write troubleshooting and doctor guide | docs | p2 | s | rfc-0008 rfc-0011 |
| #5 | docs: generate CLI reference from the declarative command definition | docs | p2 | m | rfc-0008 |
| #6 | docs: write HTTP API reference for OpenAI and Anthropic dialects | docs | p1 | m | rfc-0007 |
| #7 | docs: document the machine JSON contract and schema registry | docs | p2 | s | rfc-0004 rfc-0008 |
| #9 | docs: write supported-architecture, quant-format, and tool-dialect matrix | docs | p1 | m | rfc-0006 |
| #10 | docs: publish the honest-speed benchmark methodology page | docs | p1 | m | rfc-0009 |
| #207 | backend-mlx: MoE grouped expert matmul with dual param accounting (IC22) | backend-mlx | p1 | m | rfc-0003 |
| #208 | backend-mlx: Qwen3 MoE architecture graph | backend-mlx | p1 | m | rfc-0003 |
| #209 | backend-mlx: paged attention through the KV block table (IC10) | backend-mlx | p1 | m | rfc-0003 rfc-0005 |
| #210 | backend-mlx: fused paged varlen attention kernel spike and adoption (OQ1) | backend-mlx | p2 | m | rfc-0003 rfc-0009 |
| #211 | backend-mlx: sliding-window ring buffers and hybrid attention interleave (IC11) | backend-mlx | p1 | m | rfc-0003 rfc-0005 |
| #212 | backend-mlx: Gemma hybrid SWA architecture graph | backend-mlx | p1 | m | rfc-0003 |
| #213 | backend-mlx: gpt-oss architecture graph (alternating attention, MXFP4) | backend-mlx | p1 | m | rfc-0003 |
| #214 | backend-mlx: DeepSeek MLA + MoE architecture graph | backend-mlx | p2 | m | rfc-0003 rfc-0005 |
| #215 | backend-mlx: streaming on-device quantization kernels (IC8) | backend-mlx | p1 | m | rfc-0003 |
| #216 | backend-mlx: quantization sensitivity recipes (IC9) | backend-mlx | p2 | s | rfc-0003 |
| #217 | backend-mlx: quantization roundtrip-error and streaming-memory property tests | backend-mlx | p1 | s | rfc-0003 |
| #218 | backend-mlx: prompt-lookup (n-gram) speculation with auto-disable (IC18) | backend-mlx | p1 | m | rfc-0003 |
| #219 | backend-mlx: draft-model speculation (IC19) | backend-mlx | p2 | m | rfc-0003 |
| #220 | backend-mlx: speculative-decode output-equivalence test (IC18-IC19) | backend-mlx | p1 | s | rfc-0003 |
| #221 | backend-mlx: grammar-mask validity corpus (AC3) | backend-mlx | p1 | m | rfc-0003 |
| #222 | backend-mlx: optional second low-priority stream for KV quantize overlap (IC3) | backend-mlx | p2 | s | rfc-0003 |
| #233 | fit: admission-control API against live pool occupancy (FE18) | fit | p0 | m | rfc-0004 rfc-0005 |
| #234 | fit: consume calibration store and flip predictions to calibrated (FE4) | fit | p1 | m | rfc-0004 rfc-0009 |
| #27 | kv-cache: implement paged block pool, block tables, allocator (KV1-KV4) | kv-cache | p0 | l | rfc-0004 rfc-0005 |
| #28 | kv-cache: prefix hash chain, radix index, CoW sharing (KV9-KV12) | kv-cache | p0 | l | rfc-0005 rfc-0006 |
| #29 | kv-cache: 8-bit/4-bit block quantization with fused dequant (KV13-KV16) | kv-cache | p1 | m | rfc-0005 |
| #31 | kv-cache: sliding-window ring buffers with attention sinks (KV6) | kv-cache | p1 | m | rfc-0005 |
| #32 | kv-cache: MLA latent-KV paged layout class (KV7) | kv-cache | p1 | m | rfc-0005 |
| #33 | kv-cache: recurrent/SSM state layout with snapshot/restore (KV8) | kv-cache | p2 | m | rfc-0005 |
| #34 | kv-cache: cost-aware LRU eviction and TTL retention (KV20-KV21) | kv-cache | p1 | m | rfc-0005 |
| #35 | kv-cache: emit KV pool metrics and stats struct (KV23) | kv-cache | p1 | s | rfc-0005 rfc-0007 |
| #36 | bench: KV block-size 16-vs-32 ablation (LD7) | bench | p2 | m | rfc-0005 rfc-0009 |
| #37 | kv-cache: prefix-hit greedy byte-identical correctness test (AC1) | kv-cache | p1 | s | rfc-0005 |
| #38 | kv-cache: CoW non-aliasing and refcount-leak fuzz tests | kv-cache | p1 | m | rfc-0005 |
| #39 | kv-cache: fuzzed correctness-key invalidation property test (AC5) | kv-cache | p1 | s | rfc-0005 |
| #40 | kv-cache: 4-way fan-out shared-prefix accounting test (AC3) | kv-cache | p1 | m | rfc-0005 |
| #82 | backend-gguf: GGUF artifact selection route and bounded metadata parser | backend-gguf | p1 | m | rfc-0006 |
| #83 | models: one-keypress sibling remedy integration with fit report (MP3) | models | p1 | m | rfc-0006 |
| #84 | models: drakkar alias update signed-manifest refresh channel (LD3) | models | p1 | m | rfc-0006 |
| #95 | models: defensive-parser fuzzing for safetensors and GGUF headers (MP8) | backend-gguf | p1 | m | rfc-0006 |
| #96 | models: conversion throughput soak against the >= 1 GB/s target (MP14) | models | p2 | m | rfc-0006 |
| #144 | ci: implement Tier-1 performance gate on self-hosted M-series (RE17.2, PB16) | ci | p0 | m | rfc-0009 rfc-0012 |
| #145 | ci: commission self-hosted M-series runners for perf and startup gates | ci | p1 | m | rfc-0009 rfc-0012 |
| #146 | release: codesign, notarize, staple, checksum, and attest the artifact | release | p1 | m | rfc-0002 rfc-0012 |
| #147 | release: publish benchmark reproducibility manifest with each release (RE30) | release | p1 | s | rfc-0009 rfc-0012 |
| #148 | release: create Homebrew tap and auto-bump the formula on publish (RE21) | release | p1 | m | rfc-0012 |
| #149 | release: complete cargo xtask release driver — tag, push, monitor, waiver | release | p1 | m | rfc-0012 |
| #150 | ci: orchestrate the release-tag pipeline with gating stages (RE17) | ci | p0 | m | rfc-0012 |
| #151 | ci: run package-and-sign dry-run on every merge to main (RE18) | ci | p1 | s | rfc-0012 |
| #152 | ci: clean-VM brew-install smoke gate as post-publish check (RE21, RV26) | ci | p1 | m | rfc-0012 |
| #153 | ci: assert launch-to-server-ready under 200ms on physical hardware (RE3, P12) | ci | p1 | s | rfc-0012 |
| #154 | release: enforce the MLX pin-bump validation checklist (RE10, RV21) | release | p1 | m | rfc-0009 rfc-0010 rfc-0012 |
| #155 | ci: add seeded-failure gate-negative tests proving each gate blocks (RE Testing) | ci | p1 | m | rfc-0009 rfc-0012 |
| #156 | release: document the release checklist runbook in the repository (RE26, RV33) | release | p1 | s | rfc-0012 |
| #47 | grammar: bound structured-output compilation input and wall-clock (SEC24/IC16) | grammar | p1 | m | rfc-0003 rfc-0007 rfc-0011 |
| #55 | backend-mlx: FFI-boundary fuzz + sanitizer coverage as a security gate | backend-mlx | p1 | m | rfc-0002 rfc-0010 |
| #256 | scheduler: token-level continuous batching step loop (AS12) | scheduler | p0 | l | rfc-0007 |
| #257 | scheduler: FIFO two-priority admission ordering, interactive vs batch (AS15) | scheduler | p1 | m | rfc-0007 |
| #258 | scheduler: chunked-prefill interleave under the ITL guard (AS13) | scheduler | p0 | l | rfc-0007 |
| #259 | scheduler: admission control against live pool state with 413/429 (AS14) | scheduler | p0 | m | rfc-0007 |
| #260 | scheduler: disable per-sequence speculation above the occupancy crossover (AS16) | scheduler | p2 | s | rfc-0007 |
| #261 | server: keep-alive model residency with load/unload announcements (AS17) | server | p1 | m | rfc-0007 |
| #262 | scheduler: emit batch/queue/admission/chunk/speculation metrics (AS21) | scheduler | p1 | m | rfc-0007 |
| #263 | server: implement /v1/messages with the full Anthropic streaming event set (AS1) | server | p0 | l | rfc-0007 |
| #264 | server: implement legacy /v1/completions for eval harnesses | server | p1 | s | rfc-0007 |
| #265 | server: tool render into prompt and incremental stream-safe parse (AS9) | server | p1 | l | rfc-0007 |
| #266 | grammar: integrate llguidance to compile schema/regex/lark into token masks | grammar | p1 | m | rfc-0003 |
| #267 | server: response_format json_schema via grammar mask before admission (AS10) | server | p1 | m | rfc-0007 |
| #268 | server: stream reasoning_content/thinking with hide_reasoning config (AS11) | server | p1 | m | rfc-0007 |
| #269 | server: per-request cache opt-out and cache_control hints (AS23, LD8/LD17) | server | p1 | m | rfc-0007 |
| #270 | server: implement POST /fit and enrich /v1/models with pool stats (AS20) | server | p1 | s | rfc-0007 |
| #271 | server: render Anthropic-dialect error envelopes and mid-stream errors (AS8) | server | p1 | m | rfc-0007 |
| #272 | server: golden stream-envelope test for the Anthropic dialect | server | p1 | s | rfc-0007 |
| #273 | server: recorded-trace conformance suite for openai-python (AC1) | server | p1 | m | rfc-0007 |
| #274 | server: recorded-trace conformance suite for the Anthropic SDK (AC1) | server | p1 | m | rfc-0007 |
| #275 | server: recorded-trace conformance suites for Claude Code and OpenCode (AC1) | server | p1 | m | rfc-0007 |
| #276 | server: conformance/multi-model-shape check against a mocked two-model registry | server | p1 | s | rfc-0007 |
| #277 | scheduler: ITL-guard 32k load test (AC2) | scheduler | p0 | m | rfc-0007 |
| #278 | server: disconnect-storm zero-leak test (AC4) | server | p0 | m | rfc-0007 |
| #279 | scheduler: property fuzz that admission never over-commits the pool (AS14) | scheduler | p1 | m | rfc-0007 |
| #280 | grammar: structured-output validity corpus at concurrency 8 (AC3) | grammar | p1 | m | rfc-0007 |
| #281 | server: cache opt-out and hint-behavior tests (AS23) | server | p1 | s | rfc-0007 |
| #282 | scheduler: M4 four-session agent workload product-metric test (AC5) | scheduler | p1 | m | rfc-0007 |
| #64 | server: four-client shape-mode conformance harness + SDK pinning (RFC-0007 AC1) | server | p1 | m | rfc-0007 |
| #65 | engine: mlx-lm reference-parity fixtures + logit-parity harness (TS32) | engine | p1 | m | rfc-0002 rfc-0003 |
| #66 | engine: 24h mixed-load soak harness with RSS-drift + accounting asserts | engine | p1 | m | rfc-0003 |
| #67 | test: chaos fault-injection helpers (signals, fs shim, hub faults) (§9.1) | ci | p1 | m | rfc-0005 rfc-0006 rfc-0007 |
| #68 | core: cargo-fuzz corpus + CI wiring for parsers, grammar, FFI (TS29, TS31) | core | p1 | m | rfc-0006 rfc-0010 |

## Milestone v0.3  (16 issues)

| # | Title | Area | Priority | Effort | Spec |
|---|-------|------|----------|--------|------|
| #186 | backend-mlx: multi-ctx-per-process support + dk_ctx_set_memory_budget (AB19) | backend-mlx | p1 | m | rfc-0010 |
| #187 | backend-mlx: implement dk_kv_scatter for SSD-tier restore (RFC-0005 §6) | backend-mlx | p1 | s | rfc-0005 rfc-0010 |
| #188 | backend-mlx: debug-handle shim soak hook (RSS drift + zero-live-handle) | backend-mlx | p2 | s | rfc-0010 |
| #106 | ps: add multi-model pool occupancy, hit rate, per-model residency | cli | p2 | m | rfc-0008 |
| #109 | cache: ls\|clear the SSD KV tier (KV19) | cli | p2 | m | rfc-0005 rfc-0008 |
| #114 | serve: launchd daemon lifecycle --daemon\|--stop\|--status\|--logs (CLI12-CLI13) | cli | p1 | m | rfc-0007 rfc-0008 |
| #223 | backend-mlx: mean-pool embeddings path (§9) | backend-mlx | p2 | s | rfc-0003 |
| #30 | kv-cache: SSD persistence tier with crash-safe writes (KV17-KV19) | kv-cache | p1 | l | rfc-0005 |
| #85 | models: convert UX polish - presets, dry-run preview, batch, lineage (MP16) | models | p2 | m | rfc-0006 |
| #157 | ci: extend nightly pipeline with soak subset, fuzz corpus, and dry-run (RE16) | ci | p1 | m | rfc-0012 |
| #158 | release: publish signed rolling nightly pre-release via --HEAD tap variant | release | p2 | m | rfc-0012 |
| #159 | ci: verify build attestation as an anonymous client post-publish (RE31) | ci | p1 | s | rfc-0012 |
| #51 | kv-cache: 0600 disk-tier files and diagnostics-bundle exclusion (SEC19/KV19) | kv-cache | p1 | s | rfc-0005 rfc-0011 |
| #283 | server: implement POST /v1/embeddings for embeddings models | server | p1 | m | rfc-0007 |
| #284 | server: implement POST /v1/responses behind a config flag (LD5) | server | p2 | m | rfc-0007 |
| #285 | server: route requests by model to the multi-model engine pool | server | p1 | m | rfc-0007 |

## Milestone v1.0  (6 issues)

| # | Title | Area | Priority | Effort | Spec |
|---|-------|------|----------|--------|------|
| #189 | backend-mlx: freeze the v1.0 ABI and publish embedder documentation (AB8, AB23) | backend-mlx | p0 | m | rfc-0010 rfc-0012 |
| #12 | docs: scaffold the mkdocs documentation site over the spec corpus | docs | p1 | m | rfc-0012 |
| #13 | docs: add CI pipeline to build and publish the documentation site | docs | p1 | m | rfc-0012 |
| #14 | docs: publish per-release benchmark reports from CI manifests | docs | p2 | m | rfc-0009 |
| #160 | release: produce reusable engine library artifact in the package stage (RE29) | release | p1 | m | rfc-0010 rfc-0012 |
| #161 | release: build desktop .dmg with Sparkle auto-update over shared engine (RE28) | release | p2 | l | rfc-0012 |

## Tracking issues

- #286 [Tracking] Core crate, types, error taxonomy, config — 12 child issues
- #287 [Tracking] Backend FFI and the dk_* C ABI — 28 child issues
- #288 [Tracking] Inference core and engine actor — 34 child issues
- #289 [Tracking] Feasibility engine — 16 child issues
- #290 [Tracking] KV cache subsystem — 15 child issues
- #291 [Tracking] Model acquisition and format pipeline — 25 child issues
- #292 [Tracking] API server, scheduler, and structured output — 46 child issues
- #293 [Tracking] CLI and UX — 23 child issues
- #294 [Tracking] Performance harness and calibration — 11 child issues
- #295 [Tracking] Release engineering and CI — 30 child issues
- #296 [Tracking] Security boundaries and hardening — 17 child issues
- #297 [Tracking] Cross-cutting test infrastructure — 14 child issues
- #298 [Tracking] Documentation and developer experience — 13 child issues

## Cross-cutting dependencies

Dependencies below are the load-bearing edges — an issue cannot merge until its dependency has. The full edge set is recorded in each issue's Dependencies section.

- #123 (core: implement DkError, closed ErrorCode registry, and the total exit/HTTP) blocks: #3, #26, #42, #43, #44, #45, #46, #47, #72, #73, #75, #76, #81, #97 …
- #97 (cli: build clap command tree, global flags, dual-render + exit-code framework) blocks: #5, #15, #62, #98, #99, #100, #101, #102, #103, #104, #105, #107, #108, #109 …
- #120 (core: scaffold the eleven-crate Cargo workspace with layered dependency graph) blocks: #58, #59, #60, #61, #69, #97, #121, #123, #127, #129, #132, #133, #134, #136 …
- #15 (bench: implement drakkar bench harness with workloads, variance, result schema) blocks: #10, #14, #16, #17, #18, #19, #20, #21, #22, #23, #24, #25, #36, #66 …
- #195 (backend-mlx: lazy-graph decode step with async eval and prefill path (IC1-IC4)) blocks: #15, #65, #98, #101, #117, #196, #197, #199, #203, #204, #207, #209, #218, #222 …
- #240 (server: stand up axum/tokio server with /health, /v1/models, /metrics) blocks: #2, #48, #49, #50, #56, #63, #66, #113, #114, #153, #242, #243, #247, #248 …
- #76 (models: content-addressed blob/manifest store with HF-cache clone interop and) blocks: #16, #28, #30, #44, #77, #78, #79, #80, #81, #85, #88, #94, #102, #103 …
- #27 (kv-cache: implement paged block pool, block tables, allocator (KV1-KV4)) blocks: #28, #29, #32, #34, #35, #36, #106, #209, #211, #214, #233, #256, #259, #278
- #73 (models: metadata-only fetch and HF token discovery (MP2)) blocks: #9, #28, #31, #32, #33, #42, #43, #68, #74, #77, #99, #194, #226
- #227 (fit: implement weights, everything-else, and GQA KV memory model) blocks: #18, #26, #27, #74, #77, #79, #228, #229, #230, #231, #233, #235, #236
- #77 (models: parallel ranged resumable download with integrity and disk preflight) blocks: #46, #56, #78, #79, #82, #87, #88, #90, #91, #95, #98, #102
- #163 (backend-mlx: build.rs + CMake shim build with pinned MLX and embedded metallib) blocks: #55, #68, #79, #133, #135, #139, #146, #164, #166, #171, #192, #193
- #243 (server: implement /v1/chat/completions streaming + non-streaming (AS1, AS4-AS7)) blocks: #6, #63, #251, #253, #256, #263, #264, #267, #273, #276, #284
- #121 (core: define the shared vocabulary types) blocks: #26, #41, #72, #76, #122, #125, #126, #130, #190, #224, #241
- #241 (server: normalize OpenAI/Anthropic requests into GenerationRequest with) blocks: #242, #243, #244, #245, #246, #263, #264, #265, #268, #269, #283
- #126 (core: implement config.toml load/merge with flags>env>file>defaults precedence) blocks: #48, #52, #72, #73, #76, #100, #110, #240, #247, #284
- #256 (scheduler: token-level continuous batching step loop (AS12)) blocks: #19, #106, #257, #258, #259, #260, #261, #262, #278
- #28 (kv-cache: prefix hash chain, radix index, CoW sharing (KV9-KV12)) blocks: #17, #30, #34, #37, #38, #39, #40, #269
- #146 (release: codesign, notarize, staple, checksum, and attest the artifact) blocks: #84, #148, #150, #151, #158, #159, #161, #189
- #263 (server: implement /v1/messages with the full Anthropic streaming event set (AS1)) blocks: #6, #64, #271, #272, #274, #275, #276

## Labels

Each issue carries exactly one `type:`, one `area:`, one `priority:`, and one `effort:` label, plus one or more `spec:rfc-NNNN` labels tying it to its decision record. Filter the [issue list](https://github.com/AbdelStark/drakkar/issues) by any combination to slice the roadmap.
