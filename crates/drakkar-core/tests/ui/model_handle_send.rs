//! INV-CONFINE: `ModelHandle` is `!Send`, so requiring `Send` must not compile.
use drakkar_core::{MemoryBudget, ModelHandle, Sha256};

fn assert_send<T: Send>() {}

fn main() {
    // ModelHandle carries `PhantomData<*const ()>`, making it `!Send`.
    assert_send::<ModelHandle>();
    // Silence unused-import warnings so the failure is the Send bound, not imports.
    let _ = ModelHandle::new(0, Sha256([0u8; 32]), MemoryBudget::new(0, 0, 0, 0, 0, 0));
}
