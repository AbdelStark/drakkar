//! `drakkar-core` — the bottom of the DRAKKAR dependency graph (layer 0).
//!
//! This crate owns the shared vocabulary types every other crate imports (DM1):
//! the identifier and hash newtypes ([`Sha256`], [`RequestId`], [`SeqId`],
//! [`BlockId`], [`TokenId`], [`SchemaTag`], [`PrefixHash`], [`PrefixHashChain`]),
//! the dialect-free [`GenerationRequest`] and its aggregates, the
//! artifact/handle pair ([`ModelArtifact`]/[`ModelHandle`]), the memory-contract
//! types ([`MemoryBudget`]/[`MemoryReport`]), the runtime [`Capabilities`]
//! struct, and the [`FitReport`] mirror of `drakkar.fit/1`.
//!
//! It performs no I/O and pulls in no async runtime; everything in the workspace
//! depends on it and it depends on no other workspace crate (DEP2). All types
//! here are backend-neutral and name no Metal, MLX, or llama.cpp type
//! (INV-SEAM).
//!
//! The error taxonomy (`DkError`) lands in issue #123; the KV pool block types
//! and the engine-actor message value types land with their subsystems.
//!
//! # The `session` feature
//!
//! `RenderTarget` and the `GenerationRequest::render` accessor are gated
//! behind the `session` feature. The session/render layer (`drakkar-server`,
//! `drakkar-cli`) enables it; the scheduler and engine do not, so the
//! dialect-carrying `render` value is unreadable below the session layer
//! (DM11/INV-DIALECT).
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod artifact;
pub mod capabilities;
pub mod config;
pub mod error;
pub mod exec;
pub mod fit;
pub mod ids;
pub mod memory;
mod request;
pub mod schema;
pub mod secret;

pub use artifact::{
    ArchDescriptor, ArtifactFormat, BlobRef, LayoutClass, ModelArtifact, ModelHandle, MoeTopology,
    QuantDesc, ToolDialect,
};
pub use capabilities::{Capabilities, ChipId, PagedPath, SpecDecodeSupport};
pub use config::{
    API_KEY_ENV, Config, ImportHfCache, KvCacheConfig, ModelsConfig, RuntimeConfig,
    SchedulerConfig, ServerConfig, Source as ConfigSource, StorageConfig, Telemetry, effective,
    resolve_api_key,
};
pub use error::{
    ALL_ERROR_CODES, ContextValue, DkError, ERROR_SCHEMA, ErrorCategory, ErrorCode, ErrorContext,
    ErrorSurface, Remedy, RemedyTemplate, Retry,
};
pub use exec::{
    BlockTableRef, DecodeBatch, DecodeEntry, DraftTokens, FinishReason, MaskRef, PrefillChunk,
    SamplerStateRef, TokenOut, TopLogprob,
};
pub use fit::{
    BudgetSource, Confidence, Estimate, FIT_SCHEMA, FitContext, FitMachine, FitMemory, FitModel,
    FitPerformance, FitReport, Remedy as FitRemedy, RemedyKind as FitRemedyKind, TtftEstimate,
    Verdict,
};
pub use ids::{
    BlockId, ParseSha256Error, ParseUlidError, ParsedSchemaTag, PrefixHash, PrefixHashChain,
    RequestId, SchemaTag, SeqId, Sha256, TokenId, Ulid, render_schema_tag,
};
pub use memory::{BudgetMismatch, MemoryBreakdown, MemoryBudget, MemoryReport};
pub use request::{
    CacheHint, CachePolicy, CompiledGrammar, GenerationRequest, ModelSelector, Priority,
    RequestLimits, SamplerParams, TokenizedPrompt, ToolContext,
};
pub use schema::{Surface, VersionedRead, read_versioned, write_versioned};
pub use secret::Secret;

/// The response-rendering dialect. Exported only with the `session` feature so
/// that the scheduler and engine cannot name it (DM11/INV-DIALECT).
#[cfg(feature = "session")]
pub use request::RenderTarget;
