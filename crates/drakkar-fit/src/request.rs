//! The request-shape input (RFC-0004 FE3).

/// The requested plan: target context, concurrency, KV precision, and
/// draft-model choice (FE3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RequestShape {
    /// Target context length (prompt + generation).
    pub target_ctx: u32,
    /// Concurrent sequences to plan the KV pool for.
    pub concurrency: u32,
    /// KV precision in bits: 16 (fp16), 8, or 4 (KV13).
    pub kv_bits: u8,
    /// Whether a draft model is used for speculation.
    pub draft_model: bool,
}

/// The FE3 defaults: the model's advertised context capped at 32k for the
/// preflight display, concurrency 1, KV fp16. (The actual context is
/// `min(advertised, 32768)`; the cap is applied against the model in
/// [`crate::fit`].)
impl Default for RequestShape {
    fn default() -> Self {
        RequestShape {
            target_ctx: 32_768,
            concurrency: 1,
            kv_bits: 16,
            draft_model: false,
        }
    }
}

impl RequestShape {
    /// Bytes per KV element for the configured precision: 2 (fp16), 1 (8-bit),
    /// 0.5 (4-bit) (FE8).
    #[must_use]
    pub fn kv_bytes_per_element(&self) -> f64 {
        f64::from(self.kv_bits) / 8.0
    }
}
