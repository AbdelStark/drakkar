//! Engine-actor execution value types (data-model §3.7, RFC-0001 §5/§6).
//!
//! The message vocabulary of the engine actor loop is step-granular by design:
//! scheduling policy stays in Rust and only math crosses the seam (A6). These
//! plain-data structs live in `drakkar-core` (DM1) so the scheduler, the engine
//! actor, and the backends all name one vocabulary. The actor `EngineMsg` loop
//! and the `KvPool` trait that consume them are defined by the engine and KV
//! subsystems.
//!
//! The opaque handle newtypes ([`BlockTableRef`], [`SamplerStateRef`],
//! [`MaskRef`]) are backend-neutral markers: they carry a process-local handle
//! whose contents only the owning backend interprets, and they name no Metal,
//! MLX, or llama.cpp type (DM2/INV-SEAM).

use serde::{Deserialize, Serialize};

use crate::ids::{SeqId, TokenId};

/// An opaque handle to a backend's per-sequence logical→physical KV block map
/// (KV3, IC10). Backend-neutral: the backend reads K/V placement through it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct BlockTableRef(pub u64);

/// An opaque handle to a backend's per-sequence on-device sampler state
/// (penalty/RNG state, IC14). Backend-neutral.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct SamplerStateRef(pub u64);

/// An opaque handle to an uploaded grammar mask (a vocab bitset, IC16).
/// Backend-neutral; present only for constrained requests.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct MaskRef(pub u64);

/// Speculative draft tokens to verify in one decode step (IC18/IC19).
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct DraftTokens {
    /// The proposed tokens, verified against the true distribution this step.
    pub tokens: Vec<TokenId>,
}

/// One prefill chunk for a sequence: at most the chunk budget of tokens
/// (default 512, adaptive 256..=2048, IC12/AS13). The final chunk (`is_last`)
/// promotes the sequence into the decode batch (AS12).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PrefillChunk {
    /// The sequence this chunk belongs to.
    pub seq: SeqId,
    /// The chunk's token ids (length ≤ the chunk budget).
    pub tokens: Vec<TokenId>,
    /// The absolute position of `tokens[0]`; starts after the cached prefix
    /// (KV10).
    pub position_offset: u32,
    /// Opaque block-table handle; the backend reads K/V placement through it.
    pub block_table: BlockTableRef,
    /// Whether this is the final chunk of the prompt.
    pub is_last: bool,
}

/// One decode step across `B` sequences (RFC-0001 §5).
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct DecodeBatch {
    /// One entry per sequence in the batch.
    pub entries: Vec<DecodeEntry>,
}

/// One sequence's state for a single decode step.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct DecodeEntry {
    /// The sequence.
    pub seq: SeqId,
    /// The last sampled token, fed as this step's input.
    pub last_token: TokenId,
    /// The absolute position of `last_token`.
    pub position: u32,
    /// Opaque block-table handle.
    pub block_table: BlockTableRef,
    /// Opaque per-sequence on-device sampler state.
    pub sampler: SamplerStateRef,
    /// Opaque grammar mask, uploaded only for constrained requests (IC16).
    pub grammar_mask: Option<MaskRef>,
    /// Speculative tokens to verify this step (IC18/IC19).
    pub draft: Option<DraftTokens>,
}

/// A top-k logprob entry, computed on-GPU (IC4).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct TopLogprob {
    /// The token.
    pub token: TokenId,
    /// Its log-probability.
    pub logprob: f32,
}

/// Why a sequence finished generating.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// A stop token id was produced.
    Stop,
    /// The `max_tokens` limit was reached.
    Length,
    /// A stop string matched.
    StopString,
    /// The end-of-sequence token was produced.
    Eos,
    /// The request was cancelled (e.g. client disconnect).
    Cancelled,
}

/// The per-step readback for one sequence.
///
/// Per DM20 this carries **only** sampled token ids and optional top-k
/// logprobs — never full logits. Stop-string matching, grammar advance, and
/// tool-call parsing consume `TokenOut` in Rust on the scheduler side (IC17);
/// the backend does not know what a stop string is. There is deliberately no
/// `logits` field on this type.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TokenOut {
    /// The sequence.
    pub seq: SeqId,
    /// The sampled token run: more than one only when speculation accepts a run
    /// (IC18/IC19).
    ///
    /// Data-model §3.7 specifies `SmallVec<[TokenId; 4]>` to avoid a heap
    /// allocation in the common single-token case; that container choice is an
    /// internal representation optimization (not a serialized contract) adopted
    /// by the inference-core perf work. The field name and semantics are
    /// unchanged.
    pub tokens: Vec<TokenId>,
    /// Top-k logprobs, present only when the request asked for them (IC4).
    pub logprobs: Option<Vec<TopLogprob>>,
    /// How many speculative draft tokens were accepted this step; 0 when
    /// speculation is off or rejected.
    pub accepted_draft: u8,
    /// The finish reason, present only on the final step for this sequence.
    pub finish: Option<FinishReason>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_out_round_trips_a_multi_token_speculation_run() {
        let out = TokenOut {
            seq: SeqId(7),
            tokens: vec![TokenId(10), TokenId(20), TokenId(30)],
            logprobs: Some(vec![TopLogprob {
                token: TokenId(10),
                logprob: -0.5,
            }]),
            accepted_draft: 2,
            finish: Some(FinishReason::Eos),
        };
        let json = serde_json::to_value(&out).unwrap();
        let back: TokenOut = serde_json::from_value(json).unwrap();
        assert_eq!(back, out);
        assert_eq!(back.tokens.len(), 3);
        assert_eq!(back.accepted_draft, 2);
        assert_eq!(back.finish, Some(FinishReason::Eos));
    }

    #[test]
    fn decode_entry_holds_opaque_refs_and_optional_draft() {
        let entry = DecodeEntry {
            seq: SeqId(1),
            last_token: TokenId(42),
            position: 100,
            block_table: BlockTableRef(0xdead),
            sampler: SamplerStateRef(0xbeef),
            grammar_mask: Some(MaskRef(1)),
            draft: Some(DraftTokens {
                tokens: vec![TokenId(1), TokenId(2)],
            }),
        };
        let batch = DecodeBatch {
            entries: vec![entry.clone()],
        };
        assert_eq!(batch.entries[0], entry);
    }

    #[test]
    fn finish_reason_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&FinishReason::StopString).unwrap(),
            "\"stop_string\""
        );
    }
}
