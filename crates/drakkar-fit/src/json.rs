//! The `drakkar.fit/1` JSON surface (RFC-0004 FE26/FE27, public-API §).
//!
//! One serialization is shared by CLI `--json`, `POST /fit`, and any downstream
//! consumer (PRD P7). The schema is additive-only within major 1; a breaking
//! change mints `drakkar.fit/2`. This module renders a [`FitReport`] to the FE26
//! layout and validates it against the checked-in JSON Schema
//! (`schemas/drakkar.fit.1.schema.json`), whose `required` lists are the
//! additive-only guard: removing or renaming a field makes a serialized report
//! fail validation.

use drakkar_core::FitReport;
use serde_json::Value;

/// The checked-in `drakkar.fit/1` JSON Schema (draft-07).
pub const FIT_SCHEMA_JSON: &str = include_str!("../schemas/drakkar.fit.1.schema.json");

/// Render a report to its `drakkar.fit/1` JSON value (FE26).
#[must_use]
pub fn to_json(report: &FitReport) -> Value {
    // FitReport is infallibly serializable (no maps with non-string keys, no
    // non-finite floats produced by the modeled math).
    serde_json::to_value(report).expect("FitReport is always serializable")
}

/// Render a report to a pretty-printed `drakkar.fit/1` JSON string.
#[must_use]
pub fn to_json_string(report: &FitReport) -> String {
    serde_json::to_string_pretty(report).expect("FitReport is always serializable")
}

/// Validate a serialized value against the checked-in `drakkar.fit/1` schema.
///
/// # Errors
/// Returns a human-readable path + reason for the first violation.
pub fn validate_against_fit_schema(value: &Value) -> Result<(), String> {
    let schema: Value =
        serde_json::from_str(FIT_SCHEMA_JSON).expect("checked-in fit schema is valid JSON");
    validate(value, &schema, "$")
}

/// A focused JSON Schema (draft-07 subset) validator: `const`, `enum`, `type`
/// (object/array/string/number/integer/boolean), `required`, `properties`, and
/// `items`. Additive-only: unknown properties are permitted.
fn validate(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(expected) = schema.get("const") {
        if value != expected {
            return Err(format!("{path}: expected const {expected}, got {value}"));
        }
    }
    if let Some(Value::Array(choices)) = schema.get("enum") {
        if !choices.contains(value) {
            return Err(format!("{path}: {value} is not one of {choices:?}"));
        }
    }
    let Some(Value::String(ty)) = schema.get("type") else {
        return Ok(());
    };
    match ty.as_str() {
        "object" => {
            let Value::Object(map) = value else {
                return Err(format!("{path}: expected object, got {value}"));
            };
            if let Some(Value::Array(required)) = schema.get("required") {
                for key in required {
                    if let Value::String(k) = key {
                        if !map.contains_key(k) {
                            return Err(format!("{path}: missing required field `{k}`"));
                        }
                    }
                }
            }
            if let Some(Value::Object(props)) = schema.get("properties") {
                for (k, sub_schema) in props {
                    if let Some(v) = map.get(k) {
                        validate(v, sub_schema, &format!("{path}.{k}"))?;
                    }
                }
            }
            Ok(())
        }
        "array" => {
            let Value::Array(items) = value else {
                return Err(format!("{path}: expected array, got {value}"));
            };
            if let Some(item_schema) = schema.get("items") {
                for (i, item) in items.iter().enumerate() {
                    validate(item, item_schema, &format!("{path}[{i}]"))?;
                }
            }
            Ok(())
        }
        "string" => match value {
            Value::String(_) => Ok(()),
            _ => Err(format!("{path}: expected string, got {value}")),
        },
        "number" | "integer" => match value {
            Value::Number(_) => Ok(()),
            _ => Err(format!("{path}: expected {ty}, got {value}")),
        },
        "boolean" => match value {
            Value::Bool(_) => Ok(()),
            _ => Err(format!("{path}: expected boolean, got {value}")),
        },
        other => Err(format!("{path}: unsupported schema type `{other}`")),
    }
}

#[cfg(test)]
mod json_schema_tests {
    use super::*;
    use crate::{MachineProfile, ModelDescriptor, RequestShape, fit};
    use drakkar_core::{BudgetSource, ChipId, LayoutClass, QuantDesc};

    fn sample_report() -> FitReport {
        let model = ModelDescriptor {
            reference: "Qwen/Qwen3-8B".to_owned(),
            arch: "qwen3".to_owned(),
            layers: 36,
            hidden: 4096,
            heads: 32,
            kv_heads: 8,
            head_dim: 128,
            vocab: 151_936,
            params_total: 8_190_000_000,
            params_active: 8_190_000_000,
            moe: None,
            layout_classes: vec![LayoutClass::Global; 36],
            quant: QuantDesc {
                scheme: "mlx_affine".to_owned(),
                bits: 4,
                group: 64,
                bpw_eff: 4.5,
                recipe: None,
            },
            advertised_ctx: 131_072,
            tensors: Vec::new(),
            repo_total_bytes: 4_600_000_000,
        };
        let machine = MachineProfile {
            chip: ChipId {
                name: "Apple M4 Pro".to_owned(),
                gpu_cores: 20,
            },
            total_ram_bytes: 48 * 1024 * 1024 * 1024,
            budget_bytes: 36 * 1024 * 1024 * 1024,
            budget_source: BudgetSource::Probe,
            wired_limit_mb: 0,
            free_bytes: 0,
            macos: (26, 2),
            nax_tensor_ops: false,
            bandwidth_gbs: 273.0,
            ssd_read_gbs: 6.2,
        };
        fit(&model, &machine, &RequestShape::default())
    }

    #[test]
    fn schema_field_is_drakkar_fit_1() {
        let value = to_json(&sample_report());
        assert_eq!(value["schema"], "drakkar.fit/1");
    }

    #[test]
    fn fit_json_validates_against_schema() {
        let value = to_json(&sample_report());
        validate_against_fit_schema(&value).expect("produced report must validate");
        // Field names/nesting are present.
        assert!(value["memory"].get("confidence").is_some());
        assert!(value["performance"]["decode_tps"].get("value").is_some());
        assert!(value["context"].get("max_fp16").is_some());
    }

    #[test]
    fn additive_only_guard_catches_a_removed_field() {
        let mut value = to_json(&sample_report());
        // Removing a required field (as a field removal/rename would) fails.
        value.as_object_mut().unwrap().remove("verdict");
        let err = validate_against_fit_schema(&value).unwrap_err();
        assert!(err.contains("verdict"), "unexpected error: {err}");

        // Removing a nested required field also fails.
        let mut value = to_json(&sample_report());
        value["memory"].as_object_mut().unwrap().remove("total_gib");
        let err = validate_against_fit_schema(&value).unwrap_err();
        assert!(err.contains("total_gib"), "unexpected error: {err}");
    }

    #[test]
    fn additive_extra_field_is_permitted() {
        let mut value = to_json(&sample_report());
        value
            .as_object_mut()
            .unwrap()
            .insert("future_field".to_owned(), Value::Bool(true));
        // Additive-only: a new field must not break validation.
        validate_against_fit_schema(&value).expect("extra field is allowed");
    }

    #[test]
    fn checked_in_schema_is_well_formed() {
        let schema: Value = serde_json::from_str(FIT_SCHEMA_JSON).expect("schema parses");
        assert_eq!(schema["title"], "drakkar.fit/1");
    }
}
