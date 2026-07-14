//! `drakkar-mlx-sys` — raw FFI bindings to the `dk_*` C ABI (layer 1).
//!
//! Binds the C ABI exported by the vendored C++ shim ([RFC-0002 D2],
//! [RFC-0010]) via bindgen, and builds/statically links the shim and the pinned
//! MLX core with an embedded metallib. It depends on no workspace crate (DEP6)
//! and exposes raw `dk_*` symbols only. `unsafe` lives here and in `drakkar-mlx`
//! only.
//!
//! This is the build-orchestration stub established by the workspace scaffold
//! (issue #120): there is no shim link yet. The `dk.h` header lands in #162, the
//! CMake shim build in #163, and the generated bindings in #164.
//!
//! [RFC-0002 D2]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0002-stack-selection.md
//! [RFC-0010]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0010-backend-abi.md
