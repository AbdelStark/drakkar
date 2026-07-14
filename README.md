# DRAKKAR: a native inference engine for Apple Silicon

**Working codename:** DRAKKAR (provisional, trademark screening pending). A drakkar is a Norman longship: light, fast, shallow draft, built to move quickly close to home. The metaphor is deliberate: local-first, speed-obsessed, no dependence on distant infrastructure.

**One-line pitch:** a single native binary that takes a Hugging Face model reference, tells you honestly whether and how well it will run on your MacBook Pro, then runs it at the hardware limit through an OpenAI- and Anthropic-compatible API.

**Owner:** Abdelhamid Bakhta
**Status:** Specification corpus accepted; implementation tracked in [docs/roadmap/IMPLEMENTATION.md](docs/roadmap/IMPLEMENTATION.md)
**License:** [Apache-2.0](LICENSE)

## Document map

| Doc | Scope |
| --- | ----- |
| [PRD.md](PRD.md) | Vision, users, competitive landscape, requirements, metrics, roadmap, risks |
| [SPEC.md](SPEC.md) | Entry point to the specification corpus; index, conventions, change process |
| [docs/spec/](docs/spec/) | Normative contracts: overview, architecture, public API, data model, errors, observability, security, testing, performance budget, release policy, glossary |
| [docs/rfcs/](docs/rfcs/) | Decision records RFC-0001 through RFC-0012 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Spec-first contributor workflow |
| [SECURITY.md](SECURITY.md) | Vulnerability reporting policy |

## How to read this set

Read [PRD.md](PRD.md) first for the why, then [SPEC.md](SPEC.md) for the corpus index.
RFC-0001 (architecture) and RFC-0002 (stack selection) fix the shape and the substrate;
the remaining RFCs are independently reviewable once those two are absorbed. Each RFC
carries a status header, RFC 2119 requirement language (MUST, SHOULD, MAY), open questions
with owners, and references.

## Conventions

Requirement IDs are prefixed per document (for example `FE-3` in the Feasibility Engine
RFC). Numbers marked `est.` are modeled estimates pending measurement on the benchmark
fleet defined in RFC-0009. All memory figures use GiB unless noted. All dates ISO 8601.

## Ground truth snapshot (July 2026)

The design assumes the hardware and software landscape as of mid-2026: M5-generation
MacBook Pro (M5, M5 Pro, M5 Max with GPU-core Neural Accelerators, 153 to 614 GB/s unified
memory bandwidth), macOS 26 (Tahoe) with Metal 4 TensorOps, MLX v0.31.x with continuous
batching in mlx-lm, and an ecosystem that has largely consolidated on MLX as the compute
substrate (Ollama 0.19 MLX backend, vllm-metal, LM Studio MLX engine). Sources are cited
per RFC.
