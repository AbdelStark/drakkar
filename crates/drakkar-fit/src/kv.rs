//! Architecture-aware KV sizing (RFC-0004 §4, FE9–FE12; RFC-0005 §5).
//!
//! Uniform GQA billing (FE8) is wrong for hybrid attention layouts. This module
//! bills each layer at its own [`LayoutClass`]: sliding-window layers at their
//! window (FE9), MLA layers at one shared latent vector per token (FE10),
//! recurrent/SSM layers at a constant per-sequence state (FE11), and paged
//! caches add a pool-metadata term (FE12). An unknown layout (no per-layer
//! classification available) degrades explicitly to the uniform formula with a
//! `layout-unknown` flag rather than mis-pricing silently.

use drakkar_core::LayoutClass;

use crate::memory::kv_bytes_per_element;
use crate::model::ModelDescriptor;

/// Block size in tokens for the paged pool (RFC-0005 KV1; mirrors
/// `drakkar_core::BLOCK_TOKENS`, which the KV subsystem defines).
pub const BLOCK_TOKENS: u32 = 32;

/// Per-block pool metadata bytes (block tables + per-block scales, FE12).
pub const BLOCK_METADATA_BYTES: u64 = 96;

/// The KV footprint at a context, with the confidence caveat that applies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KvSizing {
    /// Total KV bytes at the requested context (for one sequence).
    pub bytes: u64,
    /// True when the per-layer layout could not be determined and the uniform
    /// formula was used as a fallback — the report labels this `modeled,
    /// layout-unknown`.
    pub layout_unknown: bool,
}

fn group_overhead(kv_bits: u8, kv_group: u32) -> f64 {
    if kv_bits < 16 && kv_group > 0 {
        32.0 / (16.0 * f64::from(kv_group))
    } else {
        0.0
    }
}

/// KV bytes for one global (paged full-attention) layer at `ctx` tokens (FE8).
fn global_layer_bytes(descriptor: &ModelDescriptor, ctx: u32, elem: f64, q: f64) -> f64 {
    2.0 * f64::from(descriptor.kv_heads)
        * f64::from(descriptor.head_dim)
        * elem
        * (1.0 + q)
        * f64::from(ctx)
}

/// The architecture-aware KV footprint for one sequence at `ctx` tokens,
/// dispatching per layer on its [`LayoutClass`] (FE9–FE12). When `paged` is set,
/// the FE12 pool-metadata term is added.
#[must_use]
pub fn kv_bytes(
    descriptor: &ModelDescriptor,
    ctx: u32,
    kv_bits: u8,
    kv_group: u32,
    paged: bool,
) -> KvSizing {
    let elem = kv_bytes_per_element(kv_bits);
    let q = group_overhead(kv_bits, kv_group);

    // No per-layer classification: degrade to uniform (every layer global) and
    // flag it (FE: unknown layout must not mis-price silently).
    if descriptor.layout_classes.is_empty() {
        let bytes = global_layer_bytes(descriptor, ctx, elem, q) * f64::from(descriptor.layers);
        return KvSizing {
            bytes: add_paged(bytes as u64, ctx, paged),
            layout_unknown: true,
        };
    }

    let mut total = 0.0f64;
    for layer in &descriptor.layout_classes {
        total += match *layer {
            LayoutClass::Global => global_layer_bytes(descriptor, ctx, elem, q),
            LayoutClass::SlidingWindow { window, sinks } => {
                // Billed at the window (plus attention sinks), not the context.
                let effective = ctx.min(window.saturating_add(sinks));
                2.0 * f64::from(descriptor.kv_heads)
                    * f64::from(descriptor.head_dim)
                    * elem
                    * (1.0 + q)
                    * f64::from(effective)
            }
            LayoutClass::MlaLatent { c_kv, d_rope } => {
                // One shared latent vector per token per layer, not per-head K/V.
                f64::from(c_kv + d_rope) * elem * (1.0 + q) * f64::from(ctx)
            }
            LayoutClass::Recurrent { state_bytes } => {
                // Constant per-sequence state, independent of context.
                state_bytes as f64
            }
        };
    }

    KvSizing {
        bytes: add_paged(total as u64, ctx, paged),
        layout_unknown: false,
    }
}

fn add_paged(bytes: u64, ctx: u32, paged: bool) -> u64 {
    if !paged {
        return bytes;
    }
    let blocks = u64::from(ctx).div_ceil(u64::from(BLOCK_TOKENS));
    bytes.saturating_add(blocks * BLOCK_METADATA_BYTES)
}

#[cfg(test)]
mod kv_layouts_tests {
    use super::*;
    use drakkar_core::{LayoutClass, QuantDesc};

    fn base(layers: u32) -> ModelDescriptor {
        ModelDescriptor {
            reference: "test/model".to_owned(),
            arch: "test".to_owned(),
            layers,
            hidden: 4096,
            heads: 32,
            kv_heads: 8,
            head_dim: 128,
            vocab: 150_000,
            params_total: 8_000_000_000,
            params_active: 8_000_000_000,
            moe: None,
            layout_classes: Vec::new(),
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

    #[test]
    fn uniform_dispatch_matches_the_fe8_rate() {
        let mut d = base(36);
        d.layout_classes = vec![LayoutClass::Global; 36];
        // 144 KiB/token at fp16 * 1000 tokens.
        let sizing = kv_bytes(&d, 1000, 16, 0, false);
        assert!(!sizing.layout_unknown);
        assert_eq!(sizing.bytes, (144.0 * 1024.0 * 1000.0) as u64);
    }

    #[test]
    fn swa_hybrid_shows_the_kink_at_the_window() {
        // Half global, half sliding-window with a 4096 window.
        let mut d = base(36);
        let mut layers = vec![LayoutClass::Global; 18];
        layers.extend(vec![
            LayoutClass::SlidingWindow {
                window: 4096,
                sinks: 0,
            };
            18
        ]);
        d.layout_classes = layers;

        let at = |ctx| kv_bytes(&d, ctx, 16, 0, false).bytes as f64;
        // Below the window both layer classes grow linearly.
        let slope_before = (at(4000) - at(2000)) / 2000.0;
        // Above the window only the global layers grow, so the slope halves
        // (SWA layers are pinned at W) — the kink at ctx = W.
        let slope_after = (at(16000) - at(8000)) / 8000.0;
        assert!(slope_after < slope_before);
        assert!((slope_after - slope_before / 2.0).abs() / slope_before < 0.05);
    }

    #[test]
    fn mla_uses_the_latent_vector_not_per_head_kv() {
        let mut mla = base(4);
        mla.layout_classes = vec![
            LayoutClass::MlaLatent {
                c_kv: 512,
                d_rope: 64,
            };
            4
        ];
        // MLA: (512+64) elements/token/layer. Uniform GQA would bill
        // 2*kv_heads*head_dim = 2*8*128 = 2048 elements/token/layer — much more.
        let mla_bytes = kv_bytes(&mla, 1000, 16, 0, false).bytes;
        let expected = ((512.0 + 64.0) * 2.0 * 1000.0 * 4.0) as u64; // elem=2 (fp16)
        assert_eq!(mla_bytes, expected);

        let mut uniform = base(4);
        uniform.layout_classes = vec![LayoutClass::Global; 4];
        assert!(mla_bytes < kv_bytes(&uniform, 1000, 16, 0, false).bytes);
    }

    #[test]
    fn recurrent_state_is_context_invariant() {
        let mut d = base(4);
        d.layout_classes = vec![
            LayoutClass::Recurrent {
                state_bytes: 1_000_000
            };
            4
        ];
        let a = kv_bytes(&d, 1000, 16, 0, false).bytes;
        let b = kv_bytes(&d, 100_000, 16, 0, false).bytes;
        assert_eq!(a, b);
        assert_eq!(a, 4_000_000);
    }

    #[test]
    fn unknown_layout_falls_back_to_uniform_and_flags_it() {
        let d = base(36); // empty layout_classes
        let sizing = kv_bytes(&d, 1000, 16, 0, false);
        assert!(sizing.layout_unknown);
        // Equals the uniform all-global sizing.
        let mut known = base(36);
        known.layout_classes = vec![LayoutClass::Global; 36];
        assert_eq!(sizing.bytes, kv_bytes(&known, 1000, 16, 0, false).bytes);
    }

    #[test]
    fn paged_metadata_term_is_added_when_requested() {
        let mut d = base(36);
        d.layout_classes = vec![LayoutClass::Global; 36];
        let plain = kv_bytes(&d, 1000, 16, 0, false).bytes;
        let paged = kv_bytes(&d, 1000, 16, 0, true).bytes;
        let blocks = 1000u64.div_ceil(u64::from(BLOCK_TOKENS));
        assert_eq!(paged, plain + blocks * BLOCK_METADATA_BYTES);
    }

    #[test]
    fn swa_hybrid_holds_more_context_than_equal_size_uniform() {
        // At long context, the SWA-hybrid footprint is smaller than uniform, so
        // it fits more context in the same budget (FE9).
        let mut swa = base(36);
        let mut layers = vec![LayoutClass::Global; 6];
        layers.extend(vec![
            LayoutClass::SlidingWindow {
                window: 4096,
                sinks: 0
            };
            30
        ]);
        swa.layout_classes = layers;
        let mut uniform = base(36);
        uniform.layout_classes = vec![LayoutClass::Global; 36];
        assert!(
            kv_bytes(&swa, 65_536, 16, 0, false).bytes
                < kv_bytes(&uniform, 65_536, 16, 0, false).bytes
        );
    }
}
