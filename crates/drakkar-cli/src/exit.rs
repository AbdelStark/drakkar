//! Exit-code plumbing (RFC-0008 CLI8, public-API §2.3).
//!
//! A command returns `Result<(), DkError>`; the process exit code is the CLI8
//! code carried by the error's category (owned by `drakkar-core::error::mapping`,
//! RFC-0011 ER2 — this crate consumes it, never re-maps it). Success is 0. Code
//! 1 is never emitted intentionally; a caught panic maps to `internal.panic`
//! (exit 6), so an observed exit 1 always indicates a defect in the panic
//! wrapper.

use drakkar_core::{DkError, ErrorCode};

/// The process exit code for a command result: 0 on success, else the error's
/// CLI8 exit code (2/3/4/5/6/7). Never 1.
#[must_use]
pub fn exit_code(result: &Result<(), DkError>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(err) => err.exit_code(),
    }
}

/// The exit code for a caught panic: `internal.panic` (6). The panic wrapper's
/// human/backtrace rendering (CLI15) is a separate concern (#116).
#[must_use]
pub fn panic_exit_code() -> u8 {
    ErrorCode::InternalPanic.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;
    use drakkar_core::ErrorContext;

    fn err(code: ErrorCode) -> DkError {
        DkError::new(code, "test").with_context(ErrorContext::new())
    }

    #[test]
    fn success_is_zero() {
        assert_eq!(exit_code(&Ok(())), 0);
    }

    #[test]
    fn each_category_maps_to_its_cli8_code() {
        assert_eq!(exit_code(&Err(err(ErrorCode::CliInvalidArgs))), 2); // usage
        assert_eq!(exit_code(&Err(err(ErrorCode::ModelsNotFound))), 3); // model_not_found
        assert_eq!(exit_code(&Err(err(ErrorCode::FitWontFit))), 4); // infeasible
        assert_eq!(exit_code(&Err(err(ErrorCode::DownloadHubUnreachable))), 5); // network
        assert_eq!(
            exit_code(&Err(err(ErrorCode::ModelsUnsupportedArchitecture))),
            6
        ); // format
        assert_eq!(exit_code(&Err(err(ErrorCode::EngineLoadFailed))), 6); // engine
        assert_eq!(exit_code(&Err(err(ErrorCode::DownloadNoSpace))), 7); // disk
        assert_eq!(exit_code(&Err(err(ErrorCode::InternalPanic))), 6); // internal
    }

    #[test]
    fn code_one_is_never_emitted() {
        for result in [
            Ok(()),
            Err(err(ErrorCode::CliInvalidArgs)),
            Err(err(ErrorCode::FitWontFit)),
            Err(err(ErrorCode::InternalPanic)),
        ] {
            assert_ne!(exit_code(&result), 1);
        }
        assert_ne!(panic_exit_code(), 1);
        assert_eq!(panic_exit_code(), 6);
    }
}
