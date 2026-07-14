//! Structural secret redaction (security §4, SEC27, RFC-0001 A12).
//!
//! Redaction is structural, not best-effort: HF tokens and server API keys are
//! wrapped in [`Secret`], whose `Debug`, `Display`, and `serde::Serialize`
//! implementations all emit `[redacted]` and never the underlying bytes. The
//! raw value is reachable only through the explicit [`Secret::expose`] /
//! [`Secret::into_inner`] accessors, which exist so that raw secret bytes appear
//! only at the two sanctioned sites: HTTP `Authorization`-header construction and
//! the constant-time API-key compare.
//!
//! There is deliberately **no** `Deref` to the inner value, so a secret can
//! never be printed, formatted, or serialized by accident. Because structured
//! logging encodes fields through `Display` (`%value`) or `Debug` (`?value`),
//! both of which redact, a `Secret` is safe in any tracing sink without special
//! handling.

use std::fmt;
use std::marker::PhantomData;

use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};

/// The placeholder rendered in every non-exposing view of a [`Secret`].
pub const REDACTED: &str = "[redacted]";

/// A value whose contents are redacted from every debug, display, and
/// serialization view (SEC27). Construct with [`Secret::new`]; reach the raw
/// value only through [`Secret::expose`] or [`Secret::into_inner`].
#[derive(Clone)]
pub struct Secret<T> {
    inner: T,
    // Keeps the API future-proof for non-`String` secrets without changing the
    // public shape.
    _marker: PhantomData<()>,
}

impl<T> Secret<T> {
    /// Wrap `value` as a secret.
    pub fn new(value: T) -> Self {
        Secret {
            inner: value,
            _marker: PhantomData,
        }
    }

    /// Borrow the raw secret. Use only at a sanctioned exposure site
    /// (`Authorization`-header construction, constant-time compare).
    pub fn expose(&self) -> &T {
        &self.inner
    }

    /// Consume the secret and return the raw value.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> From<T> for Secret<T> {
    fn from(value: T) -> Self {
        Secret::new(value)
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl<T> Serialize for Secret<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Secret<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Deserialization is a construction site (loading config or a token), so
        // it reads the raw value; serialization is the redacting direction.
        T::deserialize(deserializer).map(Secret::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    const TOKEN: &str = "hf_supersecrettoken_deadbeef";

    #[test]
    fn debug_and_display_redact() {
        let secret = Secret::new(TOKEN.to_owned());
        let debug = format!("{secret:?}");
        let display = format!("{secret}");
        assert_eq!(debug, REDACTED);
        assert_eq!(display, REDACTED);
        assert!(!debug.contains("hf_"));
        assert!(!display.contains("hf_"));
        assert!(!debug.contains(TOKEN));
        assert!(!display.contains(TOKEN));
    }

    #[test]
    fn expose_returns_the_raw_value() {
        let secret = Secret::new(TOKEN.to_owned());
        assert_eq!(secret.expose(), TOKEN);
        assert_eq!(Secret::new(TOKEN.to_owned()).into_inner(), TOKEN);
    }

    #[test]
    fn serialize_redacts_directly() {
        let secret = Secret::new(TOKEN.to_owned());
        let json = serde_json::to_string(&secret).unwrap();
        assert_eq!(json, format!("\"{REDACTED}\""));
        assert!(!json.contains(TOKEN));
    }

    #[test]
    fn serialize_redacts_an_embedded_field() {
        #[derive(Serialize)]
        struct Config {
            host: String,
            api_key: Secret<String>,
        }
        let cfg = Config {
            host: "127.0.0.1".to_owned(),
            api_key: Secret::new(TOKEN.to_owned()),
        };
        let value = serde_json::to_value(&cfg).unwrap();
        assert_eq!(value["api_key"], REDACTED);
        assert!(!serde_json::to_string(&cfg).unwrap().contains(TOKEN));
    }

    #[test]
    fn deserialize_reads_the_raw_value() {
        // A construction site (config load) reads the real bytes.
        let secret: Secret<String> = serde_json::from_str(&format!("\"{TOKEN}\"")).unwrap();
        assert_eq!(secret.expose(), TOKEN);
    }
}
