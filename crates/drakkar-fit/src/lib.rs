//! `drakkar-fit` — the feasibility engine ([RFC-0004]).
//!
//! A **pure library** (layer 0): given a model descriptor, a machine profile,
//! and a request shape it computes whether the model fits, at what context, and
//! how fast — deterministically and offline, with no network or disk I/O. It is
//! the single source of truth for memory math (invariant I3), serving CLI
//! preflight, the `/fit` endpoint, and scheduler admission from one
//! implementation.
//!
//! This module establishes the crate: the input types ([`ModelDescriptor`],
//! [`MachineProfile`], [`RequestShape`]), the confidence tier alias
//! ([`ConfidenceTier`]), and the entry [`fit`] signature. The report layout is
//! `drakkar-core`'s [`FitReport`] (the FE26 `drakkar.fit/1` mirror); the memory,
//! verdict, context, and performance arithmetic that fills it lands in the
//! feasibility issues (#225–#232).
//!
//! [RFC-0004]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0004-feasibility-engine.md
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod machine;
pub mod memory;
mod model;
mod request;

pub use machine::MachineProfile;
pub use model::{ModelDescriptor, TensorEntry};
pub use request::RequestShape;

// The report layout is `drakkar-core`'s FE26 mirror; re-exported so consumers
// name `drakkar_fit::FitReport` without reaching into core (DM1: one definition).
pub use drakkar_core::FitReport;

use drakkar_core::{
    Confidence, Estimate, FIT_SCHEMA, FitContext, FitMachine, FitMemory, FitModel, FitPerformance,
    TtftEstimate, Verdict,
};

/// The FE24 confidence tier printed with every prediction. An alias for
/// `drakkar-core`'s `Confidence` (`measured` / `calibrated` / `modeled`), which
/// is the single definition of the tier used across the report.
pub type ConfidenceTier = Confidence;

const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / BYTES_PER_GIB
}

/// Compute the feasibility report for a model on a machine under a request
/// shape (RFC-0004). Deterministic and I/O-free: identical inputs always yield
/// an identical report.
///
/// This fills the descriptive facets (model, machine, context) from the inputs
/// and the memory decomposition and headroom from the memory model (#227). The
/// verdict and remedies (#230), context ceilings (#229), and performance
/// estimates (#231) are populated by the remaining feasibility issues; until
/// then those fields are placeholders carrying `modeled` confidence.
#[must_use]
pub fn fit(model: &ModelDescriptor, machine: &MachineProfile, request: &RequestShape) -> FitReport {
    let mem = memory::total(
        model,
        request.target_ctx,
        request.concurrency,
        request.kv_bits,
    );
    let kv_per_token = memory::kv_bytes_per_token(model, request.kv_bits, memory::KV_GROUP_DEFAULT);
    let headroom_gib = (machine.budget_bytes as f64 - mem.total as f64) / memory::BYTES_PER_GIB;
    FitReport {
        schema: FIT_SCHEMA,
        model: FitModel {
            id: model.reference.clone(),
            arch: model.arch.clone(),
            params_total: model.params_total as f64,
            params_active: model.params_active as f64,
            quant: model.quant.clone(),
        },
        machine: FitMachine {
            chip: machine.chip.name.clone(),
            ram_gib: bytes_to_gib(machine.total_ram_bytes),
            budget_gib: bytes_to_gib(machine.budget_bytes),
            budget_source: machine.budget_source,
            bandwidth_gbs: machine.bandwidth_gbs,
            nax: machine.nax_tensor_ops,
            wired_limit_mb: machine.wired_limit_mb,
        },
        memory: FitMemory {
            weights_gib: mem.weights as f64 / memory::BYTES_PER_GIB,
            kv_per_token_kib: kv_per_token / 1024.0,
            kv_at_ctx_gib: mem.kv_pool as f64 / memory::BYTES_PER_GIB,
            activation_gib: mem.activation_watermark as f64 / memory::BYTES_PER_GIB,
            runtime_gib: mem.runtime_overhead as f64 / memory::BYTES_PER_GIB,
            total_gib: mem.total as f64 / memory::BYTES_PER_GIB,
            confidence: Confidence::Modeled,
        },
        // Placeholder — the verdict tiers and remedies (#230) fill these.
        verdict: Verdict::WontFit,
        headroom_gib,
        context: FitContext {
            requested: request.target_ctx,
            max_fp16: 0,
            max_kv8: 0,
            max_kv4: None,
            advertised: model.advertised_ctx,
        },
        // Placeholder — the performance model (#231) fills these.
        performance: FitPerformance {
            decode_tps: Estimate {
                value: 0.0,
                confidence: Confidence::Modeled,
            },
            ttft_cold_s: TtftEstimate {
                value: 0.0,
                prompt: 0,
                confidence: Confidence::Modeled,
            },
            load_s: 0.0,
        },
        remedies: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drakkar_core::{BudgetSource, ChipId, QuantDesc};

    fn sample_model() -> ModelDescriptor {
        ModelDescriptor {
            reference: "Qwen/Qwen3-8B".to_owned(),
            arch: "qwen3".to_owned(),
            layers: 36,
            hidden: 4096,
            heads: 32,
            kv_heads: 8,
            head_dim: 128,
            vocab: 151_936,
            params_total: 8_190_000_000,
            params_active: 8_190_000_000,
            moe: None,
            layout_classes: vec![drakkar_core::LayoutClass::Global; 36],
            quant: QuantDesc {
                scheme: "mlx_affine".to_owned(),
                bits: 4,
                group: 64,
                bpw_eff: 4.5,
                recipe: None,
            },
            advertised_ctx: 131_072,
            tensors: Vec::new(),
            repo_total_bytes: 4_600_000_000,
        }
    }

    fn sample_machine() -> MachineProfile {
        MachineProfile {
            chip: ChipId {
                name: "Apple M4 Pro".to_owned(),
                gpu_cores: 20,
            },
            total_ram_bytes: 48 * 1024 * 1024 * 1024,
            budget_bytes: 36 * 1024 * 1024 * 1024,
            budget_source: BudgetSource::Probe,
            wired_limit_mb: 0,
            free_bytes: 30 * 1024 * 1024 * 1024,
            macos: (26, 2),
            nax_tensor_ops: false,
            bandwidth_gbs: 273.0,
            ssd_read_gbs: 6.2,
        }
    }

    #[test]
    fn request_defaults() {
        let r = RequestShape::default();
        assert_eq!(r.concurrency, 1);
        assert_eq!(r.kv_bits, 16); // KV fp16
        assert_eq!(r.target_ctx, 32_768); // display cap 32k
        assert!(!r.draft_model);
    }

    #[test]
    fn fit_fills_descriptive_facets_from_inputs() {
        let report = fit(&sample_model(), &sample_machine(), &RequestShape::default());
        assert_eq!(report.model.id, "Qwen/Qwen3-8B");
        assert_eq!(report.model.arch, "qwen3");
        assert_eq!(report.machine.chip, "Apple M4 Pro");
        assert!((report.machine.ram_gib - 48.0).abs() < 1e-6);
        assert!((report.machine.budget_gib - 36.0).abs() < 1e-6);
        assert_eq!(report.context.requested, 32_768);
        assert_eq!(report.context.advertised, 131_072);
        // Memory facet is populated by the memory model (#227).
        assert!(report.memory.weights_gib > 0.0);
        assert!((report.memory.kv_per_token_kib - 144.0).abs() < 1e-3); // Qwen3-8B: 144 KiB/t
        assert!(report.memory.total_gib > report.memory.weights_gib);
    }

    #[test]
    fn fit_is_deterministic() {
        let (m, h, r) = (sample_model(), sample_machine(), RequestShape::default());
        assert_eq!(fit(&m, &h, &r), fit(&m, &h, &r));
    }

    #[test]
    fn exact_weight_bytes_sums_the_index() {
        let mut model = sample_model();
        assert_eq!(model.exact_weight_bytes(), None);
        model.tensors = vec![
            TensorEntry {
                name: "a".to_owned(),
                dtype: "U8".to_owned(),
                bytes: 1000,
            },
            TensorEntry {
                name: "b".to_owned(),
                dtype: "U8".to_owned(),
                bytes: 2000,
            },
        ];
        assert_eq!(model.exact_weight_bytes(), Some(3000));
    }
}
