//! The single, total mapping site (RFC-0011 ER2, error-model §2/§3).
//!
//! Category → CLI exit code and [`ErrorCode`] → HTTP status live **only** here,
//! as exhaustive matches with no wildcard arm, so a new [`ErrorCode`] or
//! [`ErrorCategory`] variant fails to compile until it is mapped
//! (INV-SINGLE-TAXONOMY). No other crate re-derives these mappings.

use super::{ErrorCategory, ErrorCode};

/// The CLI exit code for a category (error-model §2). Exit 1 is deliberately
/// never emitted; it is reserved for "the process did not run to a
/// DRAKKAR-controlled conclusion".
#[must_use]
pub const fn exit_code(category: ErrorCategory) -> u8 {
    match category {
        ErrorCategory::Usage => 2,
        ErrorCategory::ModelNotFound => 3,
        ErrorCategory::Infeasible => 4,
        ErrorCategory::Network => 5,
        ErrorCategory::Format => 6,
        ErrorCategory::Engine => 6,
        ErrorCategory::Disk => 7,
        ErrorCategory::Internal => 6,
    }
}

/// The default HTTP status for a category (error-model §2).
#[must_use]
pub const fn http_default(category: ErrorCategory) -> u16 {
    match category {
        ErrorCategory::Usage => 400,
        ErrorCategory::ModelNotFound => 404,
        ErrorCategory::Infeasible => 422,
        ErrorCategory::Network => 503,
        ErrorCategory::Format => 422,
        ErrorCategory::Engine => 500,
        ErrorCategory::Disk => 507,
        ErrorCategory::Internal => 500,
    }
}

/// The HTTP status for a code: the category default plus the four registered
/// per-code overrides (error-model §2/§3).
#[must_use]
pub const fn http_status(code: ErrorCode) -> u16 {
    match code {
        // The four per-code overrides (error-model §2 "Per-code HTTP overrides").
        ErrorCode::FitContextExceeded => 413,
        ErrorCode::KvPoolExhausted => 429,
        ErrorCode::GrammarSchemaCompileFailed => 422,
        ErrorCode::ServerModelLoading => 503,
        // Everything else takes its category default.
        ErrorCode::CliInvalidArgs
        | ErrorCode::CliMissingModelArg
        | ErrorCode::ConfigInvalidKey
        | ErrorCode::ConfigInvalidValue
        | ErrorCode::ModelsNotFound
        | ErrorCode::ModelsNotInstalled
        | ErrorCode::ModelsRepoNotFound
        | ErrorCode::ModelsGatedRepoNoToken
        | ErrorCode::ModelsUnsupportedArchitecture
        | ErrorCode::ModelsPickleRejected
        | ErrorCode::DownloadNetworkFailed
        | ErrorCode::DownloadHubUnreachable
        | ErrorCode::DownloadIntegrityMismatch
        | ErrorCode::DownloadNoSpace
        | ErrorCode::StoreWriteFailed
        | ErrorCode::StoreCorruptBlob
        | ErrorCode::FitWontFit
        | ErrorCode::ServerUnsupportedField
        | ErrorCode::EngineLoadFailed
        | ErrorCode::EngineMetalInitFailed
        | ErrorCode::EngineInferenceFailed
        | ErrorCode::BackendMetalFault
        | ErrorCode::BackendCapabilityAbsent
        | ErrorCode::BackendIo
        | ErrorCode::AbiVersionMismatch
        | ErrorCode::AbiStructSizeMismatch
        | ErrorCode::AbiThreadViolation
        | ErrorCode::AbiInvalidArgument
        | ErrorCode::InternalPanic
        | ErrorCode::InternalInvariant
        | ErrorCode::InternalBudgetBreach => http_default(code.category()),
    }
}
