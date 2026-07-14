//! Compile-fail assertions for the confinement and dialect boundaries.
//!
//! - `model_handle_send.rs` asserts [`ModelHandle`] is `!Send` (INV-CONFINE):
//!   code that requires `ModelHandle: Send` must not compile.
//! - `render_target_hidden.rs` asserts `RenderTarget` is not importable without
//!   the `session` feature (INV-DIALECT): a `drakkar-sched`-shaped crate (which
//!   depends on `drakkar-core` with default features) cannot name it.
//!
//! trybuild builds each UI case as its own crate that depends on `drakkar-core`
//! with default features (no `session`), so the dialect boundary is exercised in
//! isolation regardless of workspace feature unification.

#[test]
fn confinement_and_dialect_boundaries_do_not_compile() {
    let t = trybuild::TestCases::new();
    // ModelHandle is `!Send` regardless of features (INV-CONFINE).
    t.compile_fail("tests/ui/model_handle_send.rs");
    // The dialect boundary (INV-DIALECT) is what a `drakkar-sched`-shaped crate
    // sees: `drakkar-core` with default features. With `session` on, the
    // session/render layer is *meant* to see `RenderTarget`, so this case only
    // asserts the boundary when `session` is off.
    #[cfg(not(feature = "session"))]
    t.compile_fail("tests/ui/render_target_hidden.rs");
}
