//! Tests for the error taxonomy (error-model §2/§4/§5, RFC-0011).

use super::*;
use serde_json::{Value, json};

#[test]
fn registry_has_exactly_36_distinct_codes() {
    assert_eq!(ALL_ERROR_CODES.len(), 36);
    let mut strings: Vec<&str> = ALL_ERROR_CODES.iter().map(|c| c.as_str()).collect();
    strings.sort_unstable();
    strings.dedup();
    assert_eq!(strings.len(), 36, "code strings must be distinct");
}

#[test]
fn as_str_matches_registry_examples() {
    assert_eq!(ErrorCode::KvPoolExhausted.as_str(), "kv.pool_exhausted");
    assert_eq!(ErrorCode::FitWontFit.as_str(), "fit.wont_fit");
    assert_eq!(ErrorCode::InternalPanic.as_str(), "internal.panic");
    assert_eq!(ErrorCode::BackendIo.as_str(), "backend.io");
    // Every dotted string is `subsystem.snake_case` with a closed prefix set.
    let prefixes = [
        "cli", "config", "models", "download", "store", "fit", "kv", "engine", "backend", "abi",
        "grammar", "server", "internal",
    ];
    for code in ALL_ERROR_CODES {
        let (prefix, _) = code.as_str().split_once('.').expect("dotted code");
        assert!(
            prefixes.contains(&prefix),
            "unexpected prefix in {}",
            code.as_str()
        );
    }
}

#[test]
fn from_code_str_round_trips_every_code() {
    for code in ALL_ERROR_CODES {
        assert_eq!(ErrorCode::from_code_str(code.as_str()), Some(code));
    }
    assert_eq!(ErrorCode::from_code_str("no.such_code"), None);
}

#[test]
fn http_status_overrides_are_the_only_deviations() {
    assert_eq!(ErrorCode::FitContextExceeded.http_status(), 413);
    assert_eq!(ErrorCode::KvPoolExhausted.http_status(), 429);
    assert_eq!(ErrorCode::GrammarSchemaCompileFailed.http_status(), 422);
    assert_eq!(ErrorCode::ServerModelLoading.http_status(), 503);
    // A representative non-override in each category equals its default.
    assert_eq!(ErrorCode::CliInvalidArgs.http_status(), 400);
    assert_eq!(ErrorCode::ModelsNotFound.http_status(), 404);
    assert_eq!(ErrorCode::FitWontFit.http_status(), 422);
    assert_eq!(ErrorCode::DownloadHubUnreachable.http_status(), 503);
    assert_eq!(ErrorCode::EngineLoadFailed.http_status(), 500);
    assert_eq!(ErrorCode::DownloadNoSpace.http_status(), 507);
    assert_eq!(ErrorCode::InternalPanic.http_status(), 500);
}

#[test]
fn category_to_exit_code_map() {
    assert_eq!(ErrorCategory::Usage.exit_code(), 2);
    assert_eq!(ErrorCategory::ModelNotFound.exit_code(), 3);
    assert_eq!(ErrorCategory::Infeasible.exit_code(), 4);
    assert_eq!(ErrorCategory::Network.exit_code(), 5);
    assert_eq!(ErrorCategory::Format.exit_code(), 6);
    assert_eq!(ErrorCategory::Engine.exit_code(), 6);
    assert_eq!(ErrorCategory::Disk.exit_code(), 7);
    assert_eq!(ErrorCategory::Internal.exit_code(), 6);
}

#[test]
fn every_non_internal_code_has_a_remedy_and_internal_is_exempt() {
    for code in ALL_ERROR_CODES {
        let is_internal_prefix = code.as_str().starts_with("internal.");
        assert_eq!(
            code.remedy_exempt(),
            is_internal_prefix,
            "{}",
            code.as_str()
        );
        if is_internal_prefix {
            assert!(
                code.remedy_template().is_none(),
                "{} should be exempt",
                code.as_str()
            );
        } else {
            assert!(
                code.remedy_template().is_some(),
                "{} should bind a remedy template",
                code.as_str()
            );
        }
    }
}

#[test]
fn seed_templates_are_bound_to_their_codes() {
    assert_eq!(
        ErrorCode::FitWontFit.remedy_template().unwrap().id,
        "run_sibling"
    );
    assert_eq!(
        ErrorCode::KvPoolExhausted.remedy_template().unwrap().id,
        "retry_after_or_reduce"
    );
    assert_eq!(
        ErrorCode::FitContextExceeded.remedy_template().unwrap().id,
        "reduce_context"
    );
    assert_eq!(
        ErrorCode::DownloadNetworkFailed
            .remedy_template()
            .unwrap()
            .id,
        "resume_pull"
    );
    assert_eq!(
        ErrorCode::DownloadNoSpace.remedy_template().unwrap().id,
        "prune_store"
    );
    assert_eq!(
        ErrorCode::ModelsGatedRepoNoToken
            .remedy_template()
            .unwrap()
            .id,
        "accept_license"
    );
}

#[test]
fn render_substitutes_placeholders_and_preserves_literal_braces() {
    let ctx = ErrorContext::new().with_str("command", "run");
    let rendered = ErrorCode::CliInvalidArgs
        .remedy_template()
        .unwrap()
        .render(&ctx);
    assert_eq!(
        rendered.rendered,
        "Run 'drakkar run --help' for accepted flags and arguments."
    );
    // The grammar remedy contains literal JSON braces that are not placeholders.
    let rendered = ErrorCode::GrammarSchemaCompileFailed
        .remedy_template()
        .unwrap()
        .render(&ErrorContext::new().with_str("reason", "unbounded recursion"));
    assert!(rendered.rendered.contains("{\"type\":\"json_object\"}"));
    assert!(rendered.rendered.contains("unbounded recursion"));
}

#[test]
fn dkerror_serializes_to_drakkar_error_1_object() {
    let ctx = ErrorContext::new()
        .with_float("needed_gib", 39.1)
        .with_float("usable_gib", 34.2)
        .with_str("sibling", "qwen3:30b-a3b");
    let err = DkError::new(
        ErrorCode::FitWontFit,
        "Llama-3.3-70B-4bit needs 39.1 GiB even at the floor plan; usable budget is 34.2 GiB.",
    )
    .with_context(ctx);

    let value: Value = serde_json::to_value(&err).unwrap();
    assert_eq!(value["schema"], "drakkar.error/1");
    assert_eq!(value["code"], "fit.wont_fit");
    assert_eq!(value["category"], "infeasible");
    assert_eq!(value["exit_code"], 4);
    assert_eq!(
        value["retry"],
        json!({"kind": "terminal", "after_ms": null})
    );
    assert_eq!(value["remedy"]["template"], "run_sibling");
    assert_eq!(value["context"]["sibling"], "qwen3:30b-a3b");
    // http_status and source are never in the CLI envelope.
    assert!(value.get("http_status").is_none());
    assert!(value.get("source").is_none());
}

#[test]
fn dkerror_round_trips_through_json_minus_source() {
    let err = DkError::new(
        ErrorCode::KvPoolExhausted,
        "KV pool at 97% with no reclaimable blocks.",
    )
    .with_context(
        ErrorContext::new()
            .with_int("retry_after_ms", 1800)
            .with_float("pool_occupancy", 0.97),
    )
    .with_retry(Retry::After { after_ms: 1800 })
    .with_source(std::io::Error::other("cause chain, never serialized"));

    let json1: Value = serde_json::to_value(&err).unwrap();
    let back: DkError = serde_json::from_value(json1.clone()).unwrap();
    let json2: Value = serde_json::to_value(&back).unwrap();
    assert_eq!(json1, json2, "round-trip must be stable minus source");
    assert!(back.source.is_none(), "source is never serialized");
    assert_eq!(back.retry, Retry::After { after_ms: 1800 });
    assert_eq!(back.code, ErrorCode::KvPoolExhausted);
}

#[test]
fn retry_wire_shape() {
    assert_eq!(
        serde_json::to_value(Retry::Terminal).unwrap(),
        json!({"kind": "terminal", "after_ms": null})
    );
    assert_eq!(
        serde_json::to_value(Retry::AfterBackoff).unwrap(),
        json!({"kind": "after_backoff", "after_ms": null})
    );
    assert_eq!(
        serde_json::to_value(Retry::After { after_ms: 1800 }).unwrap(),
        json!({"kind": "after", "after_ms": 1800})
    );
}
