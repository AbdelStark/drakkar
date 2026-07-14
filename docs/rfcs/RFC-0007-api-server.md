# RFC-0007: API Server and Scheduler

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.2

## Summary

`drakkar serve` exposes the engine over HTTP in the two dialects agent ecosystems actually speak (OpenAI and Anthropic), backed by a continuous-batching scheduler whose contract is: **a long prefill must never wreck another stream's inter-token latency, and no admitted request may die of memory.** This RFC fixes the endpoint surface, streaming semantics, tool/structured-output behavior, the scheduling policy, the security posture, and observability. Requirements are numbered AS1-AS23; the source acceptance criteria AC1-AC5 are folded into [Testing Strategy](#testing-strategy). The server lives in the `drakkar-server` crate, the scheduler in `drakkar-sched` (LD24); both sit above the engine actor of [RFC-0001](RFC-0001-architecture.md#proposed-design) and delegate admission arithmetic to the fit engine ([RFC-0004](RFC-0004-feasibility-engine.md#proposed-design) FE18) and block accounting to the KV pool ([RFC-0005](RFC-0005-kv-cache.md#proposed-design)).

## Motivation

PRD G3 ([PRD §4](../../PRD.md#4-goals-and-non-goals)) commits DRAKKAR to agent-grade serving: OpenAI + Anthropic compatible endpoints, continuous batching, structured output, tool calling, and streaming with usage accounting. PRD P4 and P5 ([PRD §5.1](../../PRD.md#51-functional)) make two parts of that a MUST: the dual-dialect endpoint surface with SSE streaming, tools, and JSON-schema output, and concurrency through continuous batching with chunked prefill "so that a long prompt from one client does not stall another client's decode." This RFC is the specification of both.

The competitive motivation is the fragmented-API gap documented in [PRD §2.3](../../PRD.md#23-where-existing-tools-fall-short): agent ecosystems now expect both the OpenAI (`/v1/chat/completions`) and Anthropic (`/v1/messages`) shapes, and support across incumbents is inconsistent — partial dialect coverage, silent parameter drops, divergent streaming envelopes. The primary target user ([PRD §3](../../PRD.md#3-target-users), the agent builder) points coding agents and multi-agent loops at localhost and needs concurrent requests without ITL collapse, both dialects, and structured output that never breaks JSON.

The quantitative bar is PRD success metric M4 ([PRD §7](../../PRD.md#7-success-metrics)): 4 concurrent coding-agent sessions against one 30B-A3B model on M4 Max sustain per-stream ITL under 45 ms with warm-prefix TTFT under 500 ms. Every scheduler decision in this RFC (chunked-prefill interleave, the ITL guard, admission control, the two-priority queue) exists to hit that number without violating the memory-safety contract (PRD P11, RFC-0001 I2). The honest-speed principle from [PRD §1](../../PRD.md#1-vision) also lands here as an API rule: no silently accepted parameters the engine does not honor (AS2), and usage accounting the caller can audit (AS6).

## Goals

- Both dialects conformant against recorded traces from reference clients — the official OpenAI and Anthropic Python SDKs plus two coding-agent CLIs — passing unmodified against localhost (AC1).
- The ITL guard holds: a 32k-token prompt admitted mid-stream inflates a concurrent decode stream's p95 ITL by ≤ 25% (AC2).
- Structured output is guaranteed, not best-effort: 0 schema violations across the structured-output corpus at concurrency 8 (AC3).
- Cancellation is leak-free: 100 mid-stream disconnects leak zero KV blocks (AC4).
- The M4 product metric is met on the reference fleet (AC5).
- Every error is structured and remediable: a rejected caller learns what to change, in its own dialect's error envelope (AS8, [RFC-0011](RFC-0011-error-taxonomy.md#proposed-design)).
- Admission control never over-admits: no admitted request can exhaust the pool mid-generation (AS14, FE18, PRD P11).
- Safe-by-default network posture: loopback bind, key-gated non-loopback exposure, no body logging by default (AS18-AS19, LD22).

## Non-Goals

- **TLS termination.** The server speaks plain HTTP; exposing it beyond the machine goes through a reverse proxy, and the docs describe that pattern (AS18). Bundling a TLS stack adds certificate lifecycle surface with no benefit for the localhost-first product.
- **Multi-tenant fairness and organizational authn/z** (PRD N3). One local API key is the entire identity model; there are no per-tenant quotas, no fairness scheduling across principals. The two-priority queue (AS15) is a UX device, not a tenancy feature.
- **Ollama-compatible `/api/*` shims.** Explicitly deferred per AS3 (mlx-serve demonstrates the value; revisit at v0.3 with adoption evidence). The two dialects in scope are the ones agent frameworks standardize on.
- **WebSocket transport.** SSE only; see [Alternatives Considered](#alternatives-considered).
- **`n > 1` sampling** in v1 (AS7; v1.x).
- **Priority preemption of running sequences** in v1 (AS15, [RFC-0005](RFC-0005-kv-cache.md#proposed-design) KV21; v1.x scheduler feature). Admission control is the only backpressure mechanism.

## Proposed Design

The subsections keep their source numbering; other RFCs cite them by § number (RFC-0003 IC12 cites §6, RFC-0004 FE27 cites §8, RFC-0005 KV23 cites §9).

### 2. Endpoint surface (v1)

| Endpoint | Dialect | Notes |
| --- | --- | --- |
| `POST /v1/chat/completions` | OpenAI | streaming + non-streaming; tools; response_format json_schema; logprobs |
| `POST /v1/completions` | OpenAI legacy | text completion for eval harnesses |
| `GET /v1/models` | OpenAI | resident + installed models |
| `POST /v1/messages` | Anthropic | messages, system, tools, streaming event set; enough for coding-agent-class clients to point at localhost |
| `POST /v1/embeddings` | OpenAI | v0.3, embeddings models |
| `POST /fit` | DRAKKAR | RFC-0004 FE26 schema |
| `GET /health`, `GET /metrics` | ops | liveness + Prometheus |

- AS1. Requests in either dialect normalize to one internal `GenerationRequest`; responses render back in the caller's dialect, including its streaming envelope (OpenAI SSE `chat.completion.chunk` with a final `usage` frame; Anthropic `message_start/content_block_delta/message_delta/message_stop` events). Dialect fidelity is tested against recorded traces from reference clients (openai-python, the Anthropic Python SDK, and the Claude Code and OpenCode agent CLIs).
- AS2. Unsupported dialect fields fail loud (`400` naming the field) unless explicitly ignorable; silent acceptance of ignored parameters is forbidden (honesty rule). The set of explicitly-ignorable fields per dialect is a versioned table in `drakkar-server`, and each entry carries the reason it is safe to ignore.
- AS3. `model` field: resident model name, installed model (triggers load per keep-alive policy), or `default`. Ollama-compatible `/api/*` shims are explicitly deferred (mlx-serve demonstrates value; revisit v0.3).

The internal request type both dialects normalize into (abridged; canonical definition in `drakkar-core`):

```rust
pub struct GenerationRequest {
    pub model: ModelRef,                    // resolved name, "default", or installed ref
    pub prompt: RenderedPrompt,             // post-template token ids + prefix-hash chain (RFC-0005 §3)
    pub sampling: SamplingParams,           // AS7 set; maps to IC14/IC15 pipeline
    pub max_tokens: u32,                    // clamped to model context; clamp surfaces in finish_reason
    pub stop: StopSpec,                     // strings + token ids
    pub stream: bool,
    pub tools: Option<ToolSpec>,            // dialect-normalized tool defs (AS9, MP18)
    pub response_format: Option<Grammar>,   // compiled per IC16 before admission
    pub cache: CachePolicy,                 // AS23: default Auto; Off honors LD8
    pub priority: Priority,                 // Interactive | Batch (AS15)
    pub dialect: Dialect,                   // OpenAiChat | OpenAiCompletions | Anthropic — response rendering only
}
```

`dialect` influences rendering exclusively; nothing downstream of normalization branches on it. That one-way flow is the invariant that keeps the two surfaces from drifting apart semantically (name: **INV-DIALECT-ONEWAY**).

### 3. Streaming semantics

- AS4. SSE with heartbeats every 10 s of silence; client disconnect cancels the sequence within one decode step and frees/donates its blocks ([RFC-0005](RFC-0005-kv-cache.md#proposed-design) KV11).
- AS5. First streamed frame target: ≤ 50 ms after first sampled token (transport must never dominate TTFT). Chunk coalescing max 10 ms.
- AS6. Usage accounting on every completion (and final stream frame): prompt tokens, cached prompt tokens (prefix hits, surfaced as `prompt_tokens_details.cached_tokens` / Anthropic `cache_read_input_tokens`), completion tokens, and DRAKKAR extensions under `x_drakkar`: ttft_ms, itl_ms_p50, kv_cache_hit_ratio.

Final OpenAI stream frame, abridged (the Anthropic equivalent carries the same figures in `message_delta.usage`):

```json
{
  "object": "chat.completion.chunk",
  "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
  "usage": {
    "prompt_tokens": 4210,
    "prompt_tokens_details": {"cached_tokens": 3968},
    "completion_tokens": 187,
    "total_tokens": 4397,
    "x_drakkar": {"ttft_ms": 312, "itl_ms_p50": 21.4, "kv_cache_hit_ratio": 0.94}
  }
}
```

The `x_drakkar` namespace is additive and versioned with the server; clients MUST tolerate unknown keys inside it, and DRAKKAR MUST NOT place extensions outside that namespace (invariant **INV-EXT-NAMESPACED**), so both dialects stay parseable by strict upstream SDKs.

### 4. Sampling, limits, errors

- AS7. Honored parameters: temperature, top_p, top_k, min_p, max_tokens (with model-context clamp + explicit finish_reason), stop (strings and token ids), seed, presence/frequency/repetition penalties, logit_bias, logprobs/top_logprobs, n=1 (n>1 v1.x). Parameter semantics map one-to-one onto the on-GPU pipeline of [RFC-0003](RFC-0003-inference-core.md#5-sampling-pipeline) IC14-IC15.
- AS8. Errors are structured and remediable: `413 context_exceeded` carries `max_admissible_tokens`; `429 kv_pool_exhausted` carries `retry_after_ms` and current occupancy; `503 model_loading` carries progress. Error bodies match each dialect's error envelope; the taxonomy, codes, and per-dialect envelope mapping are normative in [RFC-0011](RFC-0011-error-taxonomy.md#proposed-design) and [the error model](../spec/04-error-model.md#3-http-status-mapping-and-dialect-error-types). Example (OpenAI envelope):

```json
{
  "error": {
    "message": "KV pool exhausted: 94% occupancy, 12288 tokens reclaimable",
    "type": "kv_pool_exhausted",
    "code": "kv_pool_exhausted",
    "x_drakkar": {"retry_after_ms": 1800, "pool_occupancy": 0.94}
  }
}
```

- AS23. Cache controls. Caching is fully automatic; two request-level controls modulate it. (a) Opt-out (LD8): `cache: false` in the request body, or the `X-Drakkar-Cache: off` header, prevents the sequence's blocks from being donated to the cached state on completion — even in RAM — and skips SSD persistence; prefix *reads* still occur unless the same control sets `read: false`. (b) Hints (LD17): Anthropic `cache_control` markers are accepted on any request and honored as retention hints — a marked prefix boundary raises the retention score of the covered block run ([RFC-0005 §7](RFC-0005-kv-cache.md#proposed-design)) — but never create obligations: no cache-write billing semantics, no TTL contract, no error if the hint cannot be honored. Presence or absence of `cache_control` MUST NOT change response content. `cache_control` is therefore an honored parameter, not an ignorable one, under AS2.

### 5. Tools, structured output, reasoning content

- AS9. Tool calling: tools render into the prompt via the model's declared dialect ([RFC-0006](RFC-0006-model-pipeline.md#proposed-design) MP18); output parsing is incremental and stream-safe (tool-call markup suppressed from visible deltas, structured `tool_calls` emitted on completion of each call, parallel calls supported where the model family does). Families without native tool training MAY opt into a constrained-decoding tool harness (grammar-forced call syntax) flagged in capabilities.
- AS10. `response_format: {type: "json_schema"}` compiles through llguidance to a token mask ([RFC-0003](RFC-0003-inference-core.md#5-sampling-pipeline) IC16): schema-valid output is guaranteed, not retried. `json_object` mode maps to a permissive JSON grammar. Grammar compilation happens before admission so a pathological schema fails fast with `400 invalid_schema` rather than stalling a scheduled sequence.
- AS11. Reasoning/thinking blocks stream as `reasoning_content` (OpenAI-style extension) / Anthropic `thinking` deltas per family dialect, with a server-level `hide_reasoning` config for clients that must not receive it. Hidden reasoning tokens still count in usage (honesty rule: the caller paid the latency).

### 6. Scheduler policy

- AS12. **Continuous batching** at the token level: the decode batch recomposes every step; new sequences join after their prefill completes; finished sequences exit without draining the batch.
- AS13. **Chunked prefill interleave:** prefill work is sliced ([RFC-0003](RFC-0003-inference-core.md#4-attention-and-prefill) IC12) and scheduled between decode steps under an ITL guard: target p95 ITL inflation ≤ 25% versus solo decode at reference concurrency. The chunk budget adapts (256-2048 tokens) from the measured decode-step time; the guard, not throughput, is the binding constraint (agent UX dies by jitter, not by mean).
- AS14. Admission control delegates to the fit engine against live pool state ([RFC-0004](RFC-0004-feasibility-engine.md#proposed-design) FE18); `max_concurrency` default 8 (config), with per-request KV reservations covering `prompt + max_tokens`.
- AS15. Queueing: FIFO within priority; two priorities (`interactive` default, `batch` via header) so background evals never starve a chat. No preemption in v1 ([RFC-0005](RFC-0005-kv-cache.md#proposed-design) KV21).
- AS16. Speculation interplay per [RFC-0003](RFC-0003-inference-core.md#proposed-design) IC21: scheduler disables per-sequence speculation above the calibrated occupancy crossover.
- AS17. Keep-alive: models unload after `keep_alive` idle (default 30 min for `serve`, immediate for one-shot); `drakkar ps` shows residency; load/unload transitions are announced on `/metrics` and logs.

The scheduler step loop (normative shape; `drakkar-sched`):

```text
loop:
  1. reap finished/cancelled sequences → free or donate blocks (KV11, AS23)
  2. admit from queue (interactive first, FIFO within class) while FE18 says
     kv_needed(prompt + max_tokens) fits free + reclaimable blocks
  3. compute prefill chunk budget from EWMA(decode_step_ms) vs ITL-guard target
  4. run one prefill chunk for the oldest admitted-but-unprefilled sequence, if budget > 0
  5. recompose decode batch (bucketed shape, IC2) and run one decode step
  6. emit sampled tokens to per-sequence SSE renderers; update metrics (AS21)
```

The guard invariant (name: **INV-ITL-GUARD**): the prefill budget in step 3 is derived so that projected step time ≤ (1 + 0.25) × solo-decode step time at the current batch; if the projection cannot be met with the minimum 256-token chunk, prefill waits — decode never does.

### 7. Security and operational posture

- AS18. Bind 127.0.0.1:11711 by default (LD22). `--host 0.0.0.0` requires `--api-key` (or config equivalent); keys check via constant-time compare; CORS opt-in with explicit origins. TLS is out of scope (document reverse-proxy pattern). See [security posture](../spec/06-security.md#21-b1-network-edge).
- AS19. Request logging default: metadata only (no prompt/completion bodies); `--log-bodies` explicit and marked sensitive. Rate limiting by client IP available but off by default (localhost reality).

### 8. `/fit` and management surface

- AS20. `POST /fit` mirrors the CLI ([RFC-0004](RFC-0004-feasibility-engine.md#proposed-design) FE25-FE26) so agents can plan model choice programmatically; `GET /v1/models` includes per-model `x_drakkar` fields: format, quant, ctx_max at current KV precision, resident state, pool stats summary.

### 9. Observability

- AS21. Prometheus metrics: request counts/latency by endpoint and outcome; TTFT and ITL histograms split cold/warm; prefill and decode tokens/s; batch occupancy; queue depth; KV metrics ([RFC-0005](RFC-0005-kv-cache.md#proposed-design) KV23); memory contract vs actual; energy counters when sampling is enabled.
- AS22. `tracing` structured logs with request ids; `--otel` OTLP export optional.

## Alternatives Considered

- **Proxy-translate one dialect onto the other at the edge** (implement OpenAI natively, rewrite Anthropic requests into it, or vice versa). Superficially halves the surface, but the translation is lossy where it matters most for agents: the tool-call lifecycles differ (Anthropic's `tool_use`/`tool_result` content blocks vs OpenAI's `tool_calls` array and role conventions), streaming event grammars differ structurally (typed content-block events vs uniform chunks), and Anthropic `cache_control` has no OpenAI carrier at all — a proxy either drops it (violating LD17) or invents a private encoding that leaks into conformance behavior. Rejected: AS1 normalizes both dialects into one internal `GenerationRequest` that is a superset of both, and renders responses natively per dialect. The internal type is the single source of semantics; dialects are codecs (INV-DIALECT-ONEWAY).
- **WebSockets for streaming.** Bidirectional framing would make cancellation explicit and heartbeats unnecessary, but SSE is what the ecosystem's SDKs actually speak: both reference SDKs, every agent framework in the conformance set, and the upstream cloud APIs stream over SSE. A WebSocket surface would be a dialect no client uses without custom integration — the opposite of the drop-in-localhost promise. Rejected; AS4-AS5 specify SSE with heartbeats, and disconnect detection via the closed connection covers cancellation within one decode step.
- **Static prefill/decode phases** (serve prefills to completion, then decode; or alternate fixed phases). Simpler scheduler, and it maximizes prefill kernel efficiency by never slicing prompts. But it recreates the convoy effect that continuous-batching literature exists to kill: one 32k-token prompt monopolizes the engine for its full prefill while every active stream's ITL spikes by the whole prefill duration — precisely the failure PRD P5 forbids and the Sarathi-style chunked-prefill line of work measured. Rejected; AS12-AS13 adopt token-level continuous batching with chunked prefill interleave under the ITL guard.
- **Priority preemption in v1** (evict or pause a running batch sequence when an interactive request arrives). Bounds interactive queueing delay under saturation, but requires suspend/resume of sequences with their KV state — an offload/restore mechanism the pool deliberately does not have in v1 (KV21: active blocks are never reclaimed), plus recompute-vs-swap policy and a starvation story for the preempted class. The complexity lands in the two subsystems with the strictest invariants (scheduler, pool) for a scenario admission control already bounds. Rejected for v1; two-priority admission ordering (AS15) ships instead, and preemption is a v1.x scheduler feature per KV21.

## Drawbacks

- **Dialect fidelity is a permanent conformance burden.** Two dialects rendered natively means every upstream API evolution (new fields, new stream event types, SDK-side strictness changes) lands as work here, forever. The recorded-trace suites (AC1) pin fidelity but also pin effort: traces must be re-recorded as reference clients release, and a trace refresh can surface breaking drift at any time. This is the accepted price of drop-in compatibility; the ignorable-fields table (AS2) at least makes the gap explicit rather than silent.
- **The ITL guard sacrifices peak throughput.** Capping prefill interleave at ≤ 25% p95 ITL inflation leaves prefill throughput on the table whenever decode streams are active: a benchmark that measures aggregate tokens/s under mixed load will show DRAKKAR below an unguarded scheduler. This is a deliberate product trade (agent UX dies by jitter, AS13), but it must be defended with published numbers rather than hidden — RFC-0009 reports both the guarded figure and the guard-off ceiling.
- **Single-port multi-dialect complicates error envelopes.** One error taxonomy must render into two incompatible envelope shapes, including for failures that occur before dialect detection is certain (malformed body on `/v1/chat/completions` vs `/v1/messages`), and ops tooling scraping `/metrics` sees one status-code space across both. [RFC-0011](RFC-0011-error-taxonomy.md#proposed-design) owns the mapping; the residual cost is that every new error variant is specified twice and golden-tested twice.
- **Two priorities without preemption is admission-time fairness only.** A burst of `batch` requests admitted during an idle moment holds its pool reservations until completion; an `interactive` request arriving one step later waits behind resources already committed (AS14). The guard bounds its ITL once running, but not its queueing delay under saturation. Documented honestly; preemption is the v1.x answer (KV21).

## Migration / Rollout

- **v0.1 "First light".** Ships the minimal single-request surface: `POST /v1/chat/completions` (streaming and non-streaming, AS4-AS7 semantics, no tools, no `response_format`), `GET /v1/models`, `GET /health`. The scheduler is degenerate: `max_concurrency = 1`, admission is a single FE18 check, no batching, no priorities. The dialect-normalization layer (AS1) and the SSE renderer land in full so v0.2 adds surface without reshaping v0.1 responses. AS2 (fail-loud fields), AS18-AS19 (bind/key/logging posture, LD22), and the `x_drakkar` usage extensions (AS6) apply from v0.1.
- **v0.2 "Convoy" (this RFC's target).** The full surface and the real scheduler: Anthropic `POST /v1/messages` with its complete streaming event set, tool calling (AS9), `json_schema` structured output (AS10), reasoning-content streaming (AS11), continuous batching + chunked prefill + ITL guard (AS12-AS13), admission control against live pool state (AS14), priorities (AS15), speculation interplay (AS16), keep-alive residency (AS17), cache controls (AS23, LD8/LD17), `POST /fit` (AS20), `GET /metrics` (AS21), and `POST /v1/completions` for eval harnesses. AC1-AC5 gate the v0.2 release.
- **v0.3 "Fleet".** `POST /v1/embeddings` (embeddings models, batch-oriented path per RFC-0003 §9); `POST /v1/responses` behind the `api.responses_endpoint` config flag (LD5, default off); multi-model routing on one port — the `model` field selects the target engine in the multi-model pool (RFC-0001 LD12), with per-model keep-alive and `GET /v1/models` reflecting pool residency. Daemon mode (launchd) changes process supervision, not API shape. Revisit Ollama `/api/*` shims here with adoption evidence (AS3).
- **v1.0 "Harbor".** The surface freezes: dialect coverage, error envelopes, and the `x_drakkar` namespaces become stability-guaranteed for the desktop app and third-party integrations; conformance suites become release gates on the hardware fleet CI (RFC-0009).
- **Compatibility commitment.** Nothing in the v0.1-v0.2 API shape may block multi-model serving: the `model` field is already the routing key on every endpoint, every response echoes the serving model, `GET /v1/models` is plural by construction, and no header, path, or error body assumes a single resident engine. This is asserted by a v0.2 conformance check (`conformance/multi-model-shape`) that replays the v0.2 golden fixtures against a mocked two-model registry.

Feature flags and schema versioning: `api.responses_endpoint` (LD5, v0.3), `hide_reasoning` (AS11), `--log-bodies` (AS19), rate limiting (AS19) are config-gated. The `/fit` body follows the FE26 schema version; `x_drakkar` extension blocks are additive-only within a minor version (INV-EXT-NAMESPACED).

## Testing Strategy

Release acceptance criteria (from the source RFC, gating v0.2 as written):

- AC1. Dialect conformance: recorded-trace suites for openai-python, the Anthropic Python SDK, Claude Code, and OpenCode pass unmodified against localhost.
- AC2. ITL guard: with one 32k-token prompt admitted mid-stream, a concurrent decode stream's p95 ITL inflates ≤ 25% (reference fleet, RFC-0009 workload C).
- AC3. Guaranteed JSON: 0 schema violations across the structured-output corpus at concurrency 8.
- AC4. Disconnect storm: 100 mid-stream disconnects leak zero blocks (pool accounting equality after quiesce).
- AC5. M4 product metric: 4 concurrent agent sessions on 30B-A3B (M4 Max) hold per-stream ITL < 45 ms with warm-prefix TTFT < 500 ms.

Named suites behind them:

- **Conformance (AC1).** One recorded-trace suite per reference client: `conformance/trace-openai-python`, `conformance/trace-anthropic-sdk`, `conformance/trace-claude-code`, `conformance/trace-opencode`. Each suite replays captured client request sequences (chat, streaming, tools, structured output, error paths) against `drakkar serve` and validates response *shape* against the recorded envelopes — field presence, types, event grammar — not content. Traces are versioned artifacts pinned to the client release that produced them; refreshing a trace is a reviewed change.
- **Streaming-envelope golden tests.** `golden/stream-envelope-openai` and `golden/stream-envelope-anthropic`: fixed seeded generations asserting the exact event sequence per dialect — OpenAI: role-bearing first chunk, content deltas, `finish_reason` chunk, final `usage` frame (AS6 fields present), `[DONE]` terminator; Anthropic: `message_start` → `content_block_start/delta/stop` (including `thinking` and `tool_use` block variants) → `message_delta` with usage → `message_stop`. Heartbeat frames (AS4) asserted under an artificially stalled generation. Any envelope change diffs a golden file.
- **Unit.** `dialect-field-matrix` (AS2): every known field of both dialects classified rejected/honored/ignorable, with a `400` asserted for each rejected field naming it, and a CI check that a new SDK field cannot enter the ignorable table without a reason string. `max-tokens-clamp`: context-boundary clamp surfaces the documented `finish_reason`. `stop-across-chunks`: stop strings split across streamed chunk boundaries are suppressed from output in both dialects.
- **Load (AC2 procedure).** `load/itl-guard-32k`: start one decode stream at reference concurrency, record 60 s of baseline ITL; admit a single 32k-token prompt mid-stream; assert p95 ITL over the interleave window ≤ 1.25 × baseline p95, on RFC-0009 workload C machines. Run at both priority classes to confirm the guard is priority-independent.
- **Leak (AC4 procedure).** `leak/disconnect-storm`: 100 concurrent streaming clients; kill each TCP connection at a uniformly random point in its stream (during prefill, mid-decode, during tool-call emission); wait for scheduler quiesce (queue empty, batch empty); assert pool accounting equality — `free + cached + active == total_blocks` with `active == 0` — and zero refcount warnings in logs. Repeated 10× in CI; also folded into the 24 h soak (PRD P14).
- **Property.** `prop/admission-fuzz`: randomized request streams (prompt lengths, `max_tokens`, priorities, cancellations) against randomized synthetic pool states; the scheduler MUST never admit a request whose `kv_needed(prompt + max_tokens)` exceeds free + reclaimable blocks (oracle: the FE18 arithmetic evaluated independently), and MUST never reach a state where an admitted sequence's reservation cannot be satisfied.
- **Security.** `sec/api-key-constant-time`: statistical timing test over the key-compare path — response-time distributions for correct-prefix vs wrong-first-byte keys are indistinguishable at a pre-registered significance level (AS18). Plus: non-loopback bind without a key MUST refuse to start; CORS denied by default.
- **Cache controls (AS23).** `cache/opt-out` (LD8): a `cache: false` request's blocks are freed, not donated — asserted by pool-state inspection and by a follow-up identical prompt measuring a cold prefix. `cache/hint-behavior` (LD17): byte-identical responses with and without `cache_control` markers; a marked prefix outlives an unmarked equal-score peer under induced pool pressure; malformed `cache_control` never errors the request.
- **Structured output (AC3).** The RFC-0003 grammar corpus driven through the API at concurrency 8, mixed with unconstrained traffic: 0 schema violations, no flake budget.
- **Product metric (AC5).** `fleet/m4-agent-workload`: the PRD M4 scenario scripted as 4 concurrent tool-loop sessions (shared scaffold prefix, per RFC-0009 workload C) on the M4 Max reference machine; per-stream ITL < 45 ms and warm-prefix TTFT < 500 ms, reported with the LD18 reproducibility manifest.
- **Soak.** 24 h mixed A-E workload (RFC-0009) through the server: zero request failures, RSS drift < 2% post-warmup (PRD P14), pool accounting equality checked hourly.

## Open Questions

None kept open. The source draft's three questions are resolved:

| Source question | Resolution |
| --- | --- |
| `/v1/responses` (OpenAI Responses API) — surface cost vs agent adoption (PRD OQ3) | Resolved per LD5: ships in v0.3 behind the `api.responses_endpoint` config flag, default off. See [Migration / Rollout](#migration--rollout). |
| Anthropic prompt-caching controls — map onto KV donate/retain, or stay automatic? | Resolved per LD17: caching stays fully automatic; explicit `cache_control` is honored as retention hints with no billing/TTL semantics. Specified as AS23, tested by `cache/hint-behavior`. |
| Does any v1 API shape block multi-model routing on one port? | Confirmed non-blocking: `model` is already the routing key on every endpoint, responses echo the serving model, `/v1/models` is plural, and no path/header/error shape assumes a single engine. Multi-model routing lands v0.3 with the pool (LD12); the `conformance/multi-model-shape` check enforces the commitment from v0.2. |

## References

- [PRD](../../PRD.md) — G3, P4/P5, P11, P14, §2.3 (fragmented API surfaces), §3 (agent builder), M4
- [RFC-0001: Architecture](RFC-0001-architecture.md) (engine actor, I2 memory contract, LD12 multi-model pool), [RFC-0003: Inference Core](RFC-0003-inference-core.md) (IC12 chunked prefill, IC14-IC16 sampling and grammar masks, IC21 speculation crossover), [RFC-0004: Feasibility Engine](RFC-0004-feasibility-engine.md) (FE18 admission, FE25-FE26 fit schema), [RFC-0005: KV Cache](RFC-0005-kv-cache.md) (KV11 sharing/donation, KV21 reclaim order, KV23 metrics), [RFC-0006: Model Pipeline](RFC-0006-model-pipeline.md) (MP18 chat templates and tool dialects), [RFC-0009: Performance](RFC-0009-performance.md) (workload C, reference fleet, LD18 manifests), [RFC-0011: Error Taxonomy](RFC-0011-error-taxonomy.md)
- OpenAI API reference (chat completions, streaming, structured outputs); Anthropic Messages API reference (2026)
- vllm-mlx and mlx-serve dual-dialect precedents; LM Studio compatibility-endpoint coverage notes (2026)
- Agrawal et al., "Sarathi-Serve: chunked prefill" (2024); vLLM continuous batching literature
- WWDC26 session 232 (agent fan-out against a local server as the reference workload)
