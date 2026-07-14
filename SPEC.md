# DRAKKAR Specification

- Status: Accepted (v1 corpus)
- Owner: abdelstark
- Date: 2026-07-14
- Product requirements: [PRD.md](PRD.md)

DRAKKAR is a native, single-binary LLM inference engine for Apple Silicon. It takes a
Hugging Face model reference, computes — before downloading a byte — whether the model fits
the machine, at what quantization, with how much context, and how fast it will run; then it
downloads, prepares, and serves the model at the hardware limit through OpenAI- and
Anthropic-compatible endpoints, with a KV cache subsystem built for multi-request,
long-context agentic workloads. The product principle is honest speed: maximum performance
the hardware allows, and no number shown to a user that the engine cannot defend.

This document is the entry point to the specification corpus. The corpus — this file,
`docs/spec/`, and `docs/rfcs/` — is the single source of truth for implementation. Every
implementation issue traces to a section of it, and every load-bearing technical decision is
locked by an RFC.

## Corpus index

### Specification (`docs/spec/`)

| Doc | Scope |
| --- | ----- |
| [00-overview](docs/spec/00-overview.md) | Thesis, goals, non-goals, success criteria, roadmap |
| [01-architecture](docs/spec/01-architecture.md) | System decomposition, crate map, threading model, invariants |
| [02-public-api](docs/spec/02-public-api.md) | Public surfaces (CLI, HTTP, JSON schemas, C ABI), versioning policy |
| [03-data-model](docs/spec/03-data-model.md) | Core types, on-disk schemas, schema versioning, invariants |
| [04-error-model](docs/spec/04-error-model.md) | Error taxonomy registry, exit codes, HTTP mappings, recovery |
| [05-observability](docs/spec/05-observability.md) | Logging, metrics catalog, tracing, redaction rules |
| [06-security](docs/spec/06-security.md) | Threat model, trust boundaries, secrets handling |
| [07-testing-strategy](docs/spec/07-testing-strategy.md) | Test pyramid, conformance suites, CI gates |
| [08-performance-budget](docs/spec/08-performance-budget.md) | Latency/throughput/memory budgets, profiling plan |
| [09-release-and-versioning](docs/spec/09-release-and-versioning.md) | SemVer policy, deprecation, changelog discipline |
| [10-glossary](docs/spec/10-glossary.md) | Canonical terms used across the corpus |
| [11-decision-log](docs/spec/11-decision-log.md) | Locked decisions (`LDn`) and the open questions they resolved |

### RFCs (`docs/rfcs/`)

| RFC | Title | Decides |
| --- | ----- | ------- |
| [RFC-0001](docs/rfcs/RFC-0001-architecture.md) | Architecture Overview and Design Principles | Decomposition, engine actor, backend seam, invariants |
| [RFC-0002](docs/rfcs/RFC-0002-stack-selection.md) | Technology Stack Selection | Rust control plane over vendored MLX core; llama.cpp secondary |
| [RFC-0003](docs/rfcs/RFC-0003-inference-core.md) | Inference Core | Execution model, quantization matrix, sampling, speculation |
| [RFC-0004](docs/rfcs/RFC-0004-feasibility-engine.md) | Feasibility Engine | Memory math, GPU budget model, verdicts, performance prediction |
| [RFC-0005](docs/rfcs/RFC-0005-kv-cache.md) | KV Cache Subsystem | Paged blocks, prefix sharing, KV quantization, SSD tier |
| [RFC-0006](docs/rfcs/RFC-0006-model-pipeline.md) | Model Acquisition and Format Pipeline | Reference resolution, downloads, store, conversion |
| [RFC-0007](docs/rfcs/RFC-0007-api-server.md) | API Server and Scheduler | Dual-dialect endpoints, continuous batching, ITL guard |
| [RFC-0008](docs/rfcs/RFC-0008-cli-ux.md) | CLI and UX Specification | Command surface, JSON contract, exit codes, REPL |
| [RFC-0009](docs/rfcs/RFC-0009-performance.md) | Performance Targets and Benchmark Methodology | Metrics, harness, Tier-1 targets, CI gate, calibration |
| [RFC-0010](docs/rfcs/RFC-0010-backend-abi.md) | Backend FFI and C ABI | The dk_* ABI between Rust and the MLX shim; embedder surface |
| [RFC-0011](docs/rfcs/RFC-0011-error-taxonomy.md) | Error Taxonomy and Failure Semantics | One error type, stable code registry, exit/HTTP mappings |
| [RFC-0012](docs/rfcs/RFC-0012-release-engineering.md) | Release Engineering and Distribution | Build, signing, notarization, Homebrew tap, CI pipeline |

## How to read the corpus

Read [PRD.md](PRD.md) for the why, then
[00-overview](docs/spec/00-overview.md), [01-architecture](docs/spec/01-architecture.md),
and RFC-0001/RFC-0002 for the shape and the stack decision. The remaining RFCs are
independently readable once those are absorbed. The `docs/spec/` sections state the
normative contracts; the RFCs are the decision records that justify them. Where a number or
contract appears in both, the location named as canonical in the text wins; by default the
spec section is canonical for contracts and the RFC for rationale.

## Conventions

- Requirement IDs are per document: `A*` (RFC-0001), `S*/D*/R*` (RFC-0002), `IC*`
  (RFC-0003), `FE*` (RFC-0004), `KV*` (RFC-0005), `MP*` (RFC-0006), `AS*` (RFC-0007),
  `CLI*` (RFC-0008), `PB*` (RFC-0009), `AB*` (RFC-0010), `ER*` (RFC-0011), `RE*`
  (RFC-0012). Spec sections mint `API-`, `DM-`, `OBS-`, `SEC-`, `TS-`, `PBU-`, `RV-`
  prefixed IDs only for contracts not already covered by an RFC ID.
- RFC 2119 keywords (MUST, SHOULD, MAY) carry their standard meaning.
- `LDn` cites a locked decision in the
  [decision log](docs/spec/11-decision-log.md).
- Numbers marked `est.` are modeled estimates pending measurement on the RFC-0009 fleet.
- Memory figures are GiB unless noted; dates are ISO 8601.

## Change process

- Contracts change by PR against the affected spec section, reviewed against the
  invariants in [01-architecture](docs/spec/01-architecture.md).
- New load-bearing decisions — new subsystems, algorithms whose correctness is not obvious
  from the type signature, cross-cutting concerns, external boundaries, or any choice a
  reasonable engineer would make differently — require a new RFC using the template
  established by RFC-0010 through RFC-0012.
- An accepted RFC is superseded, never edited into a different decision; supersession is
  recorded in both RFC headers.
- Open questions live in RFC `Open Questions` sections with an owner and a resolution path;
  the implementation tracker ([docs/roadmap/IMPLEMENTATION.md](docs/roadmap/IMPLEMENTATION.md))
  carries the corresponding work items.
