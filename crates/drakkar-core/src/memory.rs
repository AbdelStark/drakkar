//! The memory contract as data (data-model §3.3, RFC-0001 I2).
//!
//! [`MemoryBudget`] is declared once at load (computed by `drakkar-fit`);
//! [`MemoryReport`] is the backend's measured answer. The construction
//! invariant `declared == sum(components)` is enforced here so a budget can
//! never silently disagree with its own decomposition.

use serde::{Deserialize, Serialize};

/// The declared memory budget for one loaded model instance. `declared` is never
/// exceeded (invariant I2); it always equals the sum of the component fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct MemoryBudget {
    /// Total contract in bytes; equals the sum of the components below.
    pub declared: u64,
    /// Model weights (bytes).
    pub weights: u64,
    /// KV pool, carved into blocks up front at load (KV2).
    pub kv_pool: u64,
    /// Activation high-water mark, bounded by chunk size (IC13, FE13).
    pub activation_watermark: u64,
    /// Runtime overhead (FE14: shipped 1.2 GiB default, calibrated floor).
    pub runtime_overhead: u64,
    /// Draft-model weights; 0 unless speculating with a draft (IC19).
    pub draft_model: u64,
    /// Fragmentation margin (3% of the above, RFC-0004 §5).
    pub fragmentation_margin: u64,
}

/// Error returned when a [`MemoryBudget`] is asserted with a `declared` total
/// that does not equal the sum of its components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetMismatch {
    /// The `declared` total that was supplied.
    pub declared: u64,
    /// The sum of the component fields.
    pub component_sum: u64,
}

impl std::fmt::Display for BudgetMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "memory budget declared {} but components sum to {}",
            self.declared, self.component_sum
        )
    }
}

impl std::error::Error for BudgetMismatch {}

impl MemoryBudget {
    /// Build a budget from its components; `declared` is computed as their sum,
    /// so the construction invariant holds by definition. Saturating addition
    /// keeps the total finite even for pathological inputs.
    #[must_use]
    pub fn new(
        weights: u64,
        kv_pool: u64,
        activation_watermark: u64,
        runtime_overhead: u64,
        draft_model: u64,
        fragmentation_margin: u64,
    ) -> Self {
        let declared = weights
            .saturating_add(kv_pool)
            .saturating_add(activation_watermark)
            .saturating_add(runtime_overhead)
            .saturating_add(draft_model)
            .saturating_add(fragmentation_margin);
        MemoryBudget {
            declared,
            weights,
            kv_pool,
            activation_watermark,
            runtime_overhead,
            draft_model,
            fragmentation_margin,
        }
    }

    /// The sum of the component fields (everything except `declared`).
    #[must_use]
    pub fn component_sum(&self) -> u64 {
        self.weights
            .saturating_add(self.kv_pool)
            .saturating_add(self.activation_watermark)
            .saturating_add(self.runtime_overhead)
            .saturating_add(self.draft_model)
            .saturating_add(self.fragmentation_margin)
    }

    /// Validate the construction invariant `declared == sum(components)`.
    ///
    /// # Errors
    /// Returns [`BudgetMismatch`] when the declared total disagrees with the
    /// component sum.
    pub fn validate(&self) -> Result<(), BudgetMismatch> {
        let component_sum = self.component_sum();
        if self.declared == component_sum {
            Ok(())
        } else {
            Err(BudgetMismatch {
                declared: self.declared,
                component_sum,
            })
        }
    }

    /// Build a budget from an explicit `declared` total plus components,
    /// rejecting the construction when they disagree.
    ///
    /// # Errors
    /// Returns [`BudgetMismatch`] when `declared != sum(components)`.
    pub fn try_from_parts(
        declared: u64,
        weights: u64,
        kv_pool: u64,
        activation_watermark: u64,
        runtime_overhead: u64,
        draft_model: u64,
        fragmentation_margin: u64,
    ) -> Result<Self, BudgetMismatch> {
        let budget = MemoryBudget {
            declared,
            weights,
            kv_pool,
            activation_watermark,
            runtime_overhead,
            draft_model,
            fragmentation_margin,
        };
        budget.validate().map(|()| budget)
    }
}

/// The measured resident footprint of an engine instance, alongside the contract
/// it was loaded under (RFC-0001 §5 `memory_report()`, IC25).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct MemoryReport {
    /// Measured resident engine footprint (bytes).
    pub actual: u64,
    /// Echo of the contract `declared` total (bytes).
    pub declared: u64,
    /// Decomposition of `actual`.
    pub breakdown: MemoryBreakdown,
    /// Live `recommendedMaxWorkingSetSize` probe echo (FE15, IC25).
    pub metal_recommended_working_set: u64,
    /// Current `iogpu.wired_limit_mb` (FE17); 0 when unset.
    pub wired_limit_mb: u32,
}

impl MemoryReport {
    /// Whether the measured footprint honours the contract
    /// (`actual <= declared`), i.e. INV-BUDGET holds (DM15).
    #[must_use]
    pub fn within_budget(&self) -> bool {
        self.actual <= self.declared
    }
}

/// The decomposition of a [`MemoryReport::actual`] footprint (DM15).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct MemoryBreakdown {
    /// Weights resident on device (bytes).
    pub weights: u64,
    /// KV storage across all states (bytes).
    pub kv: u64,
    /// Activation buffers (bytes).
    pub activations: u64,
    /// Allocator cache / retained device memory not yet returned (bytes).
    pub allocator_cache: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_computes_declared_as_component_sum() {
        let b = MemoryBudget::new(10, 20, 3, 5, 0, 2);
        assert_eq!(b.declared, 40);
        assert!(b.validate().is_ok());
    }

    #[test]
    fn try_from_parts_rejects_mismatched_declared() {
        // 10+20+3+5+0+2 = 40, not 99.
        let err = MemoryBudget::try_from_parts(99, 10, 20, 3, 5, 0, 2).unwrap_err();
        assert_eq!(err.declared, 99);
        assert_eq!(err.component_sum, 40);
    }

    #[test]
    fn try_from_parts_accepts_consistent_declared() {
        let b = MemoryBudget::try_from_parts(40, 10, 20, 3, 5, 0, 2).unwrap();
        assert_eq!(b.declared, 40);
    }

    #[test]
    fn report_within_budget() {
        let report = MemoryReport {
            actual: 30,
            declared: 40,
            breakdown: MemoryBreakdown {
                weights: 20,
                kv: 6,
                activations: 3,
                allocator_cache: 1,
            },
            metal_recommended_working_set: 42,
            wired_limit_mb: 0,
        };
        assert!(report.within_budget());
    }
}
