//! The hardware profile input (RFC-0004 FE2).

use drakkar_core::{BudgetSource, ChipId};

/// The machine the feasibility engine plans against (FE2). Populated by the live
/// hardware probe (#225) or, for `fit --machine PROFILE` simulations, from the
/// shipped per-chip fallback table. This crate performs no probing; it only
/// fixes the shape.
#[derive(Clone, PartialEq, Debug)]
pub struct MachineProfile {
    /// Chip identity and GPU core count.
    pub chip: ChipId,
    /// Total unified memory in bytes.
    pub total_ram_bytes: u64,
    /// The GPU memory budget in bytes — the live
    /// `MTLDevice.recommendedMaxWorkingSetSize` (FE15), or a table value for a
    /// simulated profile.
    pub budget_bytes: u64,
    /// Whether `budget_bytes` was probed or read from the fallback table.
    pub budget_source: BudgetSource,
    /// Current `iogpu.wired_limit_mb` (0 when unset).
    pub wired_limit_mb: u32,
    /// Free memory in bytes at probe time.
    pub free_bytes: u64,
    /// macOS version as `(major, minor)`.
    pub macos: (u16, u16),
    /// Whether the Metal 4 tensor-op (Neural Accelerator) self-test passed
    /// (IC26); never version sniffing.
    pub nax_tensor_ops: bool,
    /// Memory bandwidth in GB/s (probed or from the FE2 fallback table).
    pub bandwidth_gbs: f32,
    /// SSD read throughput class in GB/s (feeds load-time and KV-tier estimates).
    pub ssd_read_gbs: f32,
}
