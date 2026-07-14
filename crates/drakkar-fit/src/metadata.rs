//! FE1 — build a [`ModelDescriptor`] from HuggingFace metadata **without
//! downloading weights** (RFC-0004 FE1, LD10).
//!
//! This is an untrusted-input boundary (SECURITY.md): every field is parsed
//! defensively and bounded, so an absurd, overflowing, or truncated document is
//! rejected with a structured `models.invalid_metadata` error rather than
//! propagated into sizing arithmetic or panicking.
//!
//! Two facts about the inputs shape the design:
//!
//! * **`config.json`** gives the model's structural dimensions (layers, hidden,
//!   heads, vocab, MoE topology, attention layout). Parameter *counts* are not
//!   published there, so they are computed as a deterministic structural
//!   estimate from those dimensions.
//! * **`model.safetensors.index.json`** carries `metadata.total_size` — the
//!   *exact* total weight byte count — and a `weight_map` of tensor names to
//!   shard files. It does **not** carry per-tensor dtypes or byte sizes (those
//!   live in each shard's header, which metadata-only fetch never reads). So the
//!   exact path records the exact total; it never fabricates a per-tensor
//!   breakdown it cannot see. When the index is absent, weight sizing falls to
//!   the estimate path (#227) and [`ModelDescriptor::exact_weight_bytes`]
//!   returns `None`.

use drakkar_core::{DkError, ErrorCode, ErrorContext, LayoutClass, MoeTopology, QuantDesc};
use serde_json::Value;

use crate::model::{ModelDescriptor, TensorEntry};

// Defensive upper bounds. These are orders of magnitude above any real model,
// so a legitimate config never trips them, but an absurd or overflowing value is
// rejected before it reaches arithmetic.
const MAX_LAYERS: u32 = 1 << 16; // 65_536
const MAX_HIDDEN: u32 = 1 << 20; // ~1M
const MAX_HEADS: u32 = 1 << 20;
const MAX_HEAD_DIM: u32 = 1 << 16;
const MAX_VOCAB: u32 = 1 << 24; // ~16M
const MAX_INTERMEDIATE: u32 = 1 << 24;
const MAX_EXPERTS: u32 = 1 << 16;
const MAX_CTX: u32 = 1 << 26; // ~67M tokens
/// 16 TiB — above any conceivable single model, below `u64` overflow risk.
const MAX_WEIGHT_BYTES: u64 = 1 << 44;

/// A published repository file and its size, used to total the repo footprint.
#[derive(Clone, Debug)]
pub struct RepoFile {
    /// The file name (repo-relative).
    pub name: String,
    /// The file size in bytes.
    pub bytes: u64,
}

fn invalid(reference: &str, reason: impl Into<String>) -> DkError {
    let reason = reason.into();
    DkError::new(
        ErrorCode::ModelsInvalidMetadata,
        format!("metadata for '{reference}' is malformed: {reason}"),
    )
    .with_context(
        ErrorContext::new()
            .with_str("ref", reference)
            .with_str("reason", reason),
    )
}

/// Read a required non-negative integer field, bounded by `max`.
fn req_u32(cfg: &Value, field: &str, max: u32, reference: &str) -> Result<u32, DkError> {
    let v = &cfg[field];
    if v.is_null() {
        return Err(invalid(reference, format!("config.json missing '{field}'")));
    }
    read_u32(v, field, max, reference)
}

/// Read an optional non-negative integer field, defaulting when absent.
fn opt_u32(
    cfg: &Value,
    field: &str,
    max: u32,
    default: u32,
    reference: &str,
) -> Result<u32, DkError> {
    let v = &cfg[field];
    if v.is_null() {
        Ok(default)
    } else {
        read_u32(v, field, max, reference)
    }
}

fn read_u32(v: &Value, field: &str, max: u32, reference: &str) -> Result<u32, DkError> {
    // A negative or fractional number is not a valid dimension.
    if let Some(i) = v.as_i64() {
        if i < 0 {
            return Err(invalid(reference, format!("'{field}' is negative ({i})")));
        }
    }
    let n = v.as_u64().ok_or_else(|| {
        invalid(
            reference,
            format!("'{field}' is not a non-negative integer"),
        )
    })?;
    if n > u64::from(max) {
        return Err(invalid(
            reference,
            format!("'{field}' = {n} exceeds the sane bound {max}"),
        ));
    }
    Ok(n as u32)
}

/// Parse the MoE topology if the config describes a mixture of experts.
fn parse_moe(cfg: &Value, reference: &str) -> Result<Option<MoeTopology>, DkError> {
    // HF configs spell the expert count `num_local_experts` (Mixtral) or
    // `num_experts` (others); routed-per-token is `num_experts_per_tok`.
    let num_experts = if !cfg["num_local_experts"].is_null() {
        req_u32(cfg, "num_local_experts", MAX_EXPERTS, reference)?
    } else if !cfg["num_experts"].is_null() {
        req_u32(cfg, "num_experts", MAX_EXPERTS, reference)?
    } else {
        return Ok(None);
    };
    if num_experts == 0 {
        return Ok(None);
    }
    let experts_per_token = opt_u32(cfg, "num_experts_per_tok", MAX_EXPERTS, 1, reference)?;
    if experts_per_token > num_experts {
        return Err(invalid(
            reference,
            format!(
                "num_experts_per_tok ({experts_per_token}) exceeds num_experts ({num_experts})"
            ),
        ));
    }
    // Shared experts are spelled several ways; count is what we need.
    let shared_experts = opt_u32(cfg, "n_shared_experts", MAX_EXPERTS, 0, reference)?;
    Ok(Some(MoeTopology {
        num_experts,
        experts_per_token,
        shared_experts,
    }))
}

/// Derive per-layer KV layout classes (FE9–FE11). Kept deliberately simple: a
/// global sliding window (optionally with a `sliding_window_pattern` that makes
/// every Nth layer full-attention), MLA when the latent-attention fields are
/// present, else global attention everywhere.
fn parse_layouts(cfg: &Value, layers: u32, reference: &str) -> Result<Vec<LayoutClass>, DkError> {
    // MLA (DeepSeek-style latent attention).
    if !cfg["kv_lora_rank"].is_null() {
        let c_kv = req_u32(cfg, "kv_lora_rank", MAX_HIDDEN, reference)?;
        let d_rope = opt_u32(cfg, "qk_rope_head_dim", MAX_HEAD_DIM, 0, reference)?;
        return Ok(vec![
            LayoutClass::MlaLatent { c_kv, d_rope };
            layers as usize
        ]);
    }
    // Sliding-window attention.
    if !cfg["sliding_window"].is_null() {
        let window = read_u32(&cfg["sliding_window"], "sliding_window", MAX_CTX, reference)?;
        if window > 0 {
            // Gemma-style: one full-attention layer every `pattern` layers.
            let pattern = opt_u32(cfg, "sliding_window_pattern", MAX_LAYERS, 0, reference)?;
            let classes = (0..layers)
                .map(|i| {
                    let is_full = pattern > 0 && (i + 1) % pattern == 0;
                    if is_full {
                        LayoutClass::Global
                    } else {
                        LayoutClass::SlidingWindow { window, sinks: 0 }
                    }
                })
                .collect();
            return Ok(classes);
        }
    }
    Ok(vec![LayoutClass::Global; layers as usize])
}

/// Parse a quantization descriptor from a `quantization_config` document (or the
/// same object embedded in `config.json`), defaulting to an unquantized f16
/// descriptor when absent.
fn parse_quant(quant: Option<&[u8]>, cfg: &Value, reference: &str) -> Result<QuantDesc, DkError> {
    let qc: Value = match quant {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|e| invalid(reference, format!("quant config: {e}")))?,
        None if !cfg["quantization_config"].is_null() => cfg["quantization_config"].clone(),
        None => {
            // Unquantized: 16-bit weights, no grouping.
            return Ok(QuantDesc {
                scheme: "none".to_owned(),
                bits: 16,
                group: 0,
                bpw_eff: 16.0,
                recipe: None,
            });
        }
    };
    let scheme = qc["quant_method"]
        .as_str()
        .or_else(|| qc["scheme"].as_str())
        .unwrap_or("unknown")
        .to_owned();
    let bits = read_u32(
        qc.get("bits").unwrap_or(&Value::Null),
        "bits",
        64,
        reference,
    )
    .unwrap_or(4)
    .clamp(1, 64) as u8;
    let group = opt_u32(&qc, "group_size", MAX_HIDDEN, 0, reference)?;
    // Effective bits per weight: the stored bits plus scale/zero overhead
    // amortized over the group (a small, standard approximation).
    let bpw_eff = if group > 0 {
        f64::from(bits) + 32.0 / f64::from(group)
    } else {
        f64::from(bits)
    } as f32;
    Ok(QuantDesc {
        scheme,
        bits,
        group,
        bpw_eff,
        recipe: None,
    })
}

/// Parse the safetensors index for the exact total weight bytes. The index
/// carries `metadata.total_size` (exact) and a `weight_map`; per-tensor sizes are
/// not present, so the exact total is recorded as a single aggregate entry.
fn parse_index(bytes: &[u8], reference: &str) -> Result<Vec<TensorEntry>, DkError> {
    let idx: Value = serde_json::from_slice(bytes)
        .map_err(|e| invalid(reference, format!("safetensors index: {e}")))?;
    let total = &idx["metadata"]["total_size"];
    if total.is_null() {
        return Err(invalid(
            reference,
            "safetensors index has no metadata.total_size",
        ));
    }
    if let Some(i) = total.as_i64() {
        if i < 0 {
            return Err(invalid(reference, format!("total_size is negative ({i})")));
        }
    }
    let bytes_total = total
        .as_u64()
        .ok_or_else(|| invalid(reference, "total_size is not a non-negative integer"))?;
    if bytes_total > MAX_WEIGHT_BYTES {
        return Err(invalid(
            reference,
            format!("total_size {bytes_total} exceeds the sane bound {MAX_WEIGHT_BYTES}"),
        ));
    }
    // The index is a name→shard map with an exact total, not a per-tensor size
    // table; record the exact aggregate so `exact_weight_bytes()` is exact.
    Ok(vec![TensorEntry {
        name: "*safetensors-index-total*".to_owned(),
        dtype: "mixed".to_owned(),
        bytes: bytes_total,
    }])
}

/// The structural dimensions a parameter-count estimate needs.
struct ParamDims {
    layers: u32,
    hidden: u32,
    heads: u32,
    kv_heads: u32,
    head_dim: u32,
    vocab: u32,
    intermediate: u32,
    tied_embeddings: bool,
}

/// A deterministic structural estimate of parameter counts from the model
/// dimensions (SwiGLU-style 3-matrix MLP, GQA-aware attention, embeddings).
/// Returns `(total, active)`; for a dense model the two are equal, for MoE the
/// active count uses the routed-plus-shared experts. This is a structural
/// estimate, refined by the exact weight-sizing path (FE5) when the index gives
/// an exact byte total.
fn estimate_params(d: &ParamDims, moe: Option<&MoeTopology>) -> (u64, u64) {
    let hidden = u64::from(d.hidden);
    let head_dim = u64::from(d.head_dim);
    let q_dim = u64::from(d.heads) * head_dim;
    let kv_dim = u64::from(d.kv_heads) * head_dim;
    // Attention: q, k, v, o projections.
    let attn = hidden * q_dim + 2 * hidden * kv_dim + q_dim * hidden;
    // One expert's MLP (gate, up, down).
    let mlp_one = 3 * hidden * u64::from(d.intermediate);

    let (mlp_total, mlp_active) = match moe {
        Some(m) => {
            let n = u64::from(m.num_experts);
            let active = u64::from(m.experts_per_token) + u64::from(m.shared_experts);
            (mlp_one * n, mlp_one * active)
        }
        None => (mlp_one, mlp_one),
    };

    let per_layer_total = attn + mlp_total;
    let per_layer_active = attn + mlp_active;
    let embed = u64::from(d.vocab) * hidden;
    // Untied models carry a separate lm_head of the same size.
    let embed_params = if d.tied_embeddings { embed } else { 2 * embed };

    let total = embed_params + u64::from(d.layers) * per_layer_total;
    let active = embed_params + u64::from(d.layers) * per_layer_active;
    (total, active)
}

/// Parse HuggingFace metadata into a validated [`ModelDescriptor`] (FE1).
///
/// `config` is `config.json`; `index` is `model.safetensors.index.json` when the
/// repo publishes one (enabling the exact weight total); `quant` is a
/// `quantization_config` document when separate from `config.json`; `files` is
/// the repo's published file-size list.
///
/// # Errors
/// Returns `models.invalid_metadata` for malformed/truncated JSON, a missing
/// required field, or an out-of-range / overflowing integer — never a panic.
pub fn parse_model_descriptor(
    reference: &str,
    config: &[u8],
    index: Option<&[u8]>,
    quant: Option<&[u8]>,
    files: &[RepoFile],
) -> Result<ModelDescriptor, DkError> {
    let cfg: Value = serde_json::from_slice(config)
        .map_err(|e| invalid(reference, format!("config.json: {e}")))?;
    if !cfg.is_object() {
        return Err(invalid(reference, "config.json is not a JSON object"));
    }

    let arch = cfg["model_type"]
        .as_str()
        .ok_or_else(|| invalid(reference, "config.json missing 'model_type'"))?
        .to_owned();

    let layers = req_u32(&cfg, "num_hidden_layers", MAX_LAYERS, reference)?;
    let hidden = req_u32(&cfg, "hidden_size", MAX_HIDDEN, reference)?;
    let heads = req_u32(&cfg, "num_attention_heads", MAX_HEADS, reference)?;
    if heads == 0 {
        return Err(invalid(reference, "num_attention_heads must be > 0"));
    }
    let kv_heads = opt_u32(&cfg, "num_key_value_heads", MAX_HEADS, heads, reference)?;
    let head_dim = opt_u32(&cfg, "head_dim", MAX_HEAD_DIM, hidden / heads, reference)?;
    let vocab = req_u32(&cfg, "vocab_size", MAX_VOCAB, reference)?;
    let intermediate = opt_u32(
        &cfg,
        "intermediate_size",
        MAX_INTERMEDIATE,
        hidden.saturating_mul(4),
        reference,
    )?;
    let advertised_ctx = opt_u32(&cfg, "max_position_embeddings", MAX_CTX, 0, reference)?;
    let tied = cfg["tie_word_embeddings"].as_bool().unwrap_or(false);

    let moe = parse_moe(&cfg, reference)?;
    let layout_classes = parse_layouts(&cfg, layers, reference)?;
    let quant = parse_quant(quant, &cfg, reference)?;
    let tensors = match index {
        Some(bytes) => parse_index(bytes, reference)?,
        None => Vec::new(),
    };
    let repo_total_bytes = files.iter().map(|f| f.bytes).sum();

    let (params_total, params_active) = estimate_params(
        &ParamDims {
            layers,
            hidden,
            heads,
            kv_heads,
            head_dim,
            vocab,
            intermediate,
            tied_embeddings: tied,
        },
        moe.as_ref(),
    );

    Ok(ModelDescriptor {
        reference: reference.to_owned(),
        arch,
        layers,
        hidden,
        heads,
        kv_heads,
        head_dim,
        vocab,
        params_total,
        params_active,
        moe,
        layout_classes,
        quant,
        advertised_ctx,
        tensors,
        repo_total_bytes,
    })
}

#[cfg(test)]
mod tests;
