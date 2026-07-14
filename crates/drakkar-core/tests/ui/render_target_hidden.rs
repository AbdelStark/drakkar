//! INV-DIALECT: without the `session` feature, `RenderTarget` is not exported,
//! so a `drakkar-sched`-shaped downstream crate cannot import it.
use drakkar_core::RenderTarget;

fn main() {
    let _ = RenderTarget::Anthropic;
}
