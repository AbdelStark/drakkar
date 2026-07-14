//! Golden round-trip for `drakkar.fit/1` (INV-MIRROR, DM17).
//!
//! Constructs the FE26 example [`FitReport`] and asserts it serializes to
//! exactly the FE26 example JSON — same field names, nesting, and units. Numeric
//! comparison is by value (so the example's `48` and a serialized `48.0` are
//! equal), which is the correct notion of "byte-compatible" for JSON: the field
//! set and the numeric values must match.

use drakkar_core::{
    BudgetSource, Confidence, Estimate, FIT_SCHEMA, FitContext, FitMachine, FitMemory, FitModel,
    FitPerformance, FitReport, QuantDesc, TtftEstimate, Verdict,
};
use serde_json::Value;

/// Recursively compare two JSON values, treating numbers as equal when their
/// `f64` values match and requiring identical object key sets.
fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, xv)| y.get(k).is_some_and(|yv| json_eq(xv, yv)))
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(xv, yv)| json_eq(xv, yv))
        }
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(xf), Some(yf)) => xf == yf,
            _ => x == y,
        },
        _ => a == b,
    }
}

fn fe26_example() -> FitReport {
    FitReport {
        schema: FIT_SCHEMA,
        model: FitModel {
            id: "Qwen/Qwen3-8B".to_owned(),
            arch: "qwen3".to_owned(),
            params_total: 8.19e9,
            params_active: 8.19e9,
            quant: QuantDesc {
                scheme: "mlx_affine".to_owned(),
                bits: 4,
                group: 64,
                bpw_eff: 4.5,
                recipe: None,
            },
        },
        machine: FitMachine {
            chip: "Apple M4 Pro".to_owned(),
            ram_gib: 48.0,
            budget_gib: 36.0,
            budget_source: BudgetSource::Probe,
            bandwidth_gbs: 273.0,
            nax: false,
            wired_limit_mb: 0,
        },
        memory: FitMemory {
            weights_gib: 4.21,
            kv_per_token_kib: 144.0,
            kv_at_ctx_gib: 4.5,
            activation_gib: 0.4,
            runtime_gib: 1.2,
            total_gib: 10.4,
            confidence: Confidence::Modeled,
        },
        verdict: Verdict::Comfortable,
        headroom_gib: 25.6,
        context: FitContext {
            requested: 32768,
            max_fp16: 214_000,
            max_kv8: 468_000,
            max_kv4: None,
            advertised: 131_072,
        },
        performance: FitPerformance {
            decode_tps: Estimate {
                value: 55.0,
                confidence: Confidence::Calibrated,
            },
            ttft_cold_s: TtftEstimate {
                value: 1.9,
                prompt: 4096,
                confidence: Confidence::Modeled,
            },
            load_s: 1.4,
        },
        remedies: Vec::new(),
    }
}

#[test]
fn fit_report_mirrors_fe26_example_verbatim() {
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/fit_qwen3_8b.json")).expect("valid fixture");
    let produced: Value = serde_json::to_value(fe26_example()).expect("serializable");

    assert!(
        json_eq(&produced, &expected),
        "FitReport diverged from drakkar.fit/1 FE26 example.\nproduced: {}\nexpected: {}",
        serde_json::to_string_pretty(&produced).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
    );
}

#[test]
fn schema_tag_is_drakkar_fit_1() {
    assert_eq!(FIT_SCHEMA.name(), "drakkar.fit");
    assert_eq!(FIT_SCHEMA.major(), Some(1));
}
