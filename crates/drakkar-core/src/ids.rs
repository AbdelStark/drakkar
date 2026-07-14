//! Identifier and hash newtypes (data-model §2.1–§2.2).
//!
//! These are the load-bearing identifier types the rest of the workspace shares.
//! Content digests render as `sha256-<64 hex>` (DM5); schema tags parse as
//! `name + '/' + major` (DM7).

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

/// A SHA-256 content digest. Renders as `sha256-<64 lowercase hex>` (DM5); blob
/// file names in the content-addressed store are exactly this rendering
/// (RFC-0006 MP10).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256(pub [u8; 32]);

impl Sha256 {
    /// The 64-character lowercase hex body, without the `sha256-` prefix.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for byte in self.0 {
            // Two lowercase hex digits per byte.
            s.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
            s.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
        }
        s
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sha256-{}", self.to_hex())
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256({self})")
    }
}

/// Error returned when a string is not a valid `sha256-<64 hex>` rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSha256Error(pub String);

impl fmt::Display for ParseSha256Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid sha256 digest: {}", self.0)
    }
}

impl std::error::Error for ParseSha256Error {}

impl FromStr for Sha256 {
    type Err = ParseSha256Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s
            .strip_prefix("sha256-")
            .ok_or_else(|| ParseSha256Error(s.to_owned()))?;
        if hex.len() != 64 {
            return Err(ParseSha256Error(s.to_owned()));
        }
        let mut out = [0u8; 32];
        let bytes = hex.as_bytes();
        for (i, chunk) in bytes.chunks_exact(2).enumerate() {
            let hi = (chunk[0] as char)
                .to_digit(16)
                .ok_or_else(|| ParseSha256Error(s.to_owned()))?;
            let lo = (chunk[1] as char)
                .to_digit(16)
                .ok_or_else(|| ParseSha256Error(s.to_owned()))?;
            out[i] = ((hi << 4) | lo) as u8;
        }
        Ok(Sha256(out))
    }
}

impl Serialize for Sha256 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Sha256 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

/// A ULID value (128 bits), rendered as its canonical 26-character Crockford
/// base32 string. Generation from a timestamp + entropy is a later concern; this
/// crate fixes the type and its lexical form.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ulid(pub u128);

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 26 base-32 symbols, most-significant first; the top symbol carries the
        // high 2 bits (so it is always 0..=7), matching the ULID text encoding.
        let mut buf = [0u8; 26];
        let mut v = self.0;
        for slot in buf.iter_mut().rev() {
            *slot = CROCKFORD[(v & 0x1f) as usize];
            v >>= 5;
        }
        // SAFETY-free: every byte is an ASCII symbol from CROCKFORD.
        f.write_str(std::str::from_utf8(&buf).expect("crockford symbols are ascii"))
    }
}

impl fmt::Debug for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ulid({self})")
    }
}

/// Error returned when a string is not a valid 26-character Crockford ULID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseUlidError(pub String);

impl fmt::Display for ParseUlidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid ulid: {}", self.0)
    }
}

impl std::error::Error for ParseUlidError {}

impl FromStr for Ulid {
    type Err = ParseUlidError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 26 {
            return Err(ParseUlidError(s.to_owned()));
        }
        let mut v: u128 = 0;
        for ch in s.bytes() {
            let up = ch.to_ascii_uppercase();
            let digit = CROCKFORD
                .iter()
                .position(|&c| c == up)
                .ok_or_else(|| ParseUlidError(s.to_owned()))?;
            v = (v << 5) | digit as u128;
        }
        Ok(Ulid(v))
    }
}

/// A request identifier, unique per process lifetime; appears in logs and traces
/// (RFC-0007 AS22).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct RequestId(pub Ulid);

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Ulid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Ulid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

/// A process-local sequence id, monotonically assigned per admitted sequence.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SeqId(pub u64);

/// An index into the physical block pool of one engine.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockId(pub u32);

/// A tokenizer vocabulary id.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TokenId(pub u32);

/// A schema tag of the form `drakkar.<name>/<major>` (DM7). This type declares
/// the newtype and its lexical accessors; the version-gate reader/writer helper
/// (reject a newer major, migrate on bump) is a separate concern (issue #125).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SchemaTag(pub &'static str);

impl SchemaTag {
    /// Construct a schema tag from a static string literal such as
    /// `"drakkar.fit/1"`.
    #[must_use]
    pub const fn new(tag: &'static str) -> Self {
        SchemaTag(tag)
    }

    /// The `name` portion (everything before the final `/`), e.g.
    /// `"drakkar.fit"` for `"drakkar.fit/1"`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self.0.rsplit_once('/') {
            Some((name, _major)) => name,
            None => self.0,
        }
    }

    /// The `major` portion parsed as an integer, e.g. `1` for `"drakkar.fit/1"`.
    /// Returns `None` if the tag has no `/major` suffix or it does not parse.
    #[must_use]
    pub fn major(&self) -> Option<u32> {
        self.0.rsplit_once('/').and_then(|(_, m)| m.parse().ok())
    }
}

impl fmt::Display for SchemaTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl fmt::Debug for SchemaTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SchemaTag({:?})", self.0)
    }
}

impl Serialize for SchemaTag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

/// A BLAKE3 prefix hash at block granularity (DM6). One entry per full block of
/// a prompt; the chain seed folds in every KV12 correctness key so any change to
/// model revision, tokenizer, template, KV precision, or rope scaling
/// invalidates the chain by construction.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrefixHash(pub [u8; 32]);

impl PrefixHash {
    /// The 64-character lowercase hex rendering.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for byte in self.0 {
            s.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
            s.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
        }
        s
    }
}

impl fmt::Display for PrefixHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for PrefixHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PrefixHash({self})")
    }
}

/// The ordered prefix hash chain for a prompt: one [`PrefixHash`] per full block.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct PrefixHashChain(pub Vec<PrefixHash>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_renders_with_prefix_and_round_trips() {
        let mut bytes = [0u8; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = i as u8;
        }
        let digest = Sha256(bytes);
        let rendered = digest.to_string();
        assert!(rendered.starts_with("sha256-"));
        assert_eq!(rendered.len(), "sha256-".len() + 64);
        assert_eq!(rendered.parse::<Sha256>().unwrap(), digest);
    }

    #[test]
    fn sha256_rejects_bad_input() {
        assert!("deadbeef".parse::<Sha256>().is_err());
        assert!("sha256-xyz".parse::<Sha256>().is_err());
        assert!("sha256-00".parse::<Sha256>().is_err());
    }

    #[test]
    fn schema_tag_parses_name_and_major() {
        let tag = SchemaTag::new("drakkar.fit/1");
        assert_eq!(tag.name(), "drakkar.fit");
        assert_eq!(tag.major(), Some(1));
        assert_eq!(tag.to_string(), "drakkar.fit/1");
    }

    #[test]
    fn ulid_round_trips_through_crockford() {
        for v in [0u128, 1, 0x0123_4567_89ab_cdef, u128::MAX >> 2] {
            let ulid = Ulid(v);
            let s = ulid.to_string();
            assert_eq!(s.len(), 26);
            assert_eq!(s.parse::<Ulid>().unwrap(), ulid);
        }
    }
}
