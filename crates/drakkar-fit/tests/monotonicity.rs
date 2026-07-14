//! Verdict-monotonicity property tests (RFC-0004 §7 named invariant, Testing
//! Strategy).
//!
//! For a fixed model and plan, increasing `usable` MUST NOT worsen the verdict
//! tier, and increasing `ctx_requested` MUST NOT improve it. The generator
//! covers all four KV layout classes so any refactor of the memory model is
//! validated against these properties.

use drakkar_core::{BudgetSource, ChipId, LayoutClass, QuantDesc, Verdict};
use drakkar_fit::{MachineProfile, ModelDescriptor, context, memory, verdict};
use proptest::prelude::*;

/// Rank the verdict tiers worst-to-best: lower is better, so "never worsens"
/// means the rank does not increase.
fn tier_rank(v: Verdict) -> u8 {
    match v {
        Verdict::Comfortable => 0,
        Verdict::Tight => 1,
        Verdict::NeedsTuning => 2,
        Verdict::WontFit => 3,
    }
}

fn arb_layout() -> impl Strategy<Value = LayoutClass> {
    prop_oneof![
        Just(LayoutClass::Global),
        (256u32..8192, 0u32..16)
            .prop_map(|(window, sinks)| LayoutClass::SlidingWindow { window, sinks }),
        (128u32..1024, 16u32..128)
            .prop_map(|(c_kv, d_rope)| LayoutClass::MlaLatent { c_kv, d_rope }),
        (1024u64..4_000_000).prop_map(|state_bytes| LayoutClass::Recurrent { state_bytes }),
    ]
}

fn arb_model() -> impl Strategy<Value = ModelDescriptor> {
    (
        4u32..80,             // layers
        1u32..8,              // kv_heads
        64u32..256,           // head_dim
        1u64..70_000_000_000, // params_total
        arb_layout(),
    )
        .prop_map(
            |(layers, kv_heads, head_dim, params, layout)| ModelDescriptor {
                reference: "prop/model".to_owned(),
                arch: "prop".to_owned(),
                layers,
                hidden: 4096,
                heads: kv_heads.max(1) * 4,
                kv_heads,
                head_dim,
                vocab: 150_000,
                params_total: params,
                params_active: params,
                moe: None,
                // One layout class for the whole model, cycling through all four
                // classes across generated cases.
                layout_classes: vec![layout; layers as usize],
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
            },
        )
}

fn machine(ram_gib: f64, budget_gib: f64) -> MachineProfile {
    MachineProfile {
        chip: ChipId {
            name: "prop".to_owned(),
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

fn floor_and_verdict(
    model: &ModelDescriptor,
    mach: &MachineProfile,
    ctx: u32,
    kv_bits: u8,
) -> Verdict {
    let total = memory::total(model, ctx, 1, kv_bits).total;
    let usable = memory::usable(mach, memory::RUNTIME_OVERHEAD_BYTES).bytes;
    let floor = memory::total(model, 4096, 1, 8).total;
    verdict::verdict(total, usable, floor)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// (a) Increasing `usable` (via a larger budget) never worsens the verdict.
    #[test]
    fn monotonicity_usable_increase_never_worsens_verdict(
        model in arb_model(),
        ctx in 512u32..131_072,
        base_budget in 4.0f64..96.0,
        bump in 0.0f64..64.0,
    ) {
        let ram = 128.0; // large, fixed RAM so the budget is the binding constraint
        let small = machine(ram, base_budget);
        let large = machine(ram, base_budget + bump);
        let v_small = floor_and_verdict(&model, &small, ctx, 16);
        let v_large = floor_and_verdict(&model, &large, ctx, 16);
        prop_assert!(
            tier_rank(v_large) <= tier_rank(v_small),
            "usable up worsened verdict: {v_small:?} -> {v_large:?}"
        );
    }

    /// (b) Increasing `ctx_requested` never improves the verdict.
    #[test]
    fn monotonicity_ctx_increase_never_improves_verdict(
        model in arb_model(),
        ctx1 in 512u32..64_000,
        extra in 0u32..64_000,
        budget in 8.0f64..96.0,
    ) {
        let mach = machine(128.0, budget);
        let v1 = floor_and_verdict(&model, &mach, ctx1, 16);
        let v2 = floor_and_verdict(&model, &mach, ctx1 + extra, 16);
        prop_assert!(
            tier_rank(v2) >= tier_rank(v1),
            "ctx up improved verdict: {v1:?} -> {v2:?}"
        );
    }

    /// (c) `ctx_max(kv4) >= ctx_max(kv8) >= ctx_max(fp16)`.
    #[test]
    fn monotonicity_ctx_max_ordering_across_precisions(
        model in arb_model(),
        budget in 8.0f64..96.0,
    ) {
        let mach = machine(128.0, budget);
        let fp16 = context::ctx_max(&model, &mach, 16, 1);
        let kv8 = context::ctx_max(&model, &mach, 8, 1);
        let kv4 = context::ctx_max(&model, &mach, 4, 1);
        prop_assert!(kv4 >= kv8, "kv4 {kv4} < kv8 {kv8}");
        prop_assert!(kv8 >= fp16, "kv8 {kv8} < fp16 {fp16}");
    }

    /// (d) `total` is monotone non-decreasing in ctx, concurrency, and KV width.
    #[test]
    fn monotonicity_total_is_monotone_in_ctx_conc_kv(
        model in arb_model(),
        ctx in 512u32..64_000,
        conc in 1u32..8,
    ) {
        let t = |c: u32, k: u8, cc: u32| memory::total(&model, c, cc, k).total;

        // Monotone in ctx.
        prop_assert!(t(ctx + 4096, 16, conc) >= t(ctx, 16, conc));
        // Monotone in concurrency.
        prop_assert!(t(ctx, 16, conc + 1) >= t(ctx, 16, conc));
        // Monotone in KV width (4 -> 8 -> 16 bit).
        prop_assert!(t(ctx, 8, conc) >= t(ctx, 4, conc));
        prop_assert!(t(ctx, 16, conc) >= t(ctx, 8, conc));
    }
}
