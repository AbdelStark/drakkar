//! The core memory arithmetic (RFC-0004 §3–§6, performance-budget §2).
//!
//! The master identity is
//! `total = weights + kv_pool + activation_watermark + runtime_overhead
//! + draft_model + fragmentation_margin` (FE5–FE16), and the budget identity is
//! `usable = budget − runtime_overhead − fragmentation_margin` subject to the
//! `os_floor` reserve (FE16). This module covers weights (FE5–FE7), the uniform
//! GQA KV formula (FE8), the everything-else terms (FE13–FE14), and the
//! usable/os_floor computation. Exotic KV layouts (hybrid-SWA, MLA, SSM, paged
//! metadata; FE9–FE12) are a follow-up (#228); the verdict tiers (#230) and the
//! context solver (#229) consume the raw `total`/`usable` exposed here.

use crate::machine::MachineProfile;
use crate::model::ModelDescriptor;

/// Bytes in one GiB.
pub const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// The shipped default runtime overhead (FE14): Metal + allocator + tokenizer +
/// process. Calibrated per machine on first run.
pub const RUNTIME_OVERHEAD_BYTES: u64 = (1.2 * BYTES_PER_GIB) as u64;

/// The shipped default activation high-water mark for a dense arch class at the
/// default chunk size (FE13). Replaced by measured values on first load.
pub const ACTIVATION_DEFAULT_BYTES: u64 = (0.4 * BYTES_PER_GIB) as u64;

/// The fragmentation margin as a fraction of the plan (RFC-0004 §5).
pub const FRAGMENTATION_FRACTION: f64 = 0.03;

/// The default KV-quantization group size (RFC-0005); feeds the FE8
/// `q_overhead` term when KV is group-quantized.
pub const KV_GROUP_DEFAULT: u32 = 64;

/// Effective bits per stored weight for a quantization scheme (FE5). MLX affine
/// is `bits + 32/group` (fp16 scale+bias per group); MXFP4 is 4.25; bf16/fp16
/// are 16; GGUF families use a shipped per-type table.
#[must_use]
pub fn bpw_eff(scheme: &str, bits: u8, group: u32) -> f64 {
    let lower = scheme.to_ascii_lowercase();
    // GGUF per-type table (keyed by the scheme string, e.g. "Q4_K_M").
    if let Some(bpw) = gguf_bpw(&scheme.to_ascii_uppercase()) {
        return bpw;
    }
    match lower.as_str() {
        "mxfp4" => 4.25,
        "bf16" | "fp16" | "float16" | "f16" => 16.0,
        "mlx_affine" | "mlx" | "affine" => {
            if group == 0 {
                f64::from(bits)
            } else {
                f64::from(bits) + 32.0 / f64::from(group)
            }
        }
        _ => {
            // Unknown scheme: fall back to the affine estimate if a group is
            // given, else the raw bit width.
            if group == 0 {
                f64::from(bits)
            } else {
                f64::from(bits) + 32.0 / f64::from(group)
            }
        }
    }
}

fn gguf_bpw(ty: &str) -> Option<f64> {
    Some(match ty {
        "Q2_K" => 3.35,
        "Q3_K_M" => 3.9,
        "Q4_0" => 4.5,
        "Q4_K_M" => 4.85,
        "Q5_0" => 5.5,
        "Q5_K_M" => 5.7,
        "Q6_K" => 6.6,
        "Q8_0" => 8.5,
        _ => return None,
    })
}

/// The model's weight footprint in bytes (FE5). Exact when the safetensors index
/// is present (the sum of per-tensor sizes); otherwise the estimate
/// `params_total × bpw_eff / 8`.
#[must_use]
pub fn weight_bytes(descriptor: &ModelDescriptor) -> u64 {
    if let Some(exact) = descriptor.exact_weight_bytes() {
        return exact;
    }
    let bpw = bpw_eff(
        &descriptor.quant.scheme,
        descriptor.quant.bits,
        descriptor.quant.group,
    );
    ((descriptor.params_total as f64) * bpw / 8.0) as u64
}

/// Bytes per KV element for a KV precision: 2 (fp16), 1 (8-bit), 0.5 (4-bit)
/// (FE8).
#[must_use]
pub fn kv_bytes_per_element(kv_bits: u8) -> f64 {
    f64::from(kv_bits) / 8.0
}

/// The uniform full-attention (GQA) KV footprint per token, in bytes (FE8):
/// `2 × n_layers × n_kv_heads × head_dim × bytes_elem × (1 + q_overhead)`, where
/// `q_overhead ≈ 32/(16 × group)` applies only to group-quantized KV.
#[must_use]
pub fn kv_bytes_per_token(descriptor: &ModelDescriptor, kv_bits: u8, kv_group: u32) -> f64 {
    let bytes_elem = kv_bytes_per_element(kv_bits);
    let q_overhead = if kv_bits < 16 && kv_group > 0 {
        32.0 / (16.0 * f64::from(kv_group))
    } else {
        0.0
    };
    2.0 * f64::from(descriptor.layers)
        * f64::from(descriptor.kv_heads)
        * f64::from(descriptor.head_dim)
        * bytes_elem
        * (1.0 + q_overhead)
}

/// The `os_floor` reserve of un-wired system RAM to leave for macOS (FE16): 4
/// GiB (≤ 16 GiB), 6 GiB (≤ 36 GiB), 8 GiB (≤ 64 GiB), 12 GiB (> 64 GiB).
#[must_use]
pub fn os_floor_bytes(total_ram_bytes: u64) -> u64 {
    let gib = total_ram_bytes as f64 / BYTES_PER_GIB;
    let floor_gib = if gib <= 16.0 {
        4.0
    } else if gib <= 36.0 {
        6.0
    } else if gib <= 64.0 {
        8.0
    } else {
        12.0
    };
    (floor_gib * BYTES_PER_GIB) as u64
}

/// Which constraint bound the usable budget (FE16).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindingConstraint {
    /// The GPU working-set budget bound first.
    GpuBudget,
    /// The `os_floor` system-RAM reserve bound first.
    OsFloor,
}

/// The usable memory a plan may consume, and which constraint bound it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Usable {
    /// Usable bytes for weights + KV + activation + draft.
    pub bytes: u64,
    /// Which of the two FE16 constraints bound first.
    pub binding: BindingConstraint,
}

/// The usable budget (FE16): `min(budget, total_ram − os_floor) − runtime_overhead
/// − fragmentation_margin`, recording whichever constraint binds first.
#[must_use]
pub fn usable(machine: &MachineProfile, runtime_overhead: u64) -> Usable {
    let os_floor = os_floor_bytes(machine.total_ram_bytes);
    let ram_available = machine.total_ram_bytes.saturating_sub(os_floor);
    let (effective, binding) = if machine.budget_bytes <= ram_available {
        (machine.budget_bytes, BindingConstraint::GpuBudget)
    } else {
        (ram_available, BindingConstraint::OsFloor)
    };
    let frag = (effective as f64 * FRAGMENTATION_FRACTION) as u64;
    let bytes = effective
        .saturating_sub(runtime_overhead)
        .saturating_sub(frag);
    Usable { bytes, binding }
}

/// The six-term memory decomposition and its sum (the master identity, §5).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MemoryTotal {
    /// Model weights.
    pub weights: u64,
    /// KV pool for the requested context and concurrency.
    pub kv_pool: u64,
    /// Activation high-water mark.
    pub activation_watermark: u64,
    /// Runtime overhead.
    pub runtime_overhead: u64,
    /// Draft-model weights (0 unless speculating with a draft).
    pub draft_model: u64,
    /// Fragmentation margin (3% of the other terms).
    pub fragmentation_margin: u64,
    /// The sum of the six terms.
    pub total: u64,
}

/// Assemble the six-term memory total for a plan (§5). `kv_pool` scales with
/// `ctx × concurrency`; `activation_watermark`, `runtime_overhead`, and
/// `draft_model` use the shipped defaults.
#[must_use]
pub fn total(descriptor: &ModelDescriptor, ctx: u32, concurrency: u32, kv_bits: u8) -> MemoryTotal {
    let weights = weight_bytes(descriptor);
    let kv_per_token = kv_bytes_per_token(descriptor, kv_bits, KV_GROUP_DEFAULT);
    let kv_pool = (kv_per_token * f64::from(ctx) * f64::from(concurrency.max(1))) as u64;
    let activation_watermark = ACTIVATION_DEFAULT_BYTES;
    let runtime_overhead = RUNTIME_OVERHEAD_BYTES;
    let draft_model = 0;
    let subtotal = weights + kv_pool + activation_watermark + runtime_overhead + draft_model;
    let fragmentation_margin = (subtotal as f64 * FRAGMENTATION_FRACTION) as u64;
    MemoryTotal {
        weights,
        kv_pool,
        activation_watermark,
        runtime_overhead,
        draft_model,
        fragmentation_margin,
        total: subtotal + fragmentation_margin,
    }
}

#[cfg(test)]
mod memory_model_tests {
    use super::*;
    use drakkar_core::{BudgetSource, ChipId, LayoutClass, MoeTopology, QuantDesc};

    fn descriptor(layers: u32, kv_heads: u32, head_dim: u32) -> ModelDescriptor {
        ModelDescriptor {
            reference: "test/model".to_owned(),
            arch: "test".to_owned(),
            layers,
            hidden: 4096,
            heads: 32,
            kv_heads,
            head_dim,
            vocab: 150_000,
            params_total: 8_190_000_000,
            params_active: 8_190_000_000,
            moe: None,
            layout_classes: vec![LayoutClass::Global; layers as usize],
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
            total_ram_bytes: (ram_gib * BYTES_PER_GIB) as u64,
            budget_bytes: (budget_gib * BYTES_PER_GIB) as u64,
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
    fn bpw_eff_table() {
        assert!((bpw_eff("mlx_affine", 4, 64) - 4.5).abs() < 1e-9);
        assert!((bpw_eff("mlx_affine", 4, 32) - 5.0).abs() < 1e-9);
        assert!((bpw_eff("mlx_affine", 8, 64) - 8.5).abs() < 1e-9);
        assert!((bpw_eff("mxfp4", 4, 0) - 4.25).abs() < 1e-9);
        assert!((bpw_eff("bf16", 16, 0) - 16.0).abs() < 1e-9);
        assert!((bpw_eff("Q4_K_M", 4, 0) - 4.85).abs() < 1e-9);
        assert!((bpw_eff("Q5_K_M", 5, 0) - 5.7).abs() < 1e-9);
        assert!((bpw_eff("Q6_K", 6, 0) - 6.6).abs() < 1e-9);
        assert!((bpw_eff("Q8_0", 8, 0) - 8.5).abs() < 1e-9);
    }

    #[test]
    fn kv_bytes_per_token_reproduces_fe8_table() {
        // fp16, no group overhead. KiB/token = bytes / 1024.
        let kib = |d: &ModelDescriptor| kv_bytes_per_token(d, 16, 0) / 1024.0;
        assert!((kib(&descriptor(32, 8, 128)) - 128.0).abs() < 1e-6); // Llama-3.1-8B
        assert!((kib(&descriptor(36, 8, 128)) - 144.0).abs() < 1e-6); // Qwen3-8B
        assert!((kib(&descriptor(48, 4, 128)) - 96.0).abs() < 1e-6); // Qwen3-30B-A3B
        assert!((kib(&descriptor(80, 8, 128)) - 320.0).abs() < 1e-6); // Llama-3.3-70B
    }

    #[test]
    fn kv_quant_adds_group_overhead() {
        let d = descriptor(36, 8, 128);
        // 8-bit halves the element size and adds ~3% at g64.
        let fp16 = kv_bytes_per_token(&d, 16, 0);
        let int8 = kv_bytes_per_token(&d, 8, 64);
        let expected = fp16 / 2.0 * (1.0 + 32.0 / (16.0 * 64.0));
        assert!((int8 - expected).abs() < 1e-6);
    }

    #[test]
    fn weight_bytes_exact_and_estimated_paths() {
        // Estimated: 8.19e9 params * 4.5 bpw / 8.
        let d = descriptor(36, 8, 128);
        let estimated = weight_bytes(&d);
        let expected = (8_190_000_000f64 * 4.5 / 8.0) as u64;
        assert_eq!(estimated, expected);

        // Exact path takes the index sum.
        let mut with_index = d;
        with_index.tensors = vec![crate::model::TensorEntry {
            name: "w".to_owned(),
            dtype: "U8".to_owned(),
            bytes: 1_234_567,
        }];
        assert_eq!(weight_bytes(&with_index), 1_234_567);
    }

    #[test]
    fn os_floor_binding() {
        // Large budget, small RAM: the os_floor (RAM) constraint binds.
        let m = machine(16.0, 14.0); // 16 GiB RAM, os_floor 4 -> ram_available 12 < budget 14
        let u = usable(&m, RUNTIME_OVERHEAD_BYTES);
        assert_eq!(u.binding, BindingConstraint::OsFloor);

        // Small budget, large RAM: the GPU budget binds.
        let m = machine(64.0, 40.0); // os_floor 8 -> ram_available 56 > budget 40
        let u = usable(&m, RUNTIME_OVERHEAD_BYTES);
        assert_eq!(u.binding, BindingConstraint::GpuBudget);

        // os_floor tiers.
        assert_eq!(
            os_floor_bytes((16.0 * BYTES_PER_GIB) as u64),
            (4.0 * BYTES_PER_GIB) as u64
        );
        assert_eq!(
            os_floor_bytes((36.0 * BYTES_PER_GIB) as u64),
            (6.0 * BYTES_PER_GIB) as u64
        );
        assert_eq!(
            os_floor_bytes((64.0 * BYTES_PER_GIB) as u64),
            (8.0 * BYTES_PER_GIB) as u64
        );
        assert_eq!(
            os_floor_bytes((128.0 * BYTES_PER_GIB) as u64),
            (12.0 * BYTES_PER_GIB) as u64
        );
    }

    #[test]
    fn total_returns_six_terms_and_their_sum() {
        let d = descriptor(36, 8, 128);
        let t = total(&d, 32_768, 1, 16);
        assert_eq!(t.runtime_overhead, RUNTIME_OVERHEAD_BYTES);
        assert_eq!(t.activation_watermark, ACTIVATION_DEFAULT_BYTES);
        assert_eq!(t.draft_model, 0);
        // KV pool = 144 KiB/token * 32768 tokens.
        let expected_kv = (144.0 * 1024.0 * 32_768.0) as u64;
        assert_eq!(t.kv_pool, expected_kv);
        let subtotal =
            t.weights + t.kv_pool + t.activation_watermark + t.runtime_overhead + t.draft_model;
        assert_eq!(t.fragmentation_margin, (subtotal as f64 * 0.03) as u64);
        assert_eq!(t.total, subtotal + t.fragmentation_margin);
    }

    #[test]
    fn moe_active_params_are_available_for_downstream_roofline() {
        // The descriptor distinguishes total vs active params (used by decode
        // roofline #231); the memory model bills weights on total params.
        let mut d = descriptor(48, 4, 128);
        d.moe = Some(MoeTopology {
            num_experts: 128,
            experts_per_token: 8,
            shared_experts: 0,
        });
        d.params_active = 3_000_000_000;
        assert!(weight_bytes(&d) > 0);
        assert_eq!(d.params_active, 3_000_000_000);
    }
}
