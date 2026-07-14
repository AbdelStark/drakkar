//! Runtime capability reporting (data-model §3.4, RFC-0001 A7).
//!
//! [`Capabilities`] is filled by the load-time probe (IC26) and is the only
//! sanctioned way a feature learns whether it may run (DM16). Feature code MUST
//! NOT probe hardware or macOS versions itself.

use serde::{Deserialize, Serialize};

/// Chip identity and GPU core count, from IOKit/sysctl.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ChipId {
    /// Marketing name, e.g. `"Apple M4 Pro"`.
    pub name: String,
    /// Number of GPU cores.
    pub gpu_cores: u32,
}

/// Which speculative-decoding paths the backend supports (IC18/IC19).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct SpecDecodeSupport {
    /// Prompt-lookup / n-gram speculation (IC18).
    pub ngram: bool,
    /// Draft-model speculation (IC19).
    pub draft: bool,
}

/// The paged-attention execution path the backend offers (IC10, LD20).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PagedPath {
    /// Gather blocks into a contiguous buffer, then run dense attention.
    GatherFallback,
    /// A fused variable-length paged-attention kernel.
    FusedVarlen,
}

/// The capability set reported by the load-time probe, gating features at
/// runtime (RFC-0001 A7). No caller may assume a capability the probe did not
/// report.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    /// Chip identity and GPU core count.
    pub chip: ChipId,
    /// Memory bandwidth (GB/s), probed or from the FE2 fallback table.
    pub bandwidth_gbs: f32,
    /// macOS version as `(major, minor)`.
    pub macos: (u16, u16),
    /// Whether Neural Accelerator tensor ops passed the functional self-test
    /// (never version sniffing, IC26).
    pub nax_tensor_ops: bool,
    /// Supported KV precisions, e.g. `[16, 8, 4]` (KV13).
    pub kv_bits: Vec<u8>,
    /// Speculative-decoding support.
    pub spec_decode: SpecDecodeSupport,
    /// The paged-attention path (IC10, LD20).
    pub paged_attention: PagedPath,
    /// The largest decode batch the backend accepts.
    pub max_batch: u32,
}
