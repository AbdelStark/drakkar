//! Performance prediction (RFC-0004 §8, FE21–FE24).
//!
//! Decode is a bandwidth roofline (FE21), prefill is anchored with a NAX
//! multiplier (FE22), TTFT is prefill plus a fixed constant (FE23), and every
//! prediction carries a confidence tier (FE24). This module ships the *modeled*
//! path on shipped constants; reading the calibration store and flipping to
//! `calibrated` is a separate v0.2 concern (#234). At this milestone every
//! prediction is `modeled` (equivalently, `est.`).

use drakkar_core::{Confidence, Estimate, FitPerformance, TtftEstimate};

use crate::kv;
use crate::machine::MachineProfile;
use crate::memory::{self, bpw_eff};
use crate::model::ModelDescriptor;

/// Shipped decode kernel efficiency (FE21); calibrated per chip/model-class in
/// v0.2 (observed range 0.6–0.85).
pub const ETA_D: f64 = 0.65;

/// The reference active-parameter count the shipped prefill anchor is stated
/// for (a dense-8B-class model).
const REFERENCE_ACTIVE_PARAMS: f64 = 8.0e9;

/// Shipped prefill throughput anchor (tokens/s) for the reference model at 4k
/// prompt **without** the Neural Accelerator (FE22). Scaled by the
/// active-parameter ratio and the NAX multiplier. `est.` until calibrated.
pub const PREFILL_ANCHOR_BASE_TPS: f64 = 410.0;

/// The NAX (Metal 4 tensor-op) prefill multiplier, applied only when the
/// self-test passes (FE22; the observed M5 uplift is 3.3–4.1x).
pub const NAX_MULTIPLIER: f64 = 3.4;

/// Fixed cold-start constant added to prefill time for TTFT (FE23), in seconds.
pub const TTFT_COLD_CONST_S: f64 = 0.15;

/// Fixed warm constant for prefix-cached TTFT (FE23), in seconds.
pub const TTFT_WARM_CONST_S: f64 = 0.05;

/// The prompt length the report's TTFT estimate assumes (FE26 example prompt).
pub const REPORT_PROMPT_TOKENS: u32 = 4096;

fn bytes_per_second(bandwidth_gbs: f32) -> f64 {
    f64::from(bandwidth_gbs) * 1.0e9
}

/// Active weight bytes (MoE bills active params; dense equals total) at the
/// model's quantization.
fn active_weight_bytes(descriptor: &ModelDescriptor) -> f64 {
    let bpw = bpw_eff(
        &descriptor.quant.scheme,
        descriptor.quant.bits,
        descriptor.quant.group,
    );
    descriptor.params_active as f64 * bpw / 8.0
}

/// Decode throughput (tokens/s) at context `ctx` (FE21): the bandwidth roofline
/// `eta_d * BW / (active_weight_bytes + kv_read_bytes(ctx))`. Monotone
/// non-increasing in `ctx` because `kv_read_bytes` grows with context.
#[must_use]
pub fn decode_tps(
    descriptor: &ModelDescriptor,
    machine: &MachineProfile,
    ctx: u32,
    kv_bits: u8,
) -> f64 {
    let kv_read =
        kv::kv_bytes(descriptor, ctx, kv_bits, memory::KV_GROUP_DEFAULT, false).bytes as f64;
    let denom = active_weight_bytes(descriptor) + kv_read;
    if denom <= 0.0 {
        return 0.0;
    }
    ETA_D * bytes_per_second(machine.bandwidth_gbs) / denom
}

/// Prefill throughput (tokens/s) (FE22): the shipped anchor scaled by the
/// active-parameter ratio, with the NAX multiplier applied **only** when the
/// tensor-op self-test passed.
#[must_use]
pub fn prefill_tps(descriptor: &ModelDescriptor, machine: &MachineProfile) -> f64 {
    let active = (descriptor.params_active as f64).max(1.0);
    let scaled = PREFILL_ANCHOR_BASE_TPS * (REFERENCE_ACTIVE_PARAMS / active);
    if machine.nax_tensor_ops {
        scaled * NAX_MULTIPLIER
    } else {
        scaled
    }
}

/// Cold time-to-first-token (seconds) for a `prompt_tokens` prompt (FE23):
/// `prompt / prefill_tps + c0`.
#[must_use]
pub fn ttft_cold_s(prompt_tokens: u32, prefill_tps: f64) -> f64 {
    if prefill_tps <= 0.0 {
        return f64::INFINITY;
    }
    f64::from(prompt_tokens) / prefill_tps + TTFT_COLD_CONST_S
}

/// Warm time-to-first-token (seconds) for an `uncached_suffix` (FE23):
/// `uncached_suffix / prefill_tps + c1`.
#[must_use]
pub fn ttft_warm_s(uncached_suffix: u32, prefill_tps: f64) -> f64 {
    if prefill_tps <= 0.0 {
        return f64::INFINITY;
    }
    f64::from(uncached_suffix) / prefill_tps + TTFT_WARM_CONST_S
}

/// Load-from-disk time (seconds), reported separately from TTFT (FE23):
/// `weights_bytes / ssd_bw`.
#[must_use]
pub fn load_time_s(weights_bytes: u64, ssd_read_gbs: f32) -> f64 {
    let bw = bytes_per_second(ssd_read_gbs);
    if bw <= 0.0 {
        return f64::INFINITY;
    }
    weights_bytes as f64 / bw
}

/// Assemble the FE26 `performance` sub-struct for a plan. Every value carries
/// `modeled` confidence at this milestone; the prompt the TTFT estimate assumes
/// is [`REPORT_PROMPT_TOKENS`].
#[must_use]
pub fn performance(
    descriptor: &ModelDescriptor,
    machine: &MachineProfile,
    ctx: u32,
    kv_bits: u8,
) -> FitPerformance {
    let prefill = prefill_tps(descriptor, machine);
    FitPerformance {
        decode_tps: Estimate {
            value: decode_tps(descriptor, machine, ctx, kv_bits),
            confidence: Confidence::Modeled,
        },
        ttft_cold_s: TtftEstimate {
            value: ttft_cold_s(REPORT_PROMPT_TOKENS, prefill),
            prompt: REPORT_PROMPT_TOKENS,
            confidence: Confidence::Modeled,
        },
        load_s: load_time_s(memory::weight_bytes(descriptor), machine.ssd_read_gbs),
    }
}

#[cfg(test)]
mod perf_prediction_tests {
    use super::*;
    use drakkar_core::{BudgetSource, ChipId, LayoutClass, QuantDesc};

    fn model(active: u64) -> ModelDescriptor {
        ModelDescriptor {
            reference: "test/model".to_owned(),
            arch: "test".to_owned(),
            layers: 36,
            hidden: 4096,
            heads: 32,
            kv_heads: 8,
            head_dim: 128,
            vocab: 150_000,
            params_total: 8_000_000_000,
            params_active: active,
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
            repo_total_bytes: 0,
        }
    }

    fn machine(nax: bool) -> MachineProfile {
        MachineProfile {
            chip: ChipId {
                name: "test".to_owned(),
                gpu_cores: 20,
            },
            total_ram_bytes: 48 * 1024 * 1024 * 1024,
            budget_bytes: 36 * 1024 * 1024 * 1024,
            budget_source: BudgetSource::Probe,
            wired_limit_mb: 0,
            free_bytes: 0,
            macos: (26, 2),
            nax_tensor_ops: nax,
            bandwidth_gbs: 273.0,
            ssd_read_gbs: 6.2,
        }
    }

    #[test]
    fn decode_tps_is_monotone_non_increasing_in_ctx() {
        let d = model(8_000_000_000);
        let m = machine(false);
        let short = decode_tps(&d, &m, 1000, 16);
        let long = decode_tps(&d, &m, 64_000, 16);
        assert!(short > 0.0);
        assert!(long <= short); // throughput falls as context grows (FE21)
    }

    #[test]
    fn nax_gated() {
        let d = model(8_000_000_000);
        let without = prefill_tps(&d, &machine(false));
        let with = prefill_tps(&d, &machine(true));
        assert!((with - without * NAX_MULTIPLIER).abs() < 1e-6);
        assert!(with > without);
    }

    #[test]
    fn prefill_scales_with_active_params() {
        // A model with half the active params prefills ~twice as fast.
        let big = prefill_tps(&model(8_000_000_000), &machine(false));
        let small = prefill_tps(&model(4_000_000_000), &machine(false));
        assert!((small - big * 2.0).abs() < 1e-6);
    }

    #[test]
    fn ttft_and_load_time() {
        let d = model(8_000_000_000);
        let m = machine(false);
        let pf = prefill_tps(&d, &m);
        let cold = ttft_cold_s(4096, pf);
        assert!((cold - (4096.0 / pf + TTFT_COLD_CONST_S)).abs() < 1e-9);
        let warm = ttft_warm_s(512, pf);
        assert!(warm < cold); // fewer uncached tokens on a warm start
        // Load time is a separate weights/ssd_bw term.
        let load = load_time_s(memory::weight_bytes(&d), m.ssd_read_gbs);
        assert!(load > 0.0);
    }

    #[test]
    fn performance_struct_is_all_modeled() {
        let d = model(8_000_000_000);
        let p = performance(&d, &machine(false), 32_768, 16);
        assert_eq!(p.decode_tps.confidence, Confidence::Modeled);
        assert_eq!(p.ttft_cold_s.confidence, Confidence::Modeled);
        assert_eq!(p.ttft_cold_s.prompt, REPORT_PROMPT_TOKENS);
        assert!(p.decode_tps.value > 0.0);
        assert!(p.load_s > 0.0);
    }
}
