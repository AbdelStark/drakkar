//! `drakkar-cli` — the CLI framework and composition root (layer 4, RFC-0008).
//!
//! This crate owns command parsing ([`cli`]), the dual human/`--json` output
//! framework ([`output`]), TTY/`NO_COLOR` color gating ([`color`]), and the
//! exit-code path ([`exit`]) (RFC-0008 §1 and §4). The `drakkar` binary is a
//! thin wrapper over [`run_cli`]. It is the only crate that names the backend
//! crates, and only to call their factory functions (DEP4).
//!
//! This is the framework (issue #97): the command tree, global flags, output
//! rendering, and exit-code plumbing. Individual command bodies (run, fit, ls,
//! …) plug into it in their own issues (#98, #99, …).
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cli;
pub mod color;
pub mod config;
pub mod exit;
pub mod output;

use std::process::ExitCode;

use clap::Parser;
use drakkar_core::DkError;

use crate::cli::{Cli, Command};
use crate::output::OutputMode;

/// The binary entry point: parse, dispatch, and translate the result into a
/// process exit code, wrapping the whole thing in a panic guard.
///
/// A caught panic maps to `internal.panic` (exit 6), never code 1 (CLI15; the
/// human/backtrace rendering is #116). A usage error (unknown flag/arg) is
/// handled by `clap`, which exits with code 2 before [`run`] returns.
#[must_use]
pub fn run_cli() -> ExitCode {
    let code = match std::panic::catch_unwind(run) {
        Ok(result) => exit::exit_code(&result),
        Err(_) => {
            eprintln!(
                "drakkar: internal error (panic). This is a bug — re-run with --verbose and report it."
            );
            exit::panic_exit_code()
        }
    };
    ExitCode::from(code)
}

/// Parse the command line, resolve the output mode and color gate, and dispatch.
///
/// # Errors
/// Returns the command's [`DkError`], whose category determines the exit code.
pub fn run() -> Result<(), DkError> {
    let cli = Cli::parse();
    let mode = OutputMode::from_flags(&cli.globals);
    let color = color::enabled();
    dispatch(&cli, mode, color)
}

/// Dispatch a parsed command. Command bodies land in their own issues; until
/// then each command is a wired no-op that reports the milestone status on
/// stderr (never on stdout, so `--json` stays clean).
fn dispatch(cli: &Cli, mode: OutputMode, color: bool) -> Result<(), DkError> {
    // Implemented command bodies dispatch here; the rest fall through to the
    // wired no-op below until their own issues land.
    if let Command::Config { action } = &cli.command {
        return config::run(action, mode, color);
    }
    let name = match &cli.command {
        Command::Run { .. } => "run",
        Command::Pull { .. } => "pull",
        Command::Fit { .. } => "fit",
        Command::Ls => "ls",
        Command::Rm { .. } => "rm",
        Command::Prune => "prune",
        Command::Doctor { .. } => "doctor",
        Command::Serve { .. } => "serve",
        Command::Config { .. } => "config",
        Command::Ps => "ps",
        Command::Bench { .. } => "bench",
        Command::Convert { .. } => "convert",
    };
    if !cli.globals.quiet {
        eprintln!("drakkar: `{name}` is not yet implemented in this build.");
    }
    Ok(())
}
