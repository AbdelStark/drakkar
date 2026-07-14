# DRAKKAR: a native inference engine for Apple Silicon

**Working codename:** DRAKKAR (provisional, trademark screening pending). A drakkar is a Norman longship: light, fast, shallow draft, built to move quickly close to home. The metaphor is deliberate: local-first, speed-obsessed, no dependence on distant infrastructure.

**One-line pitch:** a single native binary that takes a Hugging Face model reference, tells you honestly whether and how well it will run on your MacBook Pro, then runs it at the hardware limit through an OpenAI- and Anthropic-compatible API.

**Date:** 2026-07-14
**Owner:** Abdelhamid Bakhta
**Status:** Draft v0.1 for review

## Document map

| Doc | Title | Scope |
| --- | ----- | ----- |
| `PRD.md` | Product Requirements Document | Vision, users, competitive landscape, requirements, metrics, roadmap, risks |
| `rfcs/RFC-0001` | Architecture Overview and Design Principles | System decomposition, process model, invariants |
| `rfcs/RFC-0002` | Technology Stack Selection | Research and decision record: Rust + MLX core vs alternatives |
| `rfcs/RFC-0003` | Inference Core | Metal execution, kernels, quantization, sampling, speculative decoding |
| `rfcs/RFC-0004` | Feasibility Engine | Memory math, GPU budget model, context planning, performance prediction |
| `rfcs/RFC-0005` | KV Cache Subsystem | Paged cache, prefix sharing, KV quantization, SSD tier, eviction |
| `rfcs/RFC-0006` | Model Acquisition and Format Pipeline | HF integration, formats, conversion, on-device quantization, storage |
| `rfcs/RFC-0007` | API Server and Scheduler | OpenAI/Anthropic endpoints, continuous batching, streaming, metrics |
| `rfcs/RFC-0008` | CLI and UX Specification | Command surface, one-command run, agent-friendly JSON contract |
| `rfcs/RFC-0009` | Performance Targets and Benchmark Methodology | Metrics, harness, per-chip targets, acceptance gates |

## How to read this set

Read `PRD.md` first for the why, then RFC-0001 and RFC-0002 for the shape and the stack decision. RFCs 0003 through 0009 are independently reviewable once 0001 and 0002 are accepted. Each RFC carries a status header, RFC 2119 requirement language (MUST, SHOULD, MAY), open questions, and references.

## Conventions

Requirement IDs are prefixed per document (for example `FE-3` in the Feasibility Engine RFC). Numbers marked `est.` are modeled estimates pending measurement on the benchmark fleet defined in RFC-0009. All memory figures use GiB unless noted. All dates ISO 8601.

## Ground truth snapshot (July 2026)

The design assumes the hardware and software landscape as of mid-2026: M5-generation MacBook Pro (M5, M5 Pro, M5 Max with GPU-core Neural Accelerators, 153 to 614 GB/s unified memory bandwidth), macOS 26 (Tahoe) with Metal 4 TensorOps, MLX v0.31.x with continuous batching in mlx-lm, and an ecosystem that has largely consolidated on MLX as the compute substrate (Ollama 0.19 MLX backend, vllm-metal, LM Studio MLX engine). Sources are cited per RFC.
