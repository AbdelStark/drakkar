//! The model descriptor input (RFC-0004 FE1).

use drakkar_core::{LayoutClass, MoeTopology, QuantDesc};

/// One tensor's dtype and size from the safetensors index (FE1 exact path,
/// LD10). Present only when the repo publishes `model.safetensors.index.json`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TensorEntry {
    /// The tensor name.
    pub name: String,
    /// The dtype string as published, e.g. `"F16"`, `"BF16"`, `"U8"`.
    pub dtype: String,
    /// The tensor size in bytes.
    pub bytes: u64,
}

/// Everything the feasibility engine needs about a model, resolved from HF
/// metadata **without downloading weights** (FE1). Parsing raw `config.json` and
/// the safetensors index into this shape is the metadata parser's job (#226);
/// this crate only fixes the shape and never performs I/O.
#[derive(Clone, PartialEq, Debug)]
pub struct ModelDescriptor {
    /// The model reference, e.g. `"Qwen/Qwen3-8B"`.
    pub reference: String,
    /// Architecture family name, e.g. `"qwen3"`.
    pub arch: String,
    /// Number of transformer layers.
    pub layers: u32,
    /// Hidden size.
    pub hidden: u32,
    /// Number of attention (query) heads.
    pub heads: u32,
    /// Number of key/value heads (GQA).
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
    pub moe: Option<MoeTopology>,
    /// Per-layer KV layout classes (FE9–FE11: sliding-window, MLA, recurrent).
    pub layout_classes: Vec<LayoutClass>,
    /// Quantization descriptor (already-quantized artifact, or a projection).
    pub quant: QuantDesc,
    /// Advertised maximum context length.
    pub advertised_ctx: u32,
    /// Per-tensor sizes from the safetensors index (FE5 exact path); empty when
    /// the repo publishes no index.
    pub tensors: Vec<TensorEntry>,
    /// Total repo file size in bytes (all published files).
    pub repo_total_bytes: u64,
}

impl ModelDescriptor {
    /// The exact weight byte total from the safetensors index (FE5 exact path),
    /// or `None` when no per-tensor sizes are available and the estimate path
    /// (#227) must be used instead.
    #[must_use]
    pub fn exact_weight_bytes(&self) -> Option<u64> {
        if self.tensors.is_empty() {
            None
        } else {
            Some(self.tensors.iter().map(|t| t.bytes).sum())
        }
    }
}
