//! The feasibility report (`drakkar.fit/1`, data-model §3.5, RFC-0004 FE26).
//!
//! One struct renders both the human report card and `--json` (RFC-0008 CLI6);
//! `POST /fit` returns the same body (AS20). [`FitReport`] serializes to exactly
//! the FE26 JSON shape — field names, nesting, units — verified by a verbatim
//! golden round-trip (INV-MIRROR).
//!
//! Every memory or performance figure carried here is computed by `drakkar-fit`
//! (INV-ONE-TRUTH); this crate only fixes the shape, never the arithmetic.

use serde::{Deserialize, Serialize};

use crate::artifact::QuantDesc;
use crate::ids::SchemaTag;

/// The schema tag every [`FitReport`] carries.
pub const FIT_SCHEMA: SchemaTag = SchemaTag::new("drakkar.fit/1");

/// The confidence tier printed with every predicted number (FE24).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Directly measured on this machine.
    Measured,
    /// Derived from a calibration file for this chip (FE4).
    Calibrated,
    /// Produced from shipped closed-form constants.
    Modeled,
}

/// A predicted value with its confidence tier.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Estimate<T> {
    /// The predicted value.
    pub value: T,
    /// How the value was obtained.
    pub confidence: Confidence,
}

/// A time-to-first-token estimate, which additionally records the prompt length
/// the estimate assumes.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct TtftEstimate {
    /// Predicted seconds to first token.
    pub value: f64,
    /// Prompt length (tokens) the estimate assumes.
    pub prompt: u32,
    /// How the value was obtained.
    pub confidence: Confidence,
}

/// Where the GPU memory budget came from (FE15/FE2).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetSource {
    /// Live Metal probe of the recommended working set.
    Probe,
    /// The shipped per-chip fallback table (offline `--machine`).
    Table,
}

/// The overall feasibility verdict (FE19).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Fits with generous headroom.
    Comfortable,
    /// Fits, but with little headroom.
    Tight,
    /// Fits only after applying a remedy.
    NeedsTuning,
    /// Does not fit at the requested configuration.
    WontFit,
}

/// The model facet of the report.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FitModel {
    /// Model reference id, e.g. `"Qwen/Qwen3-8B"`.
    pub id: String,
    /// Architecture family, e.g. `"qwen3"`.
    pub arch: String,
    /// Total parameters.
    pub params_total: f64,
    /// Active parameters per token.
    pub params_active: f64,
    /// Quantization descriptor.
    pub quant: QuantDesc,
}

/// The machine facet of the report.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FitMachine {
    /// Chip marketing name.
    pub chip: String,
    /// Unified memory (GiB).
    pub ram_gib: f64,
    /// GPU memory budget (GiB).
    pub budget_gib: f64,
    /// Whether the budget was probed or read from the table.
    pub budget_source: BudgetSource,
    /// Memory bandwidth (GB/s).
    pub bandwidth_gbs: f32,
    /// Whether Neural Accelerator tensor ops are available.
    pub nax: bool,
    /// Current `iogpu.wired_limit_mb` (0 when unset).
    pub wired_limit_mb: u32,
}

/// The memory decomposition (all GiB except per-token KV in KiB, DM4).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct FitMemory {
    /// Weights (GiB).
    pub weights_gib: f64,
    /// Per-token KV footprint (KiB).
    pub kv_per_token_kib: f64,
    /// KV footprint at the requested context (GiB).
    pub kv_at_ctx_gib: f64,
    /// Activation high-water mark (GiB).
    pub activation_gib: f64,
    /// Runtime overhead (GiB).
    pub runtime_gib: f64,
    /// Total footprint (GiB).
    pub total_gib: f64,
    /// Confidence in the memory figures.
    pub confidence: Confidence,
}

/// The context-ceiling solver output per KV precision (FE20).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct FitContext {
    /// The requested context length.
    pub requested: u32,
    /// Maximum context at fp16 KV.
    pub max_fp16: u32,
    /// Maximum context at 8-bit KV.
    pub max_kv8: u32,
    /// Maximum context at 4-bit KV, when solved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_kv4: Option<u32>,
    /// The model's advertised context length.
    pub advertised: u32,
}

/// The performance facet of the report.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct FitPerformance {
    /// Decode throughput (tokens/s).
    pub decode_tps: Estimate<f64>,
    /// Cold time-to-first-token (seconds).
    pub ttft_cold_s: TtftEstimate,
    /// Model load time (seconds).
    pub load_s: f64,
}

/// The kind of a ranked remedy (FE19 order).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemedyKind {
    /// Use an official pre-quantized artifact.
    OfficialQuant,
    /// Quantize on device to the store.
    OnDeviceQuant,
    /// Use 8-bit KV.
    Kv8Bit,
    /// Reduce the requested context.
    ReducedContext,
    /// Use sub-8-bit KV.
    KvSubByte,
    /// Raise the wired memory limit (opt-in).
    WiredLimitRaise,
}

/// A ranked remedy carrying the exact command or flag to apply it (FE19). This
/// is the *fit-report* remedy (rank/kind/command/effect); it is distinct from
/// the error-taxonomy remedy of the `DkError` type.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Remedy {
    /// Rank in the FE19 order (1 = most preferred).
    pub rank: u8,
    /// The kind of remedy.
    pub kind: RemedyKind,
    /// A copy-pasteable command, e.g. `"drakkar pull qwen3:8b --quant 4bit-g64"`.
    pub command: String,
    /// The predicted outcome in domain terms.
    pub effect: String,
}

/// The feasibility report: mirror of `drakkar.fit/1` (FE26). Serializes to
/// exactly that JSON shape (INV-MIRROR).
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct FitReport {
    /// Schema tag (`drakkar.fit/1`).
    pub schema: SchemaTag,
    /// The model facet.
    pub model: FitModel,
    /// The machine facet.
    pub machine: FitMachine,
    /// The memory decomposition.
    pub memory: FitMemory,
    /// The overall verdict.
    pub verdict: Verdict,
    /// Headroom under the budget (GiB).
    pub headroom_gib: f64,
    /// The context-ceiling solver output.
    pub context: FitContext,
    /// The performance facet.
    pub performance: FitPerformance,
    /// Ranked remedies (empty when none are needed).
    pub remedies: Vec<Remedy>,
}
