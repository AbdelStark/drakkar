//! The `drakkar` binary — a thin wrapper over the CLI framework in the
//! `drakkar-cli` library ([`drakkar_cli::run_cli`]).
#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    drakkar_cli::run_cli()
}
