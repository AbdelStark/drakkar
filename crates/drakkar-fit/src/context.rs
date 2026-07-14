//! The context solver (RFC-0004 §7, FE20).
//!
//! Solves `ctx_max(precision) = solve kv_bytes(ctx) = usable − weights −
//! activation_watermark − fixed_terms` using the architecture-correct
//! `kv_bytes` (FE8–FE12). This is the answer the RAM-range prior art cannot
//! give: *fits at 16k, not at 32k*. The solver inverts the (monotone
//! non-decreasing) KV function by binary search, which handles the uniform,
//! sliding-window (piecewise around `ctx = W`), MLA, and recurrent cases
//! uniformly and correctly.

use crate::kv;
use crate::machine::MachineProfile;
use crate::memory::{self, ACTIVATION_DEFAULT_BYTES, RUNTIME_OVERHEAD_BYTES};
use crate::model::ModelDescriptor;

/// The upper bound of the context search (tokens). Far beyond any advertised
/// context; bounds the binary search.
const CTX_SEARCH_CAP: u32 = 16_777_216;

/// The KV bytes available for the context, per stream: the usable budget less
/// weights and the activation watermark, divided across concurrent streams
/// (the pool is shared). Fixed per-sequence terms (recurrent state) are billed
/// inside `kv_bytes` itself, so they need no separate subtraction here.
fn available_per_stream(
    descriptor: &ModelDescriptor,
    machine: &MachineProfile,
    concurrency: u32,
) -> u64 {
    let usable = memory::usable(machine, RUNTIME_OVERHEAD_BYTES).bytes;
    let weights = memory::weight_bytes(descriptor);
    let available = usable
        .saturating_sub(weights)
        .saturating_sub(ACTIVATION_DEFAULT_BYTES);
    available / u64::from(concurrency.max(1))
}

/// The maximum context (tokens) that fits at KV precision `kv_bits` for
/// `concurrency` concurrent streams (FE20). Returns 0 when even an empty context
/// does not fit (e.g. weights alone exceed the budget).
#[must_use]
pub fn ctx_max(
    descriptor: &ModelDescriptor,
    machine: &MachineProfile,
    kv_bits: u8,
    concurrency: u32,
) -> u64 {
    let budget = available_per_stream(descriptor, machine, concurrency);
    // Largest ctx in [0, CAP] with kv_bytes(ctx) <= budget (kv_bytes is
    // monotone non-decreasing in ctx, so binary search is exact).
    let mut lo: u32 = 0;
    let mut hi: u32 = CTX_SEARCH_CAP;
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        let bytes = kv::kv_bytes(descriptor, mid, kv_bits, memory::KV_GROUP_DEFAULT, false).bytes;
        if bytes <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    u64::from(lo)
}

/// The `ctx_max` ceilings at fp16, 8-bit, and 4-bit KV, side by side (FE20).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ContextCeilings {
    /// Maximum context at fp16 KV.
    pub max_fp16: u64,
    /// Maximum context at 8-bit KV.
    pub max_kv8: u64,
    /// Maximum context at 4-bit KV.
    pub max_kv4: u64,
}

/// Solve the fp16/8-bit/4-bit context ceilings for a plan at `concurrency`
/// (FE20). By construction `max_kv4 >= max_kv8 >= max_fp16`.
#[must_use]
pub fn ctx_ceilings(
    descriptor: &ModelDescriptor,
    machine: &MachineProfile,
    concurrency: u32,
) -> ContextCeilings {
    ContextCeilings {
        max_fp16: ctx_max(descriptor, machine, 16, concurrency),
        max_kv8: ctx_max(descriptor, machine, 8, concurrency),
        max_kv4: ctx_max(descriptor, machine, 4, concurrency),
    }
}

#[cfg(test)]
mod context_solver_tests {
    use super::*;
    use drakkar_core::{BudgetSource, ChipId, LayoutClass, QuantDesc};

    fn model(layout: Vec<LayoutClass>) -> ModelDescriptor {
        ModelDescriptor {
            reference: "test/model".to_owned(),
            arch: "test".to_owned(),
            layers: layout.len() as u32,
            hidden: 4096,
            heads: 32,
            kv_heads: 8,
            head_dim: 128,
            vocab: 150_000,
            params_total: 8_000_000_000,
            params_active: 8_000_000_000,
            moe: None,
            layout_classes: layout,
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

    fn machine(ram_gib: f64, budget_gib: f64) -> MachineProfile {
        MachineProfile {
            chip: ChipId {
                name: "test".to_owned(),
                gpu_cores: 10,
            },
            total_ram_bytes: (ram_gib * memory::BYTES_PER_GIB) as u64,
            budget_bytes: (budget_gib * memory::BYTES_PER_GIB) as u64,
            budget_source: BudgetSource::Probe,
            wired_limit_mb: 0,
            free_bytes: 0,
            macos: (26, 2),
            nax_tensor_ops: false,
            bandwidth_gbs: 273.0,
            ssd_read_gbs: 6.2,
        }
    }

    #[test]
    fn ceilings_are_monotone_in_precision() {
        let d = model(vec![LayoutClass::Global; 36]);
        let m = machine(48.0, 36.0);
        let c = ctx_ceilings(&d, &m, 1);
        assert!(c.max_kv4 >= c.max_kv8);
        assert!(c.max_kv8 >= c.max_fp16);
        assert!(c.max_fp16 > 0);
    }

    #[test]
    fn solved_ceiling_actually_fits_and_next_token_does_not() {
        let d = model(vec![LayoutClass::Global; 36]);
        let m = machine(48.0, 36.0);
        let budget = available_per_stream(&d, &m, 1);
        let max = ctx_max(&d, &m, 16, 1) as u32;
        assert!(kv::kv_bytes(&d, max, 16, memory::KV_GROUP_DEFAULT, false).bytes <= budget);
        assert!(kv::kv_bytes(&d, max + 1, 16, memory::KV_GROUP_DEFAULT, false).bytes > budget);
    }

    #[test]
    fn swa_hybrid_holds_more_context_than_uniform() {
        let m = machine(48.0, 36.0);
        let uniform = model(vec![LayoutClass::Global; 36]);
        let mut swa_layers = vec![LayoutClass::Global; 6];
        swa_layers.extend(vec![
            LayoutClass::SlidingWindow {
                window: 4096,
                sinks: 0
            };
            30
        ]);
        let swa = model(swa_layers);
        assert!(ctx_max(&swa, &m, 16, 1) > ctx_max(&uniform, &m, 16, 1));
    }

    #[test]
    fn concurrency_divides_the_context_ceiling() {
        let d = model(vec![LayoutClass::Global; 36]);
        let m = machine(48.0, 36.0);
        let one = ctx_max(&d, &m, 16, 1);
        let four = ctx_max(&d, &m, 16, 4);
        // Four shared streams get roughly a quarter of the single-stream ceiling.
        assert!(four < one);
        assert!(four >= one / 4 - 2 && four <= one / 4 + 2);
    }

    #[test]
    fn weights_exceeding_budget_yield_zero_context() {
        // A 70B-class weight footprint on a 16 GB machine: no room for any KV.
        let mut d = model(vec![LayoutClass::Global; 80]);
        d.params_total = 70_000_000_000;
        let m = machine(16.0, 11.0);
        assert_eq!(ctx_max(&d, &m, 16, 1), 0);
    }
}
