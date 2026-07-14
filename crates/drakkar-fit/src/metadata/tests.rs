//! FE1 metadata parser tests (RFC-0004 FE1).

use super::*;
use drakkar_core::ErrorCode;

/// A minimal but complete dense `config.json` (Qwen3-8B-shaped).
fn dense_config() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "model_type": "qwen3",
        "num_hidden_layers": 36,
        "hidden_size": 4096,
        "num_attention_heads": 32,
        "num_key_value_heads": 8,
        "head_dim": 128,
        "vocab_size": 151936,
        "intermediate_size": 12288,
        "max_position_embeddings": 40960,
        "tie_word_embeddings": false
    }))
    .unwrap()
}

fn index_with_total(total: u64) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "metadata": { "total_size": total },
        "weight_map": { "model.embed_tokens.weight": "model-00001-of-00002.safetensors" }
    }))
    .unwrap()
}

#[test]
fn metadata_parser_reads_a_dense_config() {
    let d = parse_model_descriptor("Qwen/Qwen3-8B", &dense_config(), None, None, &[]).unwrap();
    assert_eq!(d.reference, "Qwen/Qwen3-8B");
    assert_eq!(d.arch, "qwen3");
    assert_eq!(d.layers, 36);
    assert_eq!(d.hidden, 4096);
    assert_eq!(d.heads, 32);
    assert_eq!(d.kv_heads, 8);
    assert_eq!(d.head_dim, 128);
    assert_eq!(d.vocab, 151936);
    assert_eq!(d.advertised_ctx, 40960);
    assert_eq!(d.quant.bits, 16); // unquantized default
    assert!(d.moe.is_none());
    assert_eq!(d.layout_classes.len(), 36);
    assert!(
        d.layout_classes
            .iter()
            .all(|c| matches!(c, LayoutClass::Global))
    );
    // A dense model's active params equal its total, and it is a sane magnitude
    // (~8B) — a structural estimate, not fabricated precision.
    assert_eq!(d.params_total, d.params_active);
    assert!(
        (7..=9).contains(&(d.params_total / 1_000_000_000)),
        "params {}",
        d.params_total
    );
}

#[test]
fn metadata_parser_index_present_is_exact() {
    // AC: with an index the descriptor reports the exact weight total; without
    // it, the estimate path (exact_weight_bytes = None).
    let total = 16_060_522_496u64;
    let d = parse_model_descriptor(
        "org/m",
        &dense_config(),
        Some(&index_with_total(total)),
        None,
        &[],
    )
    .unwrap();
    assert_eq!(d.exact_weight_bytes(), Some(total));

    let e = parse_model_descriptor("org/m", &dense_config(), None, None, &[]).unwrap();
    assert_eq!(e.exact_weight_bytes(), None);
    assert!(e.tensors.is_empty());
}

#[test]
fn metadata_parser_totals_repo_files() {
    let files = vec![
        RepoFile {
            name: "a.safetensors".into(),
            bytes: 1000,
        },
        RepoFile {
            name: "b.safetensors".into(),
            bytes: 2345,
        },
    ];
    let d = parse_model_descriptor("org/m", &dense_config(), None, None, &files).unwrap();
    assert_eq!(d.repo_total_bytes, 3345);
}

#[test]
fn metadata_parser_detects_moe() {
    let cfg = serde_json::to_vec(&serde_json::json!({
        "model_type": "qwen3_moe",
        "num_hidden_layers": 48,
        "hidden_size": 2048,
        "num_attention_heads": 32,
        "num_key_value_heads": 4,
        "vocab_size": 151936,
        "intermediate_size": 768,
        "num_local_experts": 128,
        "num_experts_per_tok": 8
    }))
    .unwrap();
    let d = parse_model_descriptor("org/moe", &cfg, None, None, &[]).unwrap();
    let moe = d.moe.expect("moe topology");
    assert_eq!(moe.num_experts, 128);
    assert_eq!(moe.experts_per_token, 8);
    // Active params are far below total for a sparse MoE.
    assert!(d.params_active < d.params_total);
}

#[test]
fn metadata_parser_detects_sliding_window() {
    let mut v = serde_json::from_slice::<serde_json::Value>(&dense_config()).unwrap();
    v["sliding_window"] = serde_json::json!(4096);
    let cfg = serde_json::to_vec(&v).unwrap();
    let d = parse_model_descriptor("org/swa", &cfg, None, None, &[]).unwrap();
    assert!(
        d.layout_classes
            .iter()
            .all(|c| matches!(c, LayoutClass::SlidingWindow { window: 4096, .. }))
    );
}

#[test]
fn reject_absurd_layer_count() {
    let mut v = serde_json::from_slice::<serde_json::Value>(&dense_config()).unwrap();
    v["num_hidden_layers"] = serde_json::json!(10_000_000u64);
    let cfg = serde_json::to_vec(&v).unwrap();
    let err = parse_model_descriptor("org/m", &cfg, None, None, &[]).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ModelsInvalidMetadata);
}

#[test]
fn reject_negative_dims() {
    let mut v = serde_json::from_slice::<serde_json::Value>(&dense_config()).unwrap();
    v["hidden_size"] = serde_json::json!(-4096);
    let cfg = serde_json::to_vec(&v).unwrap();
    let err = parse_model_descriptor("org/m", &cfg, None, None, &[]).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ModelsInvalidMetadata);
}

#[test]
fn reject_overflow_tensor_size() {
    // An absurd total_size beyond the sane byte bound is rejected, not summed.
    let err = parse_model_descriptor(
        "org/m",
        &dense_config(),
        Some(&index_with_total(u64::MAX)),
        None,
        &[],
    )
    .unwrap_err();
    assert_eq!(err.code(), ErrorCode::ModelsInvalidMetadata);
}

#[test]
fn truncated_index_errors() {
    // A truncated/malformed index is a structured error, never a panic.
    let truncated = b"{ \"metadata\": { \"total_size\": 123";
    let err =
        parse_model_descriptor("org/m", &dense_config(), Some(truncated), None, &[]).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ModelsInvalidMetadata);

    // A well-formed index missing total_size is also an error, not a silent zero.
    let no_total = serde_json::to_vec(&serde_json::json!({ "weight_map": {} })).unwrap();
    assert_eq!(
        parse_model_descriptor("org/m", &dense_config(), Some(&no_total), None, &[])
            .unwrap_err()
            .code(),
        ErrorCode::ModelsInvalidMetadata
    );
}

#[test]
fn truncated_config_errors() {
    let truncated = b"{ \"model_type\": \"qwen3\", \"num_hidden_layers\":";
    let err = parse_model_descriptor("org/m", truncated, None, None, &[]).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ModelsInvalidMetadata);
}

#[test]
fn missing_required_field_errors() {
    // Missing model_type and missing num_hidden_layers each error by name.
    let no_arch = serde_json::to_vec(&serde_json::json!({
        "num_hidden_layers": 1, "hidden_size": 8, "num_attention_heads": 1, "vocab_size": 4
    }))
    .unwrap();
    let err = parse_model_descriptor("org/m", &no_arch, None, None, &[]).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ModelsInvalidMetadata);
    assert!(err.to_string().contains("model_type"), "{}", err);
}

#[test]
fn parses_a_separate_quant_config() {
    let quant = serde_json::to_vec(&serde_json::json!({
        "quant_method": "mlx", "bits": 4, "group_size": 64
    }))
    .unwrap();
    let d = parse_model_descriptor("org/q", &dense_config(), None, Some(&quant), &[]).unwrap();
    assert_eq!(d.quant.bits, 4);
    assert_eq!(d.quant.group, 64);
    assert_eq!(d.quant.scheme, "mlx");
    assert!(d.quant.bpw_eff > 4.0 && d.quant.bpw_eff < 5.0);
}
