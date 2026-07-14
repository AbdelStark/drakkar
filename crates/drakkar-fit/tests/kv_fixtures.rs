//! Hand-computed KV-curve fixtures (RFC-0004 Testing Strategy AC2, FE8–FE11).
//!
//! Each fixture pins `kv_bytes(ctx)` at ctx ∈ {1k, 4k, 8k, 32k, 128k} against
//! values computed by hand (the arithmetic is written out in comments), one per
//! layout class. Wrapped in a `kv_fixtures` module so `cargo test -p drakkar-fit
//! kv_fixtures` selects them.

#[cfg(test)]
mod kv_fixtures {
    use drakkar_core::LayoutClass;
    use drakkar_core::QuantDesc;
    use drakkar_fit::{ModelDescriptor, kv};

    const CTX_POINTS: [u32; 5] = [1024, 4096, 8192, 32768, 131_072];

    /// A model with the given per-layer layout; only the fields `kv_bytes`
    /// consumes (layers, kv_heads, head_dim, layout_classes) are meaningful.
    fn model(kv_heads: u32, head_dim: u32, layout: Vec<LayoutClass>) -> ModelDescriptor {
        ModelDescriptor {
            reference: "fixture/model".to_owned(),
            arch: "fixture".to_owned(),
            layers: layout.len() as u32,
            hidden: 4096,
            heads: kv_heads * 4,
            kv_heads,
            head_dim,
            vocab: 150_000,
            params_total: 1,
            params_active: 1,
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

    fn kv(m: &ModelDescriptor, ctx: u32) -> u64 {
        kv::kv_bytes(m, ctx, 16, 0, false).bytes // fp16, no group overhead, unpaged
    }

    #[test]
    fn kv_uniform_llama31_8b() {
        // Llama-3.1-8B: 32 layers × 8 kv_heads × 128 head_dim, fp16 (2 B/elem).
        // per-token = 2 · 32 · 8 · 128 · 2 = 131_072 B = 128 KiB/token.
        // kv(ctx) = 131_072 · ctx.
        let m = model(8, 128, vec![LayoutClass::Global; 32]);
        let expected = [
            131_072u64 * 1024, // 134_217_728  (128 MiB)
            131_072 * 4096,    // 536_870_912  (512 MiB)
            131_072 * 8192,    // 1_073_741_824 (1 GiB)
            131_072 * 32768,   // 4_294_967_296 (4 GiB)
            131_072 * 131_072, // 17_179_869_184 (16 GiB)
        ];
        for (ctx, want) in CTX_POINTS.iter().zip(expected) {
            assert_eq!(kv(&m, *ctx), want, "llama3.1-8b at ctx={ctx}");
        }
        // 128 KiB/token cross-check.
        assert_eq!(kv(&m, 1) as f64 / 1024.0, 128.0);
    }

    #[test]
    fn kv_uniform_qwen3_8b() {
        // Qwen3-8B: 36 layers × 8 kv_heads × 128 head_dim, fp16.
        // per-token = 2 · 36 · 8 · 128 · 2 = 147_456 B = 144 KiB/token.
        let m = model(8, 128, vec![LayoutClass::Global; 36]);
        let per_token = 147_456u64;
        for ctx in CTX_POINTS {
            assert_eq!(
                kv(&m, ctx),
                per_token * u64::from(ctx),
                "qwen3-8b at ctx={ctx}"
            );
        }
        assert_eq!(kv(&m, 1) as f64 / 1024.0, 144.0);
    }

    #[test]
    fn kv_hybrid_gemma_class() {
        // Gemma-class hybrid SWA: 24 layers = 12 global + 12 sliding-window,
        // window W = 4096, 8 kv_heads × 128 head_dim, fp16.
        // per-layer-per-token = 2 · 8 · 128 · 2 = 4096 B.
        // kv(ctx) = 12·4096·ctx  +  12·4096·min(ctx, 4096)
        //         = 49_152·ctx    +  49_152·min(ctx, 4096).
        let mut layout = vec![LayoutClass::Global; 12];
        layout.extend(vec![
            LayoutClass::SlidingWindow {
                window: 4096,
                sinks: 0,
            };
            12
        ]);
        let m = model(8, 128, layout);
        let expect = |ctx: u64| 49_152 * ctx + 49_152 * ctx.min(4096);
        for ctx in CTX_POINTS {
            assert_eq!(
                kv(&m, ctx),
                expect(u64::from(ctx)),
                "gemma-hybrid at ctx={ctx}"
            );
        }
        // The kink at ctx = W: below W both classes grow (slope 2·49_152);
        // above W only the global layers grow (slope 49_152) — the SWA layers
        // are pinned at the window.
        let slope_below = (kv(&m, 4096) - kv(&m, 1024)) / (4096 - 1024);
        let slope_above = (kv(&m, 32768) - kv(&m, 8192)) / (32768 - 8192);
        assert_eq!(slope_below, 2 * 49_152);
        assert_eq!(slope_above, 49_152);
        assert_eq!(
            slope_below,
            2 * slope_above,
            "the kink halves the slope at ctx=W"
        );
    }

    #[test]
    fn kv_mla_deepseek_lineage() {
        // DeepSeek-lineage MLA: 60 layers, one shared latent (c_kv=512,
        // d_rope=64 => 576 elements) per token per layer, NOT per-head K/V.
        // per-layer-per-token = 576 · 2 = 1152 B.  kv(ctx) = 60·1152·ctx = 69_120·ctx.
        let m = model(
            128, // many query heads, but MLA stores the latent, not per-head
            128,
            vec![
                LayoutClass::MlaLatent {
                    c_kv: 512,
                    d_rope: 64
                };
                60
            ],
        );
        let per_token = 69_120u64;
        for ctx in CTX_POINTS {
            assert_eq!(kv(&m, ctx), per_token * u64::from(ctx), "mla at ctx={ctx}");
        }
        // Latent-size storage, not per-head: uniform GQA with 128 kv_heads×128
        // head_dim would bill 2·128·128·2 = 65_536 B/layer/token — 57× more.
        let uniform = model(128, 128, vec![LayoutClass::Global; 60]);
        assert!(kv(&m, 32768) * 50 < kv(&uniform, 32768));
    }

    #[test]
    fn kv_ssm_hybrid_constant_state() {
        // SSM/recurrent hybrid: 2 global (8 kv_heads × 128 head_dim, fp16 =>
        // 4096 B/layer/token) + 2 recurrent (constant 1_000_000 B state each).
        // kv(ctx) = 2·4096·ctx + 2·1_000_000 = 8192·ctx + 2_000_000.
        let mut layout = vec![LayoutClass::Global; 2];
        layout.extend(vec![
            LayoutClass::Recurrent {
                state_bytes: 1_000_000
            };
            2
        ]);
        let m = model(8, 128, layout);
        let expect = |ctx: u64| 8192 * ctx + 2_000_000;
        for ctx in CTX_POINTS {
            assert_eq!(
                kv(&m, ctx),
                expect(u64::from(ctx)),
                "ssm-hybrid at ctx={ctx}"
            );
        }
        // The recurrent contribution is context-invariant: kv(ctx) − 8192·ctx is
        // the same constant at every context.
        for ctx in CTX_POINTS {
            assert_eq!(
                kv(&m, ctx) - 8192 * u64::from(ctx),
                2_000_000,
                "constant state at ctx={ctx}"
            );
        }
    }
}
