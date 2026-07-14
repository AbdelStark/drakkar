# 06 — Security Model

This document is the canonical security specification for DRAKKAR: the assets the system
protects, the trust boundaries it enforces, the enumerated threats with their concrete
mitigations, and the handling rules for secrets. It consolidates and extends RFC-0001 A9-A12
([Architecture](../rfcs/RFC-0001-architecture.md#proposed-design)), RFC-0006 MP2/MP6/MP8
([Model Pipeline](../rfcs/RFC-0006-model-pipeline.md#proposed-design)), RFC-0007 AS18-AS19
([API Server](../rfcs/RFC-0007-api-server.md#proposed-design)), and RFC-0005 KV19
([KV Cache](../rfcs/RFC-0005-kv-cache.md#proposed-design)). Requirements minted here carry
`SEC-n` IDs; where a requirement restates an RFC requirement, the RFC ID is cited and the
RFC remains authoritative for its subsystem.

Posture in one sentence: DRAKKAR is a single-user, local-first program that treats every
byte it did not compute itself — model artifacts, HTTP requests, prompts, hub metadata — as
untrusted input, and treats the user's prompts, completions, and credentials as sensitive
data that never leave the machine ([PRD](../../PRD.md#5-product-requirements) P13).

## 1. Assets

Ranked by the damage their compromise causes:

| # | Asset | Where it lives | Compromise consequence |
|---|-------|----------------|------------------------|
| 1 | The machine itself (code execution, kernel state) | Process memory, `iogpu.wired_limit_mb` sysctl | Arbitrary code execution or a destabilized OS |
| 2 | User prompts and completions | Process memory; SSE streams; optionally request logs | Disclosure of the user's most sensitive working data (PRD persona 3: lawyer, clinician, security researcher) |
| 3 | KV cache contents, including the SSD tier | Engine KV pool (RAM); `~/.drakkar/kv-cache/` safetensors blocks | KV blocks are a lossy but substantially recoverable encoding of prompt content; disk tier persists across reboots (RFC-0005 KV17/KV19) |
| 4 | Hugging Face tokens | Env, `~/.huggingface/token`, macOS keychain | Access to the user's gated/private repos and HF account scope |
| 5 | Server API keys | `~/.config/drakkar/config.toml` (`server.api_key`), `DRAKKAR_*` env, `--api-key` flag | LAN access to the inference server and everything in asset 2 |
| 6 | Model store integrity | `~/.drakkar/models/` content-addressed blobs | Silent substitution of model weights changes model behavior undetectably |
| 7 | Configuration | `~/.config/drakkar/config.toml` | Redirection of storage, binding, or cache behavior |

- SEC1. KV cache blocks — RAM and disk tier alike — MUST be treated with the same
  sensitivity class as raw prompt text. They are derived from prompts and MUST be excluded
  from any diagnostics bundle by default (RFC-0005 KV19).
- SEC2. Everything under `~/.drakkar/` is reconstructible state (RFC-0001 A8); its
  confidentiality matters (SEC1), its availability does not. Deleting it is always safe.

## 2. Trust boundaries

```
                            UNTRUSTED                          TRUSTED (user's session)
             ┌───────────────────────────────┐   ┌──────────────────────────────────────┐
             │                               │   │  drakkar process (single binary)     │
  HTTP       │  loopback client ────────────────▶│  axum handlers                       │
  clients    │  LAN client (opt-in) ──[B1]──────▶│   │ dialect normalize, auth,         │
             │                               │   │   │ grammar compile (bounded)        │
             │                               │   │   ▼                                  │
  HF hub     │  repo metadata, safetensors,  │   │  model manager ──[B2 parse]──▶ store │
  (TLS)      │  GGUF, tokenizer, template ──────▶│   │ defensive parsers, sandboxed     │
             │                               │   │   │ template engine                  │
             │                               │   │   ▼                                  │
             │                               │   │  Rust control plane ──[B3 FFI]──▶    │
             │                               │   │        drakkar-mlx C++ shim / MLX    │
             │                               │   │        drakkar-gguf / llama.cpp      │
             │                               │   ├──────────────[B4 fs]─────────────────┤
             │  other local users /          │   │  ~/.drakkar/kv-cache (0600)          │
             │  world-readable paths ◀──────────▶│  ~/.config/drakkar/config.toml (0600)│
             └───────────────────────────────┘   └──────────────────────────────────────┘
```

Four boundaries: **B1** the network edge, **B2** the model-artifact boundary, **B3** the
FFI boundary into C++, **B4** the local filesystem. Each is specified below.

### 2.1 B1: Network edge

- SEC3. The server MUST bind `127.0.0.1:11711` by default (RFC-0007 AS18). Non-loopback
  binding (`--host 0.0.0.0`, `server.host` config, or `DRAKKAR_HOST`) is opt-in and MUST
  be refused at startup — with a named error, before any socket is opened — unless an API
  key is configured. There is no unauthenticated non-loopback mode.
- SEC4. API key verification MUST use a constant-time byte comparison (RFC-0007 AS18);
  key material MUST never appear in logs, error bodies, or `--json` output. The server
  accepts the key as `Authorization: Bearer <key>` (OpenAI dialect) or `x-api-key: <key>`
  (Anthropic dialect); both headers are redacted from request logging unconditionally.
- SEC5. CORS is off by default. Enabling it requires an explicit origin list
  (`server.cors_origins`); wildcard `*` is rejected by config validation when an API key
  is not set (RFC-0007 AS18).
- SEC6. To defeat DNS-rebinding drive-by attacks against the loopback server, the server
  MUST validate the `Host` header: when bound to loopback, only `localhost`,
  `127.0.0.1`, `[::1]` (with optional port) are accepted; when bound wider, the configured
  hostnames. Mismatches return `403` with a named error code. This is the standard
  hardening for local inference daemons and costs one string compare per request.
- SEC7. TLS termination is out of scope for the binary (RFC-0007 AS18). The documented
  deployment pattern for anything beyond a trusted LAN is a reverse proxy (any
  TLS-terminating proxy the user already runs) forwarding to loopback, with the API key
  still enabled behind it. The docs site ships this pattern; DRAKKAR itself never handles
  certificates.
- SEC8. Request logging defaults to metadata only — timestamps, request id, model, token
  counts, latency, outcome — never prompt or completion bodies. `--log-bodies` is explicit,
  marked sensitive in help text, and prints a warning at startup while active
  (RFC-0007 AS19).
- SEC9. Per-client-IP rate limiting is available but off by default (localhost reality,
  RFC-0007 AS19). Resource exhaustion by request volume is otherwise bounded by admission
  control (RFC-0004 FE18): no admitted request can exceed the memory contract, and
  `max_concurrency` (default 8) caps live sequences.
- SEC10. The binary makes exactly two classes of outbound network connections: the HF hub
  (model resolution/download, always over HTTPS) and the explicit, on-demand
  `drakkar doctor --check-update` (RFC-0001 A10, RFC-0008 CLI16). No telemetry, no
  phone-home, no other endpoints. Any future opt-in metrics are governed by
  [PRD](../../PRD.md#5-product-requirements) P13.

### 2.2 B2: Model-artifact boundary

The rule of RFC-0001 A11: **artifacts are data, never code.**

- SEC11. Accepted weight formats are safetensors and GGUF, exclusively. Pickle-bearing
  checkpoints (`.pth`, `.bin` with pickle payloads) are rejected with the documented error
  and a pointer to conversion guidance (RFC-0006 MP6). There is no `trust_remote_code`
  concept anywhere in DRAKKAR: model architectures are implemented natively in the shim,
  parameterized only by `config.json` values (RFC-0002 D3). A repo whose architecture is
  unsupported fails with a named error (`unsupported_architecture`, RFC-0011 taxonomy),
  never by executing repo-supplied code.
- SEC12. Defensive parsing discipline (RFC-0006 MP8), applied identically to safetensors
  headers, the safetensors index JSON, GGUF metadata, `config.json`, and tokenizer files:
  - Header/metadata size fields are bounded before read: safetensors header length is
    capped at 100 MB (matching the reference implementation's limit); GGUF metadata
    key/value counts and string lengths are bounded and validated against the actual file
    size.
  - No allocation is ever sized from an untrusted length field alone: every declared
    tensor extent, offset, and count is checked against the real on-disk byte range
    before any buffer is allocated or mapped ("tensor bomb" defense).
  - Offsets must be non-overlapping, in-bounds, and monotone where the format requires it;
    violations abort the parse with a named error, never a partial load.
  - Parsers run in Rust (safetensors/GGUF readers in `drakkar-models`) before any data
    crosses B3; the C++ shim receives only validated, sized buffers.
- SEC13. Repo-supplied names never become filesystem paths. Blobs are stored content-
  addressed as `sha256-*` (RFC-0006 MP10); manifest paths are built from
  `org`/`repo`/`revision` components that MUST be validated to contain no path separators,
  no `..`, no NUL, and no control characters before touching the filesystem. Tensor names
  and shard filenames from the index are treated as opaque map keys, never paths.
- SEC14. Chat templates are untrusted repo content executed as templates. They run in a
  sandboxed minijinja environment exposing only the standard HF template API surface
  (RFC-0006 MP18): no filesystem, no network, no process access, with bounded recursion
  depth and render-time output size. A template that exceeds its bounds fails the request
  with a named error, not the process.
- SEC15. Download integrity: per-file sizes and hub-provided ETag/sha256 are verified
  before an artifact enters the store (RFC-0006 MP8); files imported from the HF cache via
  clonefile/hard-link are verified the same way and the HF cache is never mutated
  (RFC-0006 MP11). A verification failure quarantines the file (temp name, never renamed
  into the store) and reports exit code 5 semantics per RFC-0008 CLI8.

### 2.3 B3: FFI boundary

- SEC16. All C++ in the product sits behind the `dk_*` C ABI of the `drakkar-mlx` shim
  (~40 functions, RFC-0002 D2) and the embedded llama.cpp behind the same
  `InferenceBackend` trait. Untrusted bytes MUST be fully validated in Rust (SEC12) before
  crossing this boundary; the shim's contract is that every pointer/length pair it
  receives is already correct, and it revalidates cheap invariants (non-null, alignment,
  length caps) as defense in depth.
- SEC17. The shim is built and tested under AddressSanitizer and UndefinedBehaviorSanitizer
  in CI, and every ABI entry point that consumes variable-length input is fuzzed
  continuously, per the AB requirements of
  [RFC-0010](../rfcs/RFC-0010-backend-abi.md#testing-strategy). Fuzz corpora include
  minimized crashers from SEC12's parsers so both sides of the boundary see the same
  hostile inputs.
- SEC18. A panic or fault in the backend is contained to the engine actor thread and
  surfaces as exit-6 / `503 engine_failure` semantics (RFC-0008 CLI15, RFC-0011), never as
  undefined behavior propagating into the control plane. Full crash isolation via an
  engine subprocess is deferred to v1.0 evaluation (RFC-0001, kept open, owner
  abdelstark).

### 2.4 B4: Local filesystem

- SEC19. SSD KV tier files (`~/.drakkar/kv-cache/`) are created mode `0600` with the
  parent directory `0700` (RFC-0005 KV19). Writes are temp-file + rename, so a crash never
  leaves a partially written block readable under its final name.
- SEC20. `~/.config/drakkar/config.toml` is written mode `0600` (it may contain
  `server.api_key`). `drakkar doctor` MUST warn when the config file, the kv-cache
  directory, or `~/.huggingface/token` is group- or world-readable, and name the exact
  `chmod` remedy (RFC-0008 CLI15 error shape).
- SEC21. Logs (`~/.drakkar/logs/`) inherit SEC8's metadata-only rule. Nothing written under
  `~/.drakkar/` or the config directory ever contains an HF token, API key, or — absent
  `--log-bodies` — prompt/completion text.
- SEC22. Protection against other *users* on the machine is delivered by these POSIX
  permissions and nothing more. A same-UID process (any program the user runs) can read
  everything the user can; defending against that is a non-goal (§6).

### 2.5 Machine safety (wired-limit guidance)

- SEC23. DRAKKAR MUST NOT modify `iogpu.wired_limit_mb` or any other sysctl itself. When a
  fit plan requires a raised limit, the report prints the exact command for the user to
  run, the computed safe value respecting the `os_floor` reserve, the one-command revert,
  and the statement that Apple does not support the setting (RFC-0004 FE17,
  [PRD](../../PRD.md#9-risks-and-mitigations) risk table). This is a safety boundary, not
  merely UX: an automated wrong value can hang the user's machine.

## 3. Threat enumeration

Every row names the concrete mitigation and its normative source. "Local attacker" means
an unprivileged process or LAN peer; root and physical access are out of scope (§6).

| Threat | Vector | Mitigation | Source |
|--------|--------|------------|--------|
| Malicious model repo: code execution | Pickle checkpoint, `trust_remote_code`-style hooks, executable "model" files | safetensors/GGUF only; pickle rejected with named error; architectures implemented natively, config-driven; no code loading path exists | RFC-0001 A11, RFC-0006 MP6, RFC-0002 D3, SEC11 |
| Malicious model repo: tensor bomb / OOM | Header declares multi-TiB tensor extents to force allocation | No allocation from untrusted lengths; extents validated against real file size before any buffer exists | RFC-0006 MP8, SEC12 |
| Malicious model repo: oversized header | Multi-GiB safetensors header / unbounded GGUF metadata to stall or exhaust the parser | Bounded header size (100 MB cap); bounded metadata counts and string lengths; parse aborts with named error | RFC-0006 MP8, SEC12 |
| Malicious model repo: path traversal | `../`-bearing shard names, org/repo components, tensor names | Content-addressed blob store; path components validated (no separators, `..`, NUL, control chars); tensor names never become paths | RFC-0006 MP10, SEC13 |
| Malicious model repo: hostile chat template | Jinja template with unbounded recursion or environment escape attempts | Sandboxed minijinja, HF-standard API surface only, bounded recursion and output; per-request failure, not process failure | RFC-0006 MP18, SEC14 |
| Malicious prompt: grammar bomb | Pathological JSON schema / regex in `response_format` forcing exponential grammar compilation | Grammar compilation is bounded: schema/grammar input capped at 256 KiB, compilation budget 100 ms wall-clock on the blocking pool (never the engine thread); exceeding either returns `400 grammar_too_complex` naming the limit | RFC-0003 IC16, RFC-0007 AS10, SEC24 |
| Malicious prompt: resource exhaustion | Huge prompts / max_tokens to starve other streams or OOM | Admission control against the live memory contract (`413`/`429` with remediation fields); chunked-prefill ITL guard; `max_concurrency` cap | RFC-0004 FE18, RFC-0007 AS8/AS13/AS14, RFC-0001 I2 |
| Local network attacker (opt-in LAN bind) | Credential-less access, key brute force, timing oracle | No keyless non-loopback mode; constant-time compare; optional per-IP rate limiting; reverse-proxy TLS pattern for hostile networks | RFC-0007 AS18/AS19, SEC3/SEC4/SEC7 |
| Drive-by browser request / DNS rebinding | Web page scripting requests at `127.0.0.1:11711` | CORS off by default with explicit-origin allowlist; Host-header validation on loopback | RFC-0007 AS18, SEC5/SEC6 |
| Disconnect/cancel storm | Mass mid-stream disconnects to leak KV blocks | Cancellation frees/donates blocks within one decode step; pool-accounting equality verified in the disconnect-storm acceptance test | RFC-0007 AS4 and its testing strategy |
| KV disk-tier disclosure | Another user or a backup pipeline reads persisted KV blocks | Files 0600 in a 0700 directory; treated as prompt-equivalent; excluded from diagnostics bundles; `drakkar cache clear` for disposal; disk tier off by default for one-shot `run` | RFC-0005 KV17/KV19, SEC1/SEC19 |
| Secret leakage via logs/errors | Tokens or keys echoed in logs, panics, `--json` output | A12 redaction rule; SEC4/SEC21; panics are caught and rendered without environment dumps; stack traces only under `--verbose` and still redacted | RFC-0001 A12, RFC-0008 CLI15 |
| Dependency supply chain (Rust) | Compromised or vulnerable crate versions | `Cargo.lock` committed and pinned; `cargo audit` (RustSec advisories) and `cargo deny` (licenses, duplicate/major-version drift, source allowlist) as required CI gates; MSRV pinned | RFC-0002 D5/D6, RFC-0012, SEC25 |
| Dependency supply chain (native) | Compromised MLX or llama.cpp source | MLX vendored and pinned by git submodule hash per DRAKKAR release with a documented upgrade cadence; llama.cpp pinned identically behind the `drakkar-gguf` feature; upgrades are reviewed diffs, never floating refs | RFC-0002 D2/D4/D5, SEC25 |
| Release artifact tampering | Modified binary between build and user | Codesigned and notarized arm64 binary; checksums published with GitHub releases; Homebrew tap pins the checksum | RFC-0002 D5, RFC-0012 |
| Hub transport tampering | MITM of model downloads | HTTPS to the hub; size + ETag/sha256 verification before store admission; content-addressed storage makes post-admission tampering detectable | RFC-0006 MP8/MP10, SEC15 |

- SEC24. Grammar/structured-output compilation MUST be bounded in input size (256 KiB
  default, `server.grammar_max_bytes`) and wall-clock (100 ms default,
  `server.grammar_compile_budget_ms`), MUST run on the blocking pool, and MUST fail as a
  structured `400` naming the exceeded limit. The token-mask application path itself is
  O(vocab) per step and not attacker-controllable beyond that (RFC-0003 IC16).
- SEC25. Supply-chain gates are release-blocking CI jobs, not advisories: a failing
  `cargo audit` or `cargo deny` check, or an MLX/llama.cpp submodule not matching the
  pinned hash, fails the build. Waivers require a written rationale in the repo
  ([RFC-0012](../rfcs/RFC-0012-release-engineering.md#proposed-design)).

## 4. Secrets handling

- SEC26. **HF token discovery order** (first hit wins, per RFC-0006 MP2):
  1. `HF_TOKEN` environment variable,
  2. the standard HF CLI token file (`~/.huggingface/token`, honoring `HF_HOME`),
  3. the macOS keychain (service `drakkar-hf`, read-only lookup).

  DRAKKAR reads tokens; it never writes, migrates, or caches them in its own state. A
  gated repo without a token produces a named error carrying the repo's acceptance URL
  (RFC-0006 MP2) — the error message never includes any partial token.
- SEC27. Redaction is structural, not best-effort: token and API-key values are wrapped in
  a `Secret<String>` type in `drakkar-core` whose `Debug`/`Display`/`Serialize`
  implementations emit `[redacted]`. Raw secret bytes exist only at the HTTP-header
  construction site and the constant-time compare (RFC-0001 A12).
- SEC28. Server API keys load from (highest precedence first) `--api-key`,
  `DRAKKAR_API_KEY`, then `server.api_key` in config (RFC-0008 CLI10 precedence).
  `drakkar config set server.api_key` writes atomically and enforces SEC20 permissions.
  Keys are static bearer secrets; rotation is by replacement and server restart — there is
  no key-management system in scope (§6).

## 5. Vulnerability reporting

- SEC29. Security reports are received exclusively through GitHub private vulnerability
  reporting on the DRAKKAR repository. A `SECURITY.md` at the repo root states this and
  links to this document; it ships from v0.1 (repo hygiene precedes features).
- SEC30. Process commitments: acknowledge reports within 7 days; coordinate disclosure with
  the reporter with a default 90-day window; fixed vulnerabilities are noted in release
  notes with credit unless the reporter declines. Severity is judged against the assets in
  §1 (machine > prompts/KV > credentials). Public issues filed for suspected
  vulnerabilities are converted to private reports and locked, not triaged in public.

## 6. Explicit non-goals

Stated so nobody builds on a guarantee that does not exist:

- Multi-tenant authorization, per-user quotas, or fairness across organizations — the
  server is single-user with at most one shared API key
  ([PRD](../../PRD.md#4-goals-and-non-goals) N3).
- TLS termination in the binary (SEC7's reverse-proxy pattern is the supported answer).
- Defense against a same-UID local process, root, or an attacker with physical access.
  POSIX permissions (SEC19-SEC21) are the ceiling of local protection.
- Encryption at rest for the model store or KV disk tier. FileVault covers the physical
  loss case; users who cannot tolerate persisted KV keep `kv_cache.disk = false`
  (RFC-0005 KV17) or use the per-request `cache: false` opt-out (RFC-0005, RFC-0007).
- Prompt-injection and model-output safety (jailbreaks, harmful generations). DRAKKAR
  serves the model faithfully; content policy belongs to the model and the calling
  application.
- Side-channel resistance (timing of cache hits, GPU residency inference). The prefix
  cache intentionally makes warm requests faster, which a same-machine observer could in
  principle measure; with N3's single-user scope there is no victim/attacker separation
  for it to matter.
- Sandboxing the DRAKKAR process itself (App Sandbox / seatbelt profiles). Worth
  revisiting for the v1.0 desktop app alongside the crash-isolation decision (RFC-0001,
  kept open, owner abdelstark, target v1.0).

## Cross-references

- Error codes and envelopes for every named error above: [04 — Error Model](04-error-model.md).
- Release signing, notarization, and CI supply-chain gates: [09 — Release Engineering](09-release-and-versioning.md),
  [RFC-0012](../rfcs/RFC-0012-release-engineering.md#proposed-design).
- FFI sanitizer and fuzzing requirements: [RFC-0010](../rfcs/RFC-0010-backend-abi.md#testing-strategy).
- Feasibility/admission arithmetic backing SEC9: [RFC-0004](../rfcs/RFC-0004-feasibility-engine.md#proposed-design).
