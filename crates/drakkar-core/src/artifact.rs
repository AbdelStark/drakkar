//! Model artifacts and backend handles (data-model §3.2).
//!
//! [`ModelArtifact`] is the model manager's output: everything a backend needs
//! to load, resolved to immutable blobs. [`ModelHandle`] is the backend's
//! receipt — opaque above the seam and thread-confined below it (`!Send`/`!Sync`
//! by construction, DM13/INV-CONFINE).

use std::marker::PhantomData;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::Sha256;
use crate::memory::MemoryBudget;

/// The on-disk artifact format (DM: `ArtifactFormat`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactFormat {
    /// MLX-native quantized safetensors.
    MlxSafetensors,
    /// Upstream safetensors (fp16/bf16 or upstream quant).
    Safetensors,
    /// GGUF (secondary backend path).
    Gguf,
}

/// A quantization descriptor (FE5/FE6, IC6). `recipe` is optional: it names a
/// curated per-model recipe when one applies and is omitted otherwise.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct QuantDesc {
    /// Quantization scheme, e.g. `"mlx_affine"`.
    pub scheme: String,
    /// Bits per stored weight, e.g. `4`, `8`, `16`.
    pub bits: u8,
    /// Group size for group-wise schemes; `0` when not group-wise.
    pub group: u32,
    /// Effective bits per weight including scales/zeros overhead.
    pub bpw_eff: f32,
    /// Curated recipe identifier, when one applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
}

/// The per-layer KV layout class (DM14, RFC-0005 §3). Both the feasibility
/// engine (FE8–FE11) and the KV pool key their arithmetic and allocation off
/// this single per-layer enumeration.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutClass {
    /// Global attention; paged.
    Global,
    /// Sliding-window attention; a per-sequence ring buffer outside the pool.
    SlidingWindow {
        /// Window length in tokens.
        window: u32,
        /// Number of attention-sink tokens retained at the front.
        sinks: u32,
    },
    /// Multi-head latent attention; paged blocks store the shared latent.
    MlaLatent {
        /// Compressed KV latent dimension.
        c_kv: u32,
        /// Decoupled rope dimension.
        d_rope: u32,
    },
    /// Recurrent / SSM state; constant per-sequence state.
    Recurrent {
        /// Bytes of state carried per sequence.
        state_bytes: u64,
    },
}

/// Mixture-of-experts topology, when the architecture is an MoE.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct MoeTopology {
    /// Total number of experts per MoE layer.
    pub num_experts: u32,
    /// Experts routed to per token.
    pub experts_per_token: u32,
    /// Always-on shared experts, if any.
    #[serde(default)]
    pub shared_experts: u32,
}

/// The parsed model architecture (from `config.json`, FE1). Backend-neutral: it
/// carries dimensions, MoE topology, and the per-layer KV layout classes, and
/// names no backend type (INV-SEAM).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ArchDescriptor {
    /// Architecture family name, e.g. `"qwen3"`.
    pub name: String,
    /// Number of transformer layers.
    pub layers: u32,
    /// Hidden size.
    pub hidden: u32,
    /// Number of attention (query) heads.
    pub heads: u32,
    /// Number of key/value heads (GQA); equals `heads` for MHA.
    pub kv_heads: u32,
    /// Per-head dimension.
    pub head_dim: u32,
    /// Vocabulary size.
    pub vocab: u32,
    /// Total parameters.
    pub params_total: u64,
    /// Active parameters per token (equals `params_total` for dense models).
    pub params_active: u64,
    /// MoE topology, when the architecture is a mixture of experts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moe: Option<MoeTopology>,
    /// The KV layout class of each layer, in layer order (DM14).
    pub layout_classes: Vec<LayoutClass>,
}

/// A reference to one immutable blob in the content-addressed store.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct BlobRef {
    /// Content digest, and the blob's file name in the store.
    pub digest: Sha256,
    /// Size in bytes.
    pub bytes: u64,
    /// Human-readable file name, e.g. `"model-00001-of-00002.safetensors"`.
    pub name: String,
}

/// The tool-call dialect for a model family; drives prompt rendering and
/// incremental stream parsing (AS9). Extended as new families are added.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDialect {
    /// The model family declares no native tool-call convention.
    None,
    /// Nous / Hermes-style `<tool_call>` JSON blocks.
    Hermes,
    /// Qwen-family tool-call convention.
    Qwen,
    /// Mistral-family `[TOOL_CALLS]` convention.
    Mistral,
    /// Llama-3-family tool-call convention.
    Llama,
    /// DeepSeek-family tool-call convention.
    DeepSeek,
    /// gpt-oss tool-call convention.
    GptOss,
}

/// The model manager's output: everything a backend needs to load, resolved to
/// immutable blobs (RFC-0006). Backend-neutral (INV-SEAM).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ModelArtifact {
    /// Digest of the manifest body; the identity of the artifact.
    pub digest: Sha256,
    /// Path to the manifest file under `~/.drakkar/models/manifests/...`.
    pub manifest_path: PathBuf,
    /// The on-disk format.
    pub format: ArtifactFormat,
    /// Quantization descriptor.
    pub quant: QuantDesc,
    /// The parsed architecture.
    pub arch: ArchDescriptor,
    /// Weight blobs, ordered for mmap.
    pub weights: Vec<BlobRef>,
    /// Tokenizer blob.
    pub tokenizer: BlobRef,
    /// Tokenizer content hash (feeds KV12 keys, MP17).
    pub tokenizer_hash: Sha256,
    /// Chat-template blob.
    pub chat_template: BlobRef,
    /// Chat-template content hash **after** override-table patching (MP18).
    pub chat_template_hash: Sha256,
    /// Tool-call dialect for this model family.
    pub tool_dialect: ToolDialect,
    /// Advertised maximum context length.
    pub advertised_ctx: u32,
}

/// The backend's receipt for a loaded model. Opaque above the seam; `!Send` and
/// `!Sync` by construction so it never leaves the engine actor thread
/// (DM13/INV-CONFINE, RFC-0001 A2). The scheduler holds only `instance` ids in
/// its bookkeeping, never the handle.
#[derive(Debug)]
pub struct ModelHandle {
    /// Unique per load within the process.
    pub instance: u64,
    /// The exact artifact digest this handle ties to.
    pub artifact: Sha256,
    /// The contract this instance was loaded under.
    pub budget: MemoryBudget,
    // `*const ()` makes the handle `!Send` and `!Sync`.
    _confined: PhantomData<*const ()>,
}

impl ModelHandle {
    /// Construct a handle for a freshly loaded model instance. Callable only on
    /// the engine actor thread (the type's `!Send`/`!Sync` bound keeps it
    /// there).
    #[must_use]
    pub fn new(instance: u64, artifact: Sha256, budget: MemoryBudget) -> Self {
        ModelHandle {
            instance,
            artifact,
            budget,
            _confined: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_format_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&ArtifactFormat::MlxSafetensors).unwrap(),
            "\"mlx_safetensors\""
        );
    }

    #[test]
    fn quant_desc_omits_absent_recipe() {
        let q = QuantDesc {
            scheme: "mlx_affine".into(),
            bits: 4,
            group: 64,
            bpw_eff: 4.5,
            recipe: None,
        };
        let v: serde_json::Value = serde_json::to_value(&q).unwrap();
        assert!(v.get("recipe").is_none());
    }

    #[test]
    fn model_handle_is_constructible() {
        let h = ModelHandle::new(1, Sha256([0u8; 32]), MemoryBudget::new(1, 1, 1, 1, 0, 0));
        assert_eq!(h.instance, 1);
    }
}
