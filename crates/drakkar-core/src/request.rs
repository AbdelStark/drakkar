//! The normalized, dialect-free request (data-model §3.1).
//!
//! Both HTTP dialects, the CLI REPL, and the desktop shim normalize into
//! [`GenerationRequest`] before anything touches the scheduler; everything below
//! the session layer is dialect-blind.
//!
//! The `render` field is the one dialect-carrying value, and it is confined
//! (DM11/INV-DIALECT): it is a `pub(crate)` field, so no downstream crate reads
//! it as a struct field, and it is only readable through the `session`-gated
//! [`GenerationRequest::render`] accessor. [`RenderTarget`] itself is exported at
//! the crate root only under the `session` feature, which the server and CLI
//! enable and the scheduler and engine do not.

use serde::{Deserialize, Serialize};

use crate::ids::{PrefixHashChain, RequestId, TokenId};

/// Which model the request targets (AS3).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ModelSelector {
    /// A model already resident in memory, by name.
    Resident(String),
    /// An installed model to be loaded, by name.
    Installed(String),
    /// The configured default model.
    Default,
}

/// Scheduling priority class (AS15).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Latency-sensitive interactive traffic.
    Interactive,
    /// Throughput-oriented batch traffic.
    Batch,
}

/// The response-rendering dialect. **Dialect-carrying**: read only by the
/// response-rendering layer (DM11/INV-DIALECT), never by the scheduler, fit
/// engine, KV pool, or backends.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderTarget {
    /// OpenAI `/v1/chat/completions` shape.
    OpenAiChat,
    /// OpenAI legacy `/v1/completions` shape.
    OpenAiLegacy,
    /// Anthropic `/v1/messages` shape.
    Anthropic,
    /// CLI rendering.
    Cli,
}

/// An explicit `cache_control` hint, honored as a hint and never required
/// (LD17).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheHint {
    /// An ephemeral cache breakpoint (Anthropic `cache_control`).
    Ephemeral,
}

/// Sampling parameters (data-model §3.1). A temperature of `0.0` short-circuits
/// to greedy sampling (IC14).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SamplerParams {
    /// Sampling temperature; `0.0` means greedy.
    pub temperature: f32,
    /// Top-k truncation.
    pub top_k: Option<u32>,
    /// Nucleus (top-p) truncation.
    pub top_p: Option<f32>,
    /// Minimum-probability truncation.
    pub min_p: Option<f32>,
    /// Presence penalty.
    pub presence_penalty: f32,
    /// Frequency penalty.
    pub frequency_penalty: f32,
    /// Repetition penalty.
    pub repetition_penalty: f32,
    /// Seed for the counter-based RNG (IC14, LD6).
    pub seed: Option<u64>,
    /// Per-token logit bias.
    pub logit_bias: Vec<(TokenId, f32)>,
}

impl Default for SamplerParams {
    fn default() -> Self {
        SamplerParams {
            temperature: 1.0,
            top_k: None,
            top_p: None,
            min_p: None,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            repetition_penalty: 1.0,
            seed: None,
            logit_bias: Vec::new(),
        }
    }
}

/// Per-request limits (data-model §3.1).
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct RequestLimits {
    /// Maximum tokens to generate, already clamped to the model context; a
    /// clamp sets `finish_reason` (AS7).
    pub max_tokens: u32,
    /// Stop strings, matched across token boundaries in Rust (IC17).
    pub stop_strings: Vec<String>,
    /// Stop token ids.
    pub stop_token_ids: Vec<TokenId>,
}

/// The per-request cache policy (data-model §3.1).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CachePolicy {
    /// When `false`, blocks are never donated to the cache, even in RAM (LD8).
    pub donate: bool,
    /// Explicit cache-control hints, honored as hints (LD17).
    pub hints: Vec<CacheHint>,
}

impl Default for CachePolicy {
    fn default() -> Self {
        // Donation on by default; opt out per request (LD8).
        CachePolicy {
            donate: true,
            hints: Vec::new(),
        }
    }
}

/// A tokenized prompt: token ids plus the byte length needed for incremental
/// tool/reasoning parsing (DM12). The raw untokenized message list does not
/// travel past normalization. This minimal shape is finalized alongside the
/// tokenizer and dialect-normalization work (issues #80/#241).
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct TokenizedPrompt {
    /// The prompt token ids.
    pub tokens: Vec<TokenId>,
    /// The byte length of the rendered prompt text.
    pub byte_len: usize,
}

/// A compiled structured-output grammar. Opaque here; compiled by
/// `drakkar-grammar` from `json_schema`/`regex`/`lark` (IC16, AS10). This crate
/// fixes the vocabulary slot; the compiled representation lands in issue #266.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub struct CompiledGrammar {}

/// Declared tools plus the family dialect that drives stream parsing (AS9).
/// Opaque here; populated by the session/render layer (issues #241/#265).
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub struct ToolContext {}

/// The normalized, dialect-free request. Everything below the session layer
/// operates on the dialect-blind fields; only response rendering reads `render`
/// (DM11/INV-DIALECT).
#[derive(Clone, Debug)]
pub struct GenerationRequest {
    /// Request identifier.
    pub id: RequestId,
    /// The targeted model.
    pub model: ModelSelector,
    /// The tokenized prompt.
    pub prompt: TokenizedPrompt,
    /// Prefix hash chain, computed at tokenize time for cache lookup (KV9).
    pub prefix_chain: PrefixHashChain,
    /// Sampling parameters.
    pub sampling: SamplerParams,
    /// Per-request limits.
    pub limits: RequestLimits,
    /// Compiled structured-output grammar, when constrained.
    pub structured: Option<CompiledGrammar>,
    /// Tool context, when tools are declared.
    pub tools: Option<ToolContext>,
    /// Cache policy.
    pub cache: CachePolicy,
    /// Scheduling priority.
    pub priority: Priority,
    /// Whether the response streams.
    pub stream: bool,
    /// Top-k logprobs to read back, if any (IC4).
    pub logprobs: Option<u8>,
    /// Server-level reasoning-hiding override (AS11).
    pub hide_reasoning: bool,
    /// The response-rendering dialect. Confined: `pub(crate)`, read only through
    /// the `session`-gated [`GenerationRequest::render`] accessor (INV-DIALECT).
    /// Without the `session` feature there is no reader, by design.
    #[cfg_attr(not(feature = "session"), allow(dead_code))]
    pub(crate) render: RenderTarget,
}

#[cfg(feature = "session")]
impl GenerationRequest {
    /// Assemble a request in the session/render layer. Available only with the
    /// `session` feature, which the server and CLI enable; the scheduler and
    /// engine cannot construct a request with a `render` dialect (INV-DIALECT).
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        id: RequestId,
        model: ModelSelector,
        prompt: TokenizedPrompt,
        prefix_chain: PrefixHashChain,
        sampling: SamplerParams,
        limits: RequestLimits,
        structured: Option<CompiledGrammar>,
        tools: Option<ToolContext>,
        cache: CachePolicy,
        priority: Priority,
        stream: bool,
        logprobs: Option<u8>,
        hide_reasoning: bool,
        render: RenderTarget,
    ) -> Self {
        GenerationRequest {
            id,
            model,
            prompt,
            prefix_chain,
            sampling,
            limits,
            structured,
            tools,
            cache,
            priority,
            stream,
            logprobs,
            hide_reasoning,
            render,
        }
    }

    /// The response-rendering dialect. Available only with the `session`
    /// feature; the scheduler and engine have no way to read `render`
    /// (INV-DIALECT).
    #[must_use]
    pub fn render(&self) -> RenderTarget {
        self.render
    }
}

#[cfg(all(test, feature = "session"))]
mod tests {
    use super::*;
    use crate::ids::Ulid;

    #[test]
    fn session_layer_can_construct_and_read_render() {
        let req = GenerationRequest::new(
            RequestId(Ulid(1)),
            ModelSelector::Default,
            TokenizedPrompt::default(),
            PrefixHashChain::default(),
            SamplerParams::default(),
            RequestLimits::default(),
            None,
            None,
            CachePolicy::default(),
            Priority::Interactive,
            false,
            None,
            false,
            RenderTarget::Anthropic,
        );
        assert_eq!(req.render(), RenderTarget::Anthropic);
    }
}
