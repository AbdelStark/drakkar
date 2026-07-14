//! Verdict tiers, ranked remedies, and wired-limit guidance (RFC-0004 §7, FE17,
//! FE19).
//!
//! Getting either direction wrong is a product failure: a false won't-fit turns
//! users away; a false comfortable produces the mid-generation OOM the product
//! exists to eliminate. The `0.85` headroom factor is fixed policy and is never
//! tuned per machine. Wired-limit guidance is exact and revertible, and DRAKKAR
//! never applies it automatically.

use drakkar_core::{FitRemedy, FitRemedyKind, Verdict};

use crate::machine::MachineProfile;
use crate::memory::os_floor_bytes;

/// The fixed headroom factor separating Comfortable from Tight (FE19). Fixed
/// policy: it MUST NOT be tuned per machine.
pub const HEADROOM_FACTOR: f64 = 0.85;

const BYTES_PER_MIB: f64 = 1024.0 * 1024.0;

/// The four FE19 verdict tiers for a plan.
///
/// `total` is the requested plan's footprint, `usable` is the FE16 usable
/// budget, and `floor_plan_total` is the footprint of the floor plan (lowest
/// sane quant, 4k context, 8-bit KV). The floor plan is required to distinguish
/// *Needs tuning* (fails as requested but a remedy fits) from *Won't fit* (even
/// the floor plan exceeds the machine) — the exact false-comfortable /
/// false-won't-fit failure mode this verdict guards against; a `total`/`usable`
/// pair alone cannot make that distinction.
#[must_use]
pub fn verdict(total: u64, usable: u64, floor_plan_total: u64) -> Verdict {
    if (total as f64) <= HEADROOM_FACTOR * (usable as f64) {
        Verdict::Comfortable
    } else if total <= usable {
        Verdict::Tight
    } else if floor_plan_total <= usable {
        Verdict::NeedsTuning
    } else {
        Verdict::WontFit
    }
}

/// The FE19-ordered remedy ladder for a model that needs tuning, ranked by
/// expected quality impact: (1) official smaller quant, (2) on-device
/// quantization, (3) KV 8-bit, (4) reduced context, (5) KV below 8-bit, (6)
/// wired-limit raise (opt-in, always last).
#[must_use]
pub fn remedies(model_ref: &str, wired: Option<&WiredLimitProposal>) -> Vec<FitRemedy> {
    let mut plan = vec![
        FitRemedy {
            rank: 1,
            kind: FitRemedyKind::OfficialQuant,
            command: format!("drakkar pull {model_ref} --quant 4bit-g64"),
            effect: "Use an official 4-bit artifact; smallest quality cost.".to_owned(),
        },
        FitRemedy {
            rank: 2,
            kind: FitRemedyKind::OnDeviceQuant,
            command: format!("drakkar convert {model_ref} --quant 4bit-g64"),
            effect: "Quantize on device to the store when no official quant exists.".to_owned(),
        },
        FitRemedy {
            rank: 3,
            kind: FitRemedyKind::Kv8Bit,
            command: format!("drakkar run {model_ref} --kv-bits 8"),
            effect: "Halve KV memory at negligible quality cost.".to_owned(),
        },
        FitRemedy {
            rank: 4,
            kind: FitRemedyKind::ReducedContext,
            command: format!("drakkar run {model_ref} --ctx 8192"),
            effect: "Lower the context ceiling to fit the KV pool.".to_owned(),
        },
        FitRemedy {
            rank: 5,
            kind: FitRemedyKind::KvSubByte,
            command: format!("drakkar run {model_ref} --kv-bits 4"),
            effect: "Quarter KV memory; measurable quality cost on long context.".to_owned(),
        },
    ];
    // Wired-limit raise is always last and opt-in (FE17).
    if let Some(w) = wired {
        plan.push(FitRemedy {
            rank: 6,
            kind: FitRemedyKind::WiredLimitRaise,
            command: w.command.clone(),
            effect: format!(
                "Raise the GPU wired limit to {} MB (opt-in; Apple does not support it; revert with `{}`).",
                w.wired_limit_mb, w.revert
            ),
        });
    }
    plan
}

/// An exact, revertible `iogpu.wired_limit_mb` proposal (FE17). Never applied
/// automatically.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WiredLimitProposal {
    /// The proposed limit in MB, computed to respect the FE16 `os_floor`.
    pub wired_limit_mb: u32,
    /// The exact command to apply it.
    pub command: String,
    /// The exact command to revert it.
    pub revert: String,
    /// The unsupported-configuration note shown with the proposal.
    pub note: String,
}

/// Propose a wired-limit raise that lets `plan_bytes` fit while leaving the FE16
/// `os_floor` of system RAM un-wired, or `None` when even the maximum safe wired
/// limit cannot fit the plan (a genuine Won't-fit). Never sets the value.
#[must_use]
pub fn wired_limit_proposal(
    plan_bytes: u64,
    machine: &MachineProfile,
) -> Option<WiredLimitProposal> {
    let os_floor = os_floor_bytes(machine.total_ram_bytes);
    let max_wired = machine.total_ram_bytes.saturating_sub(os_floor);
    if plan_bytes > max_wired {
        return None;
    }
    // Round the requirement up to whole MB, but never exceed the os_floor-safe
    // maximum.
    let needed_mb = (plan_bytes as f64 / BYTES_PER_MIB).ceil() as u64;
    let max_mb = (max_wired as f64 / BYTES_PER_MIB).floor() as u64;
    let n_mb = needed_mb.min(max_mb) as u32;
    Some(WiredLimitProposal {
        wired_limit_mb: n_mb,
        command: format!("sudo sysctl iogpu.wired_limit_mb={n_mb}"),
        revert: "sudo sysctl iogpu.wired_limit_mb=0".to_owned(),
        note: "Apple does not officially support raising iogpu.wired_limit_mb; \
               persist it via /etc/sysctl.conf or a LaunchDaemon. Revert with \
               `sudo sysctl iogpu.wired_limit_mb=0`."
            .to_owned(),
    })
}

/// Whether `plan_bytes` can be wired for the GPU while respecting the FE16
/// `os_floor` on this machine — the computation `drakkar doctor` consumes to
/// report whether the current wired limit is safe for a resident model.
#[must_use]
pub fn wired_limit_safe_for(plan_bytes: u64, machine: &MachineProfile) -> bool {
    let os_floor = os_floor_bytes(machine.total_ram_bytes);
    plan_bytes <= machine.total_ram_bytes.saturating_sub(os_floor)
}

/// The full verdict outcome for a plan: the tier, the ranked remedies (empty
/// when it fits as requested), the nearest sibling to suggest on Won't-fit, and
/// any wired-limit proposal.
#[derive(Clone, PartialEq, Debug)]
pub struct VerdictOutcome {
    /// The FE19 tier.
    pub verdict: Verdict,
    /// The ranked remedy plan (empty for Comfortable/Tight).
    pub remedies: Vec<FitRemedy>,
    /// The nearest sibling model that fits (populated on Won't-fit by the
    /// sibling-remedy integration, #83).
    pub nearest_sibling: Option<String>,
    /// The wired-limit proposal, when a raise would help.
    pub wired_limit: Option<WiredLimitProposal>,
}

/// Assemble the full verdict outcome (FE17/FE19).
#[must_use]
pub fn assess(
    total: u64,
    usable: u64,
    floor_plan_total: u64,
    model_ref: &str,
    machine: &MachineProfile,
    sibling_hint: Option<String>,
) -> VerdictOutcome {
    let tier = verdict(total, usable, floor_plan_total);
    let wired_limit = if matches!(tier, Verdict::NeedsTuning | Verdict::WontFit) {
        wired_limit_proposal(total, machine)
    } else {
        None
    };
    let remedies = match tier {
        Verdict::Comfortable | Verdict::Tight => Vec::new(),
        Verdict::NeedsTuning | Verdict::WontFit => remedies(model_ref, wired_limit.as_ref()),
    };
    let nearest_sibling = if matches!(tier, Verdict::WontFit) {
        sibling_hint
    } else {
        None
    };
    VerdictOutcome {
        verdict: tier,
        remedies,
        nearest_sibling,
        wired_limit,
    }
}

#[cfg(test)]
mod verdicts_tests {
    use super::*;
    use drakkar_core::{BudgetSource, ChipId};

    fn machine(ram_gib: f64, budget_gib: f64) -> MachineProfile {
        MachineProfile {
            chip: ChipId {
                name: "test".to_owned(),
                gpu_cores: 10,
            },
            total_ram_bytes: (ram_gib * crate::memory::BYTES_PER_GIB) as u64,
            budget_bytes: (budget_gib * crate::memory::BYTES_PER_GIB) as u64,
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
    fn verdict_thresholds_use_the_fixed_0_85_factor() {
        let usable = 100;
        assert_eq!(verdict(80, usable, 40), Verdict::Comfortable); // 80 <= 85
        assert_eq!(verdict(85, usable, 40), Verdict::Comfortable); // exactly 0.85
        assert_eq!(verdict(90, usable, 40), Verdict::Tight); // 85 < 90 <= 100
        assert_eq!(verdict(100, usable, 40), Verdict::Tight); // exactly usable
        assert_eq!(verdict(120, usable, 40), Verdict::NeedsTuning); // floor fits
        assert_eq!(verdict(120, usable, 150), Verdict::WontFit); // even floor exceeds
    }

    #[test]
    fn remedies_are_fe19_ordered_and_wired_is_last_and_opt_in() {
        let m = machine(48.0, 36.0);
        let wired = wired_limit_proposal(20 * (crate::memory::BYTES_PER_GIB as u64), &m);
        let plan = remedies("qwen3:8b", wired.as_ref());
        let ranks: Vec<u8> = plan.iter().map(|r| r.rank).collect();
        assert_eq!(ranks, vec![1, 2, 3, 4, 5, 6]);
        let last = plan.last().unwrap();
        assert_eq!(last.kind, FitRemedyKind::WiredLimitRaise);
        assert!(last.effect.contains("opt-in"));
        // Without a wired proposal, the ladder ends at rank 5.
        let plan = remedies("qwen3:8b", None);
        assert_eq!(plan.len(), 5);
        assert_ne!(plan.last().unwrap().kind, FitRemedyKind::WiredLimitRaise);
    }

    #[test]
    fn wired_proposal_respects_os_floor() {
        let m = machine(48.0, 36.0); // os_floor 8 GiB -> max_wired 40 GiB
        let plan_bytes = 37 * (crate::memory::BYTES_PER_GIB as u64);
        let p = wired_limit_proposal(plan_bytes, &m).unwrap();
        // The proposed limit never exceeds ram - os_floor (leaves the OS its floor).
        let max_mb = ((40.0 * crate::memory::BYTES_PER_GIB) / BYTES_PER_MIB) as u32;
        assert!(p.wired_limit_mb <= max_mb);
        assert!(p.wired_limit_mb >= (37.0 * crate::memory::BYTES_PER_GIB / BYTES_PER_MIB) as u32);
        assert_eq!(
            p.command,
            format!("sudo sysctl iogpu.wired_limit_mb={}", p.wired_limit_mb)
        );
        assert_eq!(p.revert, "sudo sysctl iogpu.wired_limit_mb=0");
        // A plan larger than the os_floor-safe maximum yields no proposal.
        assert!(wired_limit_proposal(45 * (crate::memory::BYTES_PER_GIB as u64), &m).is_none());
    }

    #[test]
    fn wont_fit_outcome_carries_nearest_sibling() {
        let m = machine(16.0, 12.0);
        let outcome = assess(
            30 * (crate::memory::BYTES_PER_GIB as u64),
            8 * (crate::memory::BYTES_PER_GIB as u64),
            20 * (crate::memory::BYTES_PER_GIB as u64),
            "llama3.3:70b",
            &m,
            Some("qwen3:30b-a3b".to_owned()),
        );
        assert_eq!(outcome.verdict, Verdict::WontFit);
        assert_eq!(outcome.nearest_sibling.as_deref(), Some("qwen3:30b-a3b"));
    }

    #[test]
    fn comfortable_outcome_has_no_remedies_or_sibling() {
        let m = machine(48.0, 36.0);
        let outcome = assess(10, 100, 5, "qwen3:8b", &m, Some("x".to_owned()));
        assert_eq!(outcome.verdict, Verdict::Comfortable);
        assert!(outcome.remedies.is_empty());
        assert!(outcome.nearest_sibling.is_none());
        assert!(outcome.wired_limit.is_none());
    }
}
