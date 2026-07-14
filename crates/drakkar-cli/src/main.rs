//! `drakkar` — the command-line binary and composition root (layer 4).
//!
//! This crate owns command parsing (`clap`), output rendering (human and
//! `--json` from the same structs, [RFC-0008]), and the wiring of every other
//! workspace crate. It is the only crate that names the backend crates
//! (`drakkar-mlx`, and `drakkar-gguf` behind the `gguf` feature), and only to
//! call their factory functions (DEP4).
//!
//! Skeleton established by the workspace scaffold (issue #120): the clap command
//! tree, global flags, dual rendering, and the exit-code framework land in #97.
//!
//! [RFC-0008]: https://github.com/AbdelStark/drakkar/blob/main/docs/rfcs/RFC-0008-cli-ux.md
#![forbid(unsafe_code)]

fn main() {
    // Command tree, argument parsing, and dispatch land in issue #97.
}
