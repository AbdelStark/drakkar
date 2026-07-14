//! The backend seam: the `InferenceBackend` trait and its output value types
//! (RFC-0001 §5/§6, invariants I1–I5).
//!
//! This trait is the only portability boundary in DRAKKAR (I5): nothing above
//! it may name Metal, MLX, or llama.cpp types. Both `drakkar-mlx` and (later)
//! `drakkar-gguf` implement it. It is deliberately step-granular (A6) — one
//! prefill chunk, one decode step across `B` sequences — so scheduling policy
//! stays in Rust and only math crosses the seam. Every type in the signature is
//! a `drakkar-core`/`drakkar-engine` type; the opaque handles ([`LogitsRef`],
//! `ModelHandle`, `BlockTableRef`, `SamplerStateRef`, `MaskRef`) are the only
//! backend-owned state above the seam.

use drakkar_core::{
    Capabilities, DecodeBatch, DkError, MemoryBudget, MemoryReport, ModelArtifact, ModelHandle,
    PrefillChunk, SamplerParams, TokenOut,
};

use crate::kv::KvPool;

/// The seam result type: every backend operation yields a `drakkar-core`
/// [`DkError`] on failure (no raw Metal/FFI error crosses the seam, RFC-0011).
pub type BackendResult<T> = Result<T, DkError>;

/// An opaque handle to on-device logits produced by prefill/decode, consumed by
/// [`InferenceBackend::sample`]. Backend-neutral: only the owning backend
/// interprets it (INV-SEAM); it names no Metal/MLX type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LogitsRef(pub u64);

/// The result of a prefill chunk: the logits for the final position (the seed
/// for decode) and how many tokens were processed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrefillOut {
    /// On-device logits for the last processed position.
    pub logits: LogitsRef,
    /// Tokens processed in this chunk.
    pub tokens_processed: u32,
}

/// The result of one decode step across the batch: the logits for each sequence
/// this step, referenced opaquely and sampled via
/// [`InferenceBackend::sample`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecodeOut {
    /// On-device logits for the sequences in this decode step.
    pub logits: LogitsRef,
    /// Number of sequences in the step (the batch size `B`).
    pub batch: u32,
}

/// The compute seam between the Rust control plane and a native backend
/// (RFC-0001 §5). Step-granular by design (A6): the backend does math, never
/// scheduling. `Capabilities` gates features at runtime — no caller may assume a
/// capability the backend did not report (A7).
pub trait InferenceBackend {
    /// Load a model under a declared memory budget, returning the thread-confined
    /// handle (I1/I2). The engine never exceeds `budget`.
    fn load(
        &mut self,
        artifact: &ModelArtifact,
        budget: MemoryBudget,
    ) -> BackendResult<ModelHandle>;

    /// Run one prefill chunk (`<= chunk_size` tokens) for a sequence (IC12).
    fn prefill(&mut self, handle: &ModelHandle, batch: PrefillChunk) -> BackendResult<PrefillOut>;

    /// Run one decode step across `B` sequences (RFC-0001 §5).
    fn decode(&mut self, handle: &ModelHandle, batch: DecodeBatch) -> BackendResult<DecodeOut>;

    /// The KV pool this backend owns (RFC-0005 KV22); the `kv()` accessor keeps
    /// the pool behind the seam (A6).
    fn kv(&mut self) -> &mut dyn KvPool;

    /// Sample a token from `logits` under `params` (fused on-GPU sampling, IC14).
    fn sample(&mut self, logits: LogitsRef, params: &SamplerParams) -> BackendResult<TokenOut>;

    /// The measured resident footprint versus the declared contract (IC25).
    fn memory_report(&self) -> MemoryReport;

    /// The runtime capability set (IC26); the only sanctioned way a feature
    /// learns whether it may run (A7, DM16).
    fn capabilities(&self) -> Capabilities;
}
