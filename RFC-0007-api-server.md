# RFC-0007: API Server and Scheduler

**Status:** Draft
**Author:** A. Bakhta
**Created:** 2026-07-14
**Requires:** RFC-0001, RFC-0003, RFC-0004, RFC-0005

## 1. Summary

`drakkar serve` exposes the engine over HTTP in the two dialects agent ecosystems actually speak (OpenAI and Anthropic), backed by a continuous-batching scheduler whose contract is: **a long prefill must never wreck another stream's inter-token latency, and no admitted request may die of memory.** This RFC fixes the endpoint surface, streaming semantics, tool/structured-output behavior, the scheduling policy, and observability.

## 2. Endpoint surface (v1)

| Endpoint | Dialect | Notes |
| --- | --- | --- |
| `POST /v1/chat/completions` | OpenAI | streaming + non-streaming; tools; response_format json_schema; logprobs |
| `POST /v1/completions` | OpenAI legacy | text completion for eval harnesses |
| `GET /v1/models` | OpenAI | resident + installed models |
| `POST /v1/messages` | Anthropic | messages, system, tools, streaming event set; enough for Claude-Code-class clients to point at localhost |
| `POST /v1/embeddings` | OpenAI | v0.3, embeddings models |
| `POST /fit` | DRAKKAR | RFC-0004 FE26 schema |
| `GET /health`, `GET /metrics` | ops | liveness + Prometheus |

- AS1. Requests in either dialect normalize to one internal `GenerationRequest`; responses render back in the caller's dialect, including its streaming envelope (OpenAI SSE `chat.completion.chunk` with a final `usage` frame; Anthropic `message_start/content_block_delta/message_delta/message_stop` events). Dialect fidelity is tested against recorded traces from reference clients (openai-python, anthropic sdk, Claude Code, OpenCode).
- AS2. Unsupported dialect fields fail loud (`400` naming the field) unless explicitly ignorable; silent acceptance of ignored parameters is forbidden (honesty rule).
- AS3. `model` field: resident model name, installed model (triggers load per keep-alive policy), or `default`. Ollama-compatible `/api/*` shims are explicitly deferred (mlx-serve demonstrates value; revisit v0.3).

## 3. Streaming semantics

- AS4. SSE with heartbeats every 10 s of silence; client disconnect cancels the sequence within one decode step and frees/donates its blocks (KV11).
- AS5. First streamed frame target: ≤ 50 ms after first sampled token (transport must never dominate TTFT). Chunk coalescing max 10 ms.
- AS6. Usage accounting on every completion (and final stream frame): prompt tokens, cached prompt tokens (prefix hits, surfaced as `prompt_tokens_details.cached_tokens` / Anthropic `cache_read_input_tokens`), completion tokens, and DRAKKAR extensions under `x_drakkar`: ttft_ms, itl_ms_p50, kv_cache_hit_ratio.

## 4. Sampling, limits, errors

- AS7. Honored parameters: temperature, top_p, top_k, min_p, max_tokens (with model-context clamp + explicit finish_reason), stop (strings and token ids), seed, presence/frequency/repetition penalties, logit_bias, logprobs/top_logprobs, n=1 (n>1 v1.x).
- AS8. Errors are structured and remediable: `413 context_exceeded` carries `max_admissible_tokens`; `429 kv_pool_exhausted` carries `retry_after_ms` and current occupancy; `503 model_loading` carries progress. Error bodies match each dialect's error envelope.

## 5. Tools, structured output, reasoning content

- AS9. Tool calling: tools render into the prompt via the model's declared dialect (MP18); output parsing is incremental and stream-safe (tool-call markup suppressed from visible deltas, structured `tool_calls` emitted on completion of each call, parallel calls supported where the model family does). Families without native tool training MAY opt into a constrained-decoding tool harness (grammar-forced call syntax) flagged in capabilities.
- AS10. `response_format: {type: "json_schema"}` compiles through llguidance to a token mask (IC16): schema-valid output is guaranteed, not retried. `json_object` mode maps to a permissive JSON grammar.
- AS11. Reasoning/thinking blocks stream as `reasoning_content` (OpenAI-style extension) / Anthropic `thinking` deltas per family dialect, with a server-level `hide_reasoning` config for clients that must not receive it.

## 6. Scheduler policy

- AS12. **Continuous batching** at the token level: the decode batch recomposes every step; new sequences join after their prefill completes; finished sequences exit without draining the batch.
- AS13. **Chunked prefill interleave:** prefill work is sliced (IC12) and scheduled between decode steps under an ITL guard: target p95 ITL inflation ≤ 25% versus solo decode at reference concurrency. The chunk budget adapts (256-2048 tokens) from the measured decode-step time; the guard, not throughput, is the binding constraint (agent UX dies by jitter, not by mean).
- AS14. Admission control delegates to the fit engine against live pool state (FE18); `max_concurrency` default 8 (config), with per-request KV reservations covering `prompt + max_tokens`.
- AS15. Queueing: FIFO within priority; two priorities (`interactive` default, `batch` via header) so background evals never starve a chat. No preemption in v1 (KV21).
- AS16. Speculation interplay per IC21: scheduler disables per-sequence speculation above the calibrated occupancy crossover.
- AS17. Keep-alive: models unload after `keep_alive` idle (default 30 min for `serve`, immediate for one-shot); `drakkar ps` shows residency; load/unload transitions are announced on `/metrics` and logs.

## 7. Security and operational posture

- AS18. Bind 127.0.0.1:11711 by default. `--host 0.0.0.0` requires `--api-key` (or config equivalent); keys check via constant-time compare; CORS opt-in with explicit origins. TLS is out of scope (document reverse-proxy pattern).
- AS19. Request logging default: metadata only (no prompt/completion bodies); `--log-bodies` explicit and marked sensitive. Rate limiting by client IP available but off by default (localhost reality).

## 8. `/fit` and management surface

- AS20. `POST /fit` mirrors the CLI (FE25-FE26) so agents can plan model choice programmatically; `GET /v1/models` includes per-model `x_drakkar` fields: format, quant, ctx_max at current KV precision, resident state, pool stats summary.

## 9. Observability

- AS21. Prometheus metrics: request counts/latency by endpoint and outcome; TTFT and ITL histograms split cold/warm; prefill and decode tokens/s; batch occupancy; queue depth; KV metrics (KV23); memory contract vs actual; energy counters when sampling is enabled.
- AS22. `tracing` structured logs with request ids; `--otel` OTLP export optional.

## 10. Acceptance criteria

- AC1. Dialect conformance: recorded-trace suites for openai-python, anthropic SDK, Claude Code, and OpenCode pass unmodified against localhost.
- AC2. ITL guard: with one 32k-token prompt admitted mid-stream, a concurrent decode stream's p95 ITL inflates ≤ 25% (reference fleet, RFC-0009 workload C).
- AC3. Guaranteed JSON: 0 schema violations across the structured-output corpus at concurrency 8.
- AC4. Disconnect storm: 100 mid-stream disconnects leak zero blocks (pool accounting equality after quiesce).
- AC5. M4 product metric: 4 concurrent agent sessions on 30B-A3B (M4 Max) hold per-stream ITL < 45 ms with warm-prefix TTFT < 500 ms.

## Open questions

1. `/v1/responses` (OpenAI Responses API): growing agent adoption vs surface cost; candidate for v0.3 behind a flag (PRD OQ3).
2. Anthropic prompt-caching control headers: map onto KV donate/retain hints, or keep caching fully automatic? (Leaning automatic + honor explicit `cache_control` as hints.)
3. Multi-model routing on one port (model field selects engine) lands v0.3 with the pool; confirm no v1 API shape blocks it.

## References

- OpenAI API reference (chat completions, streaming, structured outputs); Anthropic Messages API reference (2026)
- vllm-mlx and mlx-serve dual-dialect precedents; LM Studio compatibility-endpoint coverage notes (2026)
- Agrawal et al., "Sarathi-Serve: chunked prefill" (2024); vLLM continuous batching literature
- WWDC26 session 232 (agent fan-out against a local server as the reference workload)
