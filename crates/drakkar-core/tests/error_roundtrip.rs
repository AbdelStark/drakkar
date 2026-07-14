//! Serde round-trip property test for `DkError` (RFC-0011 Testing Strategy).
//!
//! Over registered codes with arbitrary context/remedy params, a `DkError`
//! serializes to the `drakkar.error/1` object and deserializes back to an equal
//! value (minus `source`, which is never serialized).

use drakkar_core::{ALL_ERROR_CODES, ContextValue, DkError, ErrorCode, ErrorContext, Retry};
use proptest::prelude::*;
use serde_json::Value;

fn any_context_value() -> impl Strategy<Value = ContextValue> {
    prop_oneof![
        "[a-zA-Z0-9 ._/-]{0,16}".prop_map(ContextValue::Str),
        any::<i64>().prop_map(ContextValue::Int),
        // Finite floats only; serde_json rejects NaN/Inf and every finite f64
        // round-trips exactly through JSON.
        (-1.0e9f64..1.0e9f64).prop_map(ContextValue::Float),
        any::<bool>().prop_map(ContextValue::Bool),
    ]
}

fn any_context() -> impl Strategy<Value = ErrorContext> {
    prop::collection::vec(("[a-z_]{1,12}", any_context_value()), 0..6).prop_map(|pairs| {
        let mut ctx = ErrorContext::new();
        for (k, v) in pairs {
            ctx = ctx.with(k, v);
        }
        ctx
    })
}

fn any_retry() -> impl Strategy<Value = Retry> {
    prop_oneof![
        Just(Retry::Terminal),
        Just(Retry::AfterBackoff),
        any::<u64>().prop_map(|after_ms| Retry::After { after_ms }),
    ]
}

fn any_code() -> impl Strategy<Value = ErrorCode> {
    prop::sample::select(ALL_ERROR_CODES.to_vec())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn dkerror_serde_round_trips(
        code in any_code(),
        message in "[a-zA-Z0-9 .,]{0,48}",
        context in any_context(),
        retry in any_retry(),
    ) {
        let err = DkError::new(code, message)
            .with_context(context)
            .with_retry(retry);

        let json1: Value = serde_json::to_value(&err).expect("serialize");
        let back: DkError = serde_json::from_value(json1.clone()).expect("deserialize");
        let json2: Value = serde_json::to_value(&back).expect("re-serialize");

        prop_assert_eq!(json1, json2);
        prop_assert!(back.source.is_none());
        prop_assert_eq!(back.code(), code);
        prop_assert_eq!(back.retry, retry);
    }
}
