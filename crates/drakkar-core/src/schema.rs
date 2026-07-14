//! Schema-version reading and writing (data-model §2.2, DM7–DM10, INV-SCHEMA,
//! architecture invariant I4).
//!
//! Every persistent file and versioned JSON surface carries
//! `"schema": "drakkar.<name>/<major>"` and evolves additive-only within a major
//! (DM8): readers ignore unknown fields, and a reader MUST reject a payload
//! whose major exceeds the newest it implements (DM9) rather than best-effort
//! parse it. This module is the one helper every schema-bearing type uses.
//!
//! Two identifier strings must not be conflated (error-model §8):
//! `drakkar.error/1` is the schema tag of a serialized error *object* — a value
//! of a `schema` field. `drakkar.errors/1` is the *registry-contract* version of
//! the error-code set; it is governance prose and never appears in a `schema`
//! field.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{DkError, ErrorCode, ErrorContext};
use crate::ids::SchemaTag;

/// Where a versioned payload came from, which decides how a too-new major is
/// handled (DM9).
#[derive(Clone, Copy, Debug)]
pub enum Surface<'a> {
    /// A user-owned file (`config.toml`): a too-new major is the named hard
    /// error `config.invalid_value`.
    UserOwned {
        /// The file path, named in the error message.
        file: &'a str,
    },
    /// A reconstructible store-managed file (§4.2–§4.4): a too-new major is not
    /// an error; the file is regenerated on the next write (DM34).
    ReconstructibleStore,
}

/// The outcome of [`read_versioned`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum VersionedRead<T> {
    /// The payload was within the supported major and deserialized.
    Value(T),
    /// A store-managed file was a newer major; regenerate it on next write.
    Regenerate {
        /// The major found in the file.
        found_major: u32,
        /// The newest major this build implements.
        supported_major: u32,
    },
}

/// The major version carried by a JSON payload's `schema` field, or `None` when
/// the field is absent — which reads as major 1 for `config.toml` (DM7).
#[must_use]
pub fn payload_major(payload: &Value) -> Option<u32> {
    let tag = payload.get("schema")?.as_str()?;
    SchemaTag::parse(tag).map(|p| p.major)
}

/// Read a versioned JSON payload, gating the major before deserializing (DM8/DM9).
///
/// An absent `schema` field reads as major 1 (DM7). A major within
/// `supported_major` deserializes `T`, ignoring unknown fields (additive-only,
/// DM8). A major that exceeds `supported_major` is rejected per `surface`: a
/// user-owned file yields `config.invalid_value` naming the file, the found
/// major, the supported major, and the remedy; a reconstructible store file
/// yields [`VersionedRead::Regenerate`].
///
/// # Errors
/// Returns [`ErrorCode::ConfigInvalidValue`] for a too-new user-owned file, and
/// [`ErrorCode::ConfigInvalidValue`] if the payload's `schema` field is present
/// but malformed.
pub fn read_versioned<T: DeserializeOwned>(
    payload: &Value,
    supported_major: u32,
    surface: Surface<'_>,
) -> Result<VersionedRead<T>, DkError> {
    let found_major = match payload.get("schema") {
        // Absent `schema` reads as major 1 (DM7).
        None => 1,
        Some(Value::String(tag)) => match SchemaTag::parse(tag) {
            Some(parsed) => parsed.major,
            None => return Err(malformed_schema(tag, surface)),
        },
        Some(other) => return Err(malformed_schema(&other.to_string(), surface)),
    };

    if found_major > supported_major {
        return match surface {
            Surface::UserOwned { file } => Err(too_new_major(file, found_major, supported_major)),
            Surface::ReconstructibleStore => Ok(VersionedRead::Regenerate {
                found_major,
                supported_major,
            }),
        };
    }

    // Within a known major: deserialize, ignoring unknown fields (serde default;
    // `deny_unknown_fields` MUST NOT be used on versioned surfaces, DM8).
    let value = serde_json::from_value(payload.clone()).map_err(|e| {
        DkError::new(
            ErrorCode::ConfigInvalidValue,
            format!("malformed schema payload: {e}"),
        )
    })?;
    Ok(VersionedRead::Value(value))
}

/// Serialize `value` and stamp it with `tag`, always emitting the newest minor
/// shape (DM8). Panics only if `value` does not serialize to a JSON object.
///
/// # Errors
/// Returns an error if `value` fails to serialize.
pub fn write_versioned<T: Serialize>(value: &T, tag: SchemaTag) -> Result<Value, DkError> {
    let mut json = serde_json::to_value(value).map_err(|e| {
        DkError::new(
            ErrorCode::InternalInvariant,
            format!("failed to serialize versioned payload: {e}"),
        )
    })?;
    match json.as_object_mut() {
        Some(map) => {
            map.insert("schema".to_owned(), Value::String(tag.0.to_owned()));
            Ok(json)
        }
        None => Err(DkError::new(
            ErrorCode::InternalInvariant,
            "versioned payload did not serialize to a JSON object",
        )),
    }
}

fn too_new_major(file: &str, found: u32, supported: u32) -> DkError {
    let message = format!(
        "{file} is schema major {found}, but this build supports up to major {supported}. \
         Upgrade DRAKKAR, or regenerate the file with the current version."
    );
    let context = ErrorContext::new()
        .with_str("file", file)
        .with_int("found_major", i64::from(found))
        .with_int("supported_major", i64::from(supported))
        .with_str("key", "schema")
        .with_str("expected", format!("major <= {supported}"))
        .with_str("value", format!("major {found}"));
    DkError::new(ErrorCode::ConfigInvalidValue, message).with_context(context)
}

fn malformed_schema(tag: &str, surface: Surface<'_>) -> DkError {
    let file = match surface {
        Surface::UserOwned { file } => file,
        Surface::ReconstructibleStore => "payload",
    };
    let message =
        format!("{file} has a malformed schema tag {tag:?}; expected \"drakkar.<name>/<major>\".");
    let context = ErrorContext::new()
        .with_str("file", file)
        .with_str("key", "schema")
        .with_str("expected", "drakkar.<name>/<major>")
        .with_str("value", tag);
    DkError::new(ErrorCode::ConfigInvalidValue, message).with_context(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{SchemaTag, render_schema_tag};
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        name: String,
        value: u32,
    }

    #[test]
    fn parse_and_render_are_inverse() {
        let parsed = SchemaTag::parse("drakkar.fit/1").unwrap();
        assert_eq!(parsed.name, "fit");
        assert_eq!(parsed.major, 1);
        assert_eq!(render_schema_tag("fit", 1), "drakkar.fit/1");
        assert_eq!(
            SchemaTag::parse(&render_schema_tag("manifest", 3))
                .unwrap()
                .major,
            3
        );
    }

    #[test]
    fn parse_rejects_malformed_tags() {
        assert!(SchemaTag::parse("fit/1").is_none()); // missing drakkar. prefix
        assert!(SchemaTag::parse("drakkar.fit").is_none()); // missing major
        assert!(SchemaTag::parse("drakkar./1").is_none()); // empty name
        assert!(SchemaTag::parse("drakkar.fit/x").is_none()); // non-numeric major
    }

    #[test]
    fn read_versioned_ignores_unknown_fields_within_major() {
        let payload = json!({
            "schema": "drakkar.fit/1",
            "name": "qwen3",
            "value": 8,
            "some_future_field": [1, 2, 3]
        });
        let read: VersionedRead<Sample> =
            read_versioned(&payload, 1, Surface::ReconstructibleStore).unwrap();
        assert_eq!(
            read,
            VersionedRead::Value(Sample {
                name: "qwen3".to_owned(),
                value: 8
            })
        );
    }

    #[test]
    fn absent_schema_reads_as_major_1() {
        let payload = json!({ "name": "qwen3", "value": 8 });
        assert_eq!(payload_major(&payload), None);
        let read: VersionedRead<Sample> = read_versioned(
            &payload,
            1,
            Surface::UserOwned {
                file: "config.toml",
            },
        )
        .unwrap();
        assert!(matches!(read, VersionedRead::Value(_)));
    }

    #[test]
    fn too_new_major_on_user_file_is_config_invalid_value() {
        let payload = json!({ "schema": "drakkar.config/2", "name": "x", "value": 1 });
        let err = read_versioned::<Sample>(
            &payload,
            1,
            Surface::UserOwned {
                file: "~/.config/drakkar/config.toml",
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalidValue);
        assert!(err.message.contains("config.toml"));
        assert!(err.message.contains("major 2"));
        assert!(err.message.contains("major 1"));
        assert_eq!(err.context.get("found_major").unwrap().to_display(), "2");
        assert_eq!(
            err.context.get("supported_major").unwrap().to_display(),
            "1"
        );
        assert!(err.remedy().is_some());
    }

    #[test]
    fn too_new_major_on_store_file_signals_regenerate() {
        let payload = json!({ "schema": "drakkar.manifest/2", "name": "x", "value": 1 });
        let read: VersionedRead<Sample> =
            read_versioned(&payload, 1, Surface::ReconstructibleStore).unwrap();
        assert_eq!(
            read,
            VersionedRead::Regenerate {
                found_major: 2,
                supported_major: 1
            }
        );
    }

    #[test]
    fn write_versioned_stamps_the_tag() {
        let sample = Sample {
            name: "qwen3".to_owned(),
            value: 8,
        };
        let json = write_versioned(&sample, SchemaTag::new("drakkar.fit/1")).unwrap();
        assert_eq!(json["schema"], "drakkar.fit/1");
        assert_eq!(json["name"], "qwen3");
        assert_eq!(json["value"], 8);
    }
}
