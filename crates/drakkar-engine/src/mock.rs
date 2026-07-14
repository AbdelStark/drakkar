//! A GPU-free [`InferenceBackend`] double for tests (RFC-0001 §5).
//!
//! `MockBackend` implements every seam method with deterministic stub behavior
//! so `drakkar-engine` actor tests — and downstream crates that enable the
//! `test-util` feature — can compile and run without a GPU or the native shim.
//! It names no Metal/MLX type, so it also demonstrates the seam's neutrality.

use drakkar_core::{
    Capabilities, ChipId, DecodeBatch, MemoryBreakdown, MemoryBudget, MemoryReport, ModelArtifact,
    ModelHandle, PagedPath, PrefillChunk, SamplerParams, SeqId, Sha256, SpecDecodeSupport, TokenId,
    TokenOut,
};

use crate::backend::{BackendResult, DecodeOut, InferenceBackend, LogitsRef, PrefillOut};
use crate::kv::{ContiguousKvPool, KvPool};

/// A deterministic, GPU-free backend double.
#[derive(Debug)]
pub struct MockBackend {
    pool: ContiguousKvPool,
    instances: u64,
    last_budget: MemoryBudget,
    /// Monotonic counter making sampled tokens deterministic yet distinct.
    sampled: u32,
}

impl Default for MockBackend {
    fn default() -> Self {
        MockBackend::new()
    }
}

impl MockBackend {
    /// Construct a mock backend with a small contiguous KV pool.
    #[must_use]
    pub fn new() -> Self {
        MockBackend {
            // 1 MiB pool at 128 bytes/token — enough for the actor tests.
            pool: ContiguousKvPool::new(1 << 20, 128),
            instances: 0,
            last_budget: MemoryBudget::new(0, 0, 0, 0, 0, 0),
            sampled: 0,
        }
    }
}

impl InferenceBackend for MockBackend {
    fn load(
        &mut self,
        artifact: &ModelArtifact,
        budget: MemoryBudget,
    ) -> BackendResult<ModelHandle> {
        self.instances += 1;
        self.last_budget = budget;
        Ok(ModelHandle::new(self.instances, artifact.digest, budget))
    }

    fn prefill(&mut self, _handle: &ModelHandle, batch: PrefillChunk) -> BackendResult<PrefillOut> {
        Ok(PrefillOut {
            logits: LogitsRef(0),
            tokens_processed: u32::try_from(batch.tokens.len()).unwrap_or(u32::MAX),
        })
    }

    fn decode(&mut self, _handle: &ModelHandle, batch: DecodeBatch) -> BackendResult<DecodeOut> {
        Ok(DecodeOut {
            logits: LogitsRef(1),
            batch: u32::try_from(batch.entries.len()).unwrap_or(u32::MAX),
        })
    }

    fn kv(&mut self) -> &mut dyn KvPool {
        &mut self.pool
    }

    fn sample(&mut self, _logits: LogitsRef, params: &SamplerParams) -> BackendResult<TokenOut> {
        // Deterministic: greedy at temperature 0, else a counter-driven token.
        let token = if params.temperature == 0.0 {
            TokenId(42)
        } else {
            let t = TokenId(self.sampled);
            self.sampled += 1;
            t
        };
        Ok(TokenOut {
            seq: SeqId(0),
            tokens: vec![token],
            logprobs: None,
            accepted_draft: 0,
            finish: None,
        })
    }

    fn memory_report(&self) -> MemoryReport {
        MemoryReport {
            actual: self.last_budget.declared,
            declared: self.last_budget.declared,
            breakdown: MemoryBreakdown {
                weights: self.last_budget.weights,
                kv: self.last_budget.kv_pool,
                activations: self.last_budget.activation_watermark,
                allocator_cache: 0,
            },
            metal_recommended_working_set: self.last_budget.declared,
            wired_limit_mb: 0,
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            chip: ChipId {
                name: "MockChip".to_owned(),
                gpu_cores: 8,
            },
            bandwidth_gbs: 100.0,
            macos: (26, 2),
            nax_tensor_ops: false,
            kv_bits: vec![16, 8, 4],
            spec_decode: SpecDecodeSupport {
                ngram: false,
                draft: false,
            },
            paged_attention: PagedPath::GatherFallback,
            max_batch: 8,
        }
    }
}

/// Build a minimal, valid [`ModelArtifact`] for tests that need one.
#[must_use]
pub fn stub_artifact() -> ModelArtifact {
    use drakkar_core::{ArchDescriptor, ArtifactFormat, BlobRef, QuantDesc, ToolDialect};
    let blob = |name: &str| BlobRef {
        digest: Sha256([0u8; 32]),
        bytes: 1,
        name: name.to_owned(),
    };
    ModelArtifact {
        digest: Sha256([1u8; 32]),
        manifest_path: std::path::PathBuf::from("/dev/null"),
        format: ArtifactFormat::MlxSafetensors,
        quant: QuantDesc {
            scheme: "mlx_affine".to_owned(),
            bits: 4,
            group: 64,
            bpw_eff: 4.5,
            recipe: None,
        },
        arch: ArchDescriptor {
            name: "mock".to_owned(),
            layers: 1,
            hidden: 8,
            heads: 2,
            kv_heads: 1,
            head_dim: 4,
            vocab: 100,
            params_total: 1000,
            params_active: 1000,
            moe: None,
            layout_classes: vec![drakkar_core::LayoutClass::Global],
        },
        weights: vec![blob("model.safetensors")],
        tokenizer: blob("tokenizer.json"),
        tokenizer_hash: Sha256([0u8; 32]),
        chat_template: blob("chat_template.jinja"),
        chat_template_hash: Sha256([0u8; 32]),
        tool_dialect: ToolDialect::None,
        advertised_ctx: 4096,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_backend_drives_the_seam_end_to_end() {
        let mut backend = MockBackend::new();
        let artifact = stub_artifact();
        let budget = MemoryBudget::new(1000, 500, 100, 1200, 0, 81);
        let handle = backend.load(&artifact, budget).unwrap();
        assert_eq!(handle.artifact, artifact.digest);

        // Admit a sequence through the backend's KV pool.
        assert!(backend.kv().admit(SeqId(0), 128).is_ok());

        let prefill = backend
            .prefill(
                &handle,
                PrefillChunk {
                    seq: SeqId(0),
                    tokens: vec![TokenId(1), TokenId(2), TokenId(3)],
                    position_offset: 0,
                    block_table: drakkar_core::BlockTableRef(0),
                    is_last: true,
                },
            )
            .unwrap();
        assert_eq!(prefill.tokens_processed, 3);

        let out = backend
            .sample(prefill.logits, &SamplerParams::default())
            .unwrap();
        assert_eq!(out.tokens.len(), 1);

        let report = backend.memory_report();
        assert_eq!(report.declared, budget.declared);
        assert!(report.within_budget());
        assert!(backend.capabilities().kv_bits.contains(&8));
    }

    #[test]
    fn greedy_sampling_is_deterministic() {
        let mut backend = MockBackend::new();
        let greedy = SamplerParams {
            temperature: 0.0,
            ..SamplerParams::default()
        };
        let a = backend.sample(LogitsRef(0), &greedy).unwrap();
        let b = backend.sample(LogitsRef(0), &greedy).unwrap();
        assert_eq!(a.tokens, b.tokens);
    }
}
