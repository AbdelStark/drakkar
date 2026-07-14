//! The error taxonomy (`drakkar.errors/1`, error-model §1–§8, RFC-0011).
//!
//! One flat [`DkError`] type for the whole workspace: its failure class lives in
//! the closed [`ErrorCategory`], and the specific error in the closed
//! [`ErrorCode`] enum, which **is** the normative registry (error-model §4). An
//! unregistered code is unrepresentable — the compiler cannot construct it
//! (INV-SINGLE-TAXONOMY). The category→exit and code→HTTP mappings live in one
//! place, [`mapping`], as exhaustive matches (RFC-0011 ER2).

use std::collections::BTreeMap;
use std::fmt;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

use crate::ids::SchemaTag;

pub mod mapping;

/// The schema tag of a serialized error *object* (error-model §8). Distinct from
/// `drakkar.errors/1`, the registry-contract version, which never appears in a
/// `schema` field.
pub const ERROR_SCHEMA: SchemaTag = SchemaTag::new("drakkar.error/1");

/// The closed failure-class set (error-model §2). Consumers that see an unknown
/// code fall back to this field, which is closed and sufficient to choose a
/// handling strategy (INV-ADDITIVE-REGISTRY).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ErrorCategory {
    /// Malformed or unsupported input.
    Usage,
    /// The model reference does not resolve to anything servable.
    ModelNotFound,
    /// The feasibility engine or admission control rejects the plan/request.
    Infeasible,
    /// Hub or download-path failure.
    Network,
    /// The artifact or its metadata is unusable.
    Format,
    /// Backend/runtime failure.
    Engine,
    /// Local storage failure.
    Disk,
    /// Invariant violations, ABI-boundary faults, caught panics — a DRAKKAR bug.
    Internal,
}

impl ErrorCategory {
    /// The stable dotted string, e.g. `"model_not_found"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCategory::Usage => "usage",
            ErrorCategory::ModelNotFound => "model_not_found",
            ErrorCategory::Infeasible => "infeasible",
            ErrorCategory::Network => "network",
            ErrorCategory::Format => "format",
            ErrorCategory::Engine => "engine",
            ErrorCategory::Disk => "disk",
            ErrorCategory::Internal => "internal",
        }
    }

    /// The CLI exit code for this category (error-model §2).
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        mapping::exit_code(self)
    }

    /// The default HTTP status for this category (error-model §2).
    #[must_use]
    pub const fn http_default(self) -> u16 {
        mapping::http_default(self)
    }
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ErrorCategory {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The closed error-code registry (error-model §4). Each variant renders to a
/// stable `subsystem.snake_case` string via [`ErrorCode::as_str`]; that string
/// is the wire/JSON code. Adding a variant requires a matching §4 row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum ErrorCode {
    /// `cli.invalid_args`
    CliInvalidArgs,
    /// `cli.missing_model_arg`
    CliMissingModelArg,
    /// `config.invalid_key`
    ConfigInvalidKey,
    /// `config.invalid_value`
    ConfigInvalidValue,
    /// `models.not_found`
    ModelsNotFound,
    /// `models.not_installed`
    ModelsNotInstalled,
    /// `models.repo_not_found`
    ModelsRepoNotFound,
    /// `models.gated_repo_no_token`
    ModelsGatedRepoNoToken,
    /// `models.unsupported_architecture`
    ModelsUnsupportedArchitecture,
    /// `models.pickle_rejected`
    ModelsPickleRejected,
    /// `download.network_failed`
    DownloadNetworkFailed,
    /// `download.hub_unreachable`
    DownloadHubUnreachable,
    /// `download.integrity_mismatch`
    DownloadIntegrityMismatch,
    /// `download.no_space`
    DownloadNoSpace,
    /// `store.write_failed`
    StoreWriteFailed,
    /// `store.corrupt_blob`
    StoreCorruptBlob,
    /// `fit.wont_fit`
    FitWontFit,
    /// `fit.context_exceeded`
    FitContextExceeded,
    /// `kv.pool_exhausted`
    KvPoolExhausted,
    /// `grammar.schema_compile_failed`
    GrammarSchemaCompileFailed,
    /// `server.unsupported_field`
    ServerUnsupportedField,
    /// `server.model_loading`
    ServerModelLoading,
    /// `engine.load_failed`
    EngineLoadFailed,
    /// `engine.metal_init_failed`
    EngineMetalInitFailed,
    /// `engine.inference_failed`
    EngineInferenceFailed,
    /// `backend.metal_fault`
    BackendMetalFault,
    /// `backend.capability_absent`
    BackendCapabilityAbsent,
    /// `backend.io`
    BackendIo,
    /// `abi.version_mismatch`
    AbiVersionMismatch,
    /// `abi.struct_size_mismatch`
    AbiStructSizeMismatch,
    /// `abi.thread_violation`
    AbiThreadViolation,
    /// `abi.invalid_argument`
    AbiInvalidArgument,
    /// `internal.panic`
    InternalPanic,
    /// `internal.invariant`
    InternalInvariant,
    /// `internal.budget_breach`
    InternalBudgetBreach,
}

/// Every [`ErrorCode`] variant, in registry order — the iteration source for
/// exhaustiveness tests and the golden tuple snapshot.
pub const ALL_ERROR_CODES: [ErrorCode; 35] = [
    ErrorCode::CliInvalidArgs,
    ErrorCode::CliMissingModelArg,
    ErrorCode::ConfigInvalidKey,
    ErrorCode::ConfigInvalidValue,
    ErrorCode::ModelsNotFound,
    ErrorCode::ModelsNotInstalled,
    ErrorCode::ModelsRepoNotFound,
    ErrorCode::ModelsGatedRepoNoToken,
    ErrorCode::ModelsUnsupportedArchitecture,
    ErrorCode::ModelsPickleRejected,
    ErrorCode::DownloadNetworkFailed,
    ErrorCode::DownloadHubUnreachable,
    ErrorCode::DownloadIntegrityMismatch,
    ErrorCode::DownloadNoSpace,
    ErrorCode::StoreWriteFailed,
    ErrorCode::StoreCorruptBlob,
    ErrorCode::FitWontFit,
    ErrorCode::FitContextExceeded,
    ErrorCode::KvPoolExhausted,
    ErrorCode::GrammarSchemaCompileFailed,
    ErrorCode::ServerUnsupportedField,
    ErrorCode::ServerModelLoading,
    ErrorCode::EngineLoadFailed,
    ErrorCode::EngineMetalInitFailed,
    ErrorCode::EngineInferenceFailed,
    ErrorCode::BackendMetalFault,
    ErrorCode::BackendCapabilityAbsent,
    ErrorCode::BackendIo,
    ErrorCode::AbiVersionMismatch,
    ErrorCode::AbiStructSizeMismatch,
    ErrorCode::AbiThreadViolation,
    ErrorCode::AbiInvalidArgument,
    ErrorCode::InternalPanic,
    ErrorCode::InternalInvariant,
    ErrorCode::InternalBudgetBreach,
];

impl ErrorCode {
    /// The stable dotted code string, e.g. `"kv.pool_exhausted"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::CliInvalidArgs => "cli.invalid_args",
            ErrorCode::CliMissingModelArg => "cli.missing_model_arg",
            ErrorCode::ConfigInvalidKey => "config.invalid_key",
            ErrorCode::ConfigInvalidValue => "config.invalid_value",
            ErrorCode::ModelsNotFound => "models.not_found",
            ErrorCode::ModelsNotInstalled => "models.not_installed",
            ErrorCode::ModelsRepoNotFound => "models.repo_not_found",
            ErrorCode::ModelsGatedRepoNoToken => "models.gated_repo_no_token",
            ErrorCode::ModelsUnsupportedArchitecture => "models.unsupported_architecture",
            ErrorCode::ModelsPickleRejected => "models.pickle_rejected",
            ErrorCode::DownloadNetworkFailed => "download.network_failed",
            ErrorCode::DownloadHubUnreachable => "download.hub_unreachable",
            ErrorCode::DownloadIntegrityMismatch => "download.integrity_mismatch",
            ErrorCode::DownloadNoSpace => "download.no_space",
            ErrorCode::StoreWriteFailed => "store.write_failed",
            ErrorCode::StoreCorruptBlob => "store.corrupt_blob",
            ErrorCode::FitWontFit => "fit.wont_fit",
            ErrorCode::FitContextExceeded => "fit.context_exceeded",
            ErrorCode::KvPoolExhausted => "kv.pool_exhausted",
            ErrorCode::GrammarSchemaCompileFailed => "grammar.schema_compile_failed",
            ErrorCode::ServerUnsupportedField => "server.unsupported_field",
            ErrorCode::ServerModelLoading => "server.model_loading",
            ErrorCode::EngineLoadFailed => "engine.load_failed",
            ErrorCode::EngineMetalInitFailed => "engine.metal_init_failed",
            ErrorCode::EngineInferenceFailed => "engine.inference_failed",
            ErrorCode::BackendMetalFault => "backend.metal_fault",
            ErrorCode::BackendCapabilityAbsent => "backend.capability_absent",
            ErrorCode::BackendIo => "backend.io",
            ErrorCode::AbiVersionMismatch => "abi.version_mismatch",
            ErrorCode::AbiStructSizeMismatch => "abi.struct_size_mismatch",
            ErrorCode::AbiThreadViolation => "abi.thread_violation",
            ErrorCode::AbiInvalidArgument => "abi.invalid_argument",
            ErrorCode::InternalPanic => "internal.panic",
            ErrorCode::InternalInvariant => "internal.invariant",
            ErrorCode::InternalBudgetBreach => "internal.budget_breach",
        }
    }

    /// Parse a dotted code string back into an [`ErrorCode`], or `None` if it is
    /// not a registered code.
    #[must_use]
    pub fn from_code_str(s: &str) -> Option<ErrorCode> {
        ALL_ERROR_CODES.iter().copied().find(|c| c.as_str() == s)
    }

    /// The failure class of this code (error-model §4).
    #[must_use]
    pub const fn category(self) -> ErrorCategory {
        match self {
            ErrorCode::CliInvalidArgs
            | ErrorCode::CliMissingModelArg
            | ErrorCode::ConfigInvalidKey
            | ErrorCode::ConfigInvalidValue
            | ErrorCode::GrammarSchemaCompileFailed
            | ErrorCode::ServerUnsupportedField => ErrorCategory::Usage,
            ErrorCode::ModelsNotFound
            | ErrorCode::ModelsNotInstalled
            | ErrorCode::ModelsRepoNotFound
            | ErrorCode::ModelsGatedRepoNoToken => ErrorCategory::ModelNotFound,
            ErrorCode::FitWontFit | ErrorCode::FitContextExceeded | ErrorCode::KvPoolExhausted => {
                ErrorCategory::Infeasible
            }
            ErrorCode::DownloadNetworkFailed | ErrorCode::DownloadHubUnreachable => {
                ErrorCategory::Network
            }
            ErrorCode::ModelsUnsupportedArchitecture
            | ErrorCode::ModelsPickleRejected
            | ErrorCode::DownloadIntegrityMismatch
            | ErrorCode::StoreCorruptBlob => ErrorCategory::Format,
            ErrorCode::ServerModelLoading
            | ErrorCode::EngineLoadFailed
            | ErrorCode::EngineMetalInitFailed
            | ErrorCode::EngineInferenceFailed
            | ErrorCode::BackendMetalFault
            | ErrorCode::BackendCapabilityAbsent
            | ErrorCode::BackendIo => ErrorCategory::Engine,
            ErrorCode::DownloadNoSpace | ErrorCode::StoreWriteFailed => ErrorCategory::Disk,
            ErrorCode::AbiVersionMismatch
            | ErrorCode::AbiStructSizeMismatch
            | ErrorCode::AbiThreadViolation
            | ErrorCode::AbiInvalidArgument
            | ErrorCode::InternalPanic
            | ErrorCode::InternalInvariant
            | ErrorCode::InternalBudgetBreach => ErrorCategory::Internal,
        }
    }

    /// The CLI exit code for this code (via its category, error-model §2).
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        mapping::exit_code(self.category())
    }

    /// The HTTP status for this code (category default + per-code override).
    #[must_use]
    pub const fn http_status(self) -> u16 {
        mapping::http_status(self)
    }

    /// The retry class pinned by the registry for this code (error-model §4).
    /// For the one `after` code (`kv.pool_exhausted`) the concrete `after_ms` is
    /// filled from `context` by the constructing subsystem; the default carries
    /// `0`.
    #[must_use]
    pub const fn default_retry(self) -> Retry {
        match self {
            ErrorCode::DownloadNetworkFailed
            | ErrorCode::DownloadHubUnreachable
            | ErrorCode::ServerModelLoading => Retry::AfterBackoff,
            ErrorCode::KvPoolExhausted => Retry::After { after_ms: 0 },
            _ => Retry::Terminal,
        }
    }

    /// Whether this code is exempt from carrying a specific actionable remedy
    /// (the `internal.*` codes, whose remedy is always the bug-report
    /// instruction — INV-REMEDY-ALWAYS).
    #[must_use]
    pub const fn remedy_exempt(self) -> bool {
        matches!(
            self,
            ErrorCode::InternalPanic
                | ErrorCode::InternalInvariant
                | ErrorCode::InternalBudgetBreach
        )
    }

    /// The remedy template bound to this code, or `None` for the
    /// `remedy_exempt` `internal.*` codes (error-model §4, RFC-0011 ER7).
    #[must_use]
    pub fn remedy_template(self) -> Option<&'static RemedyTemplate> {
        remedy::template_for(self)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The retry semantics of an error (error-model §5.1/§6). Serializes as
/// `{ "kind": "terminal|after_backoff|after", "after_ms": <n|null> }`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Retry {
    /// Resubmitting the identical request fails identically until the remedy is
    /// acted on.
    Terminal,
    /// Transient; retry with client-chosen exponential backoff and jitter.
    AfterBackoff,
    /// A computed delay; retrying earlier is permitted but wasteful.
    After {
        /// The suggested delay in milliseconds.
        after_ms: u64,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RetryKind {
    Terminal,
    AfterBackoff,
    After,
}

#[derive(Serialize, Deserialize)]
struct RetryWire {
    kind: RetryKind,
    after_ms: Option<u64>,
}

impl From<Retry> for RetryWire {
    fn from(r: Retry) -> Self {
        match r {
            Retry::Terminal => RetryWire {
                kind: RetryKind::Terminal,
                after_ms: None,
            },
            Retry::AfterBackoff => RetryWire {
                kind: RetryKind::AfterBackoff,
                after_ms: None,
            },
            Retry::After { after_ms } => RetryWire {
                kind: RetryKind::After,
                after_ms: Some(after_ms),
            },
        }
    }
}

impl From<RetryWire> for Retry {
    fn from(w: RetryWire) -> Self {
        match w.kind {
            RetryKind::Terminal => Retry::Terminal,
            RetryKind::AfterBackoff => Retry::AfterBackoff,
            RetryKind::After => Retry::After {
                after_ms: w.after_ms.unwrap_or(0),
            },
        }
    }
}

impl Serialize for Retry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RetryWire::from(*self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Retry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        RetryWire::deserialize(deserializer).map(Retry::from)
    }
}

/// A JSON-native scalar value carried in [`ErrorContext`]. Finite and free of
/// user content (INV-NO-SECRETS is the caller's responsibility).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContextValue {
    /// A string value (a file path, model id, or reference — never user content
    /// or secrets).
    Str(String),
    /// A signed integer value.
    Int(i64),
    /// A floating-point value.
    Float(f64),
    /// A boolean value.
    Bool(bool),
}

impl ContextValue {
    /// Render the value for placeholder substitution (a string without quotes;
    /// numbers and booleans in their natural form).
    #[must_use]
    pub fn to_display(&self) -> String {
        match self {
            ContextValue::Str(s) => s.clone(),
            ContextValue::Int(n) => n.to_string(),
            ContextValue::Float(x) => x.to_string(),
            ContextValue::Bool(b) => b.to_string(),
        }
    }
}

/// Typed key/value context for an error (error-model §5.1). Per-code keys are
/// additive-only (INV-ADDITIVE-REGISTRY). Serializes as a JSON object.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ErrorContext(BTreeMap<String, ContextValue>);

impl ErrorContext {
    /// An empty context.
    #[must_use]
    pub fn new() -> Self {
        ErrorContext(BTreeMap::new())
    }

    /// Insert a key/value pair, returning `self` for chaining.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: ContextValue) -> Self {
        self.0.insert(key.into(), value);
        self
    }

    /// Insert a string value.
    #[must_use]
    pub fn with_str(self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.with(key, ContextValue::Str(value.into()))
    }

    /// Insert an integer value.
    #[must_use]
    pub fn with_int(self, key: impl Into<String>, value: i64) -> Self {
        self.with(key, ContextValue::Int(value))
    }

    /// Insert a floating-point value.
    #[must_use]
    pub fn with_float(self, key: impl Into<String>, value: f64) -> Self {
        self.with(key, ContextValue::Float(value))
    }

    /// Look up a value by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&ContextValue> {
        self.0.get(key)
    }

    /// Whether the context is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

pub mod remedy;

pub use remedy::{Remedy, RemedyTemplate};

/// The one flat error type for the whole workspace (error-model §5.1). Its
/// failure class lives in `category` and the specific error in `code`; there is
/// no per-subsystem sub-error hierarchy.
#[derive(Debug)]
pub struct DkError {
    /// The registered code (the registry §4 IS this enum).
    pub code: ErrorCode,
    /// The failure class (derived from `code`).
    pub category: ErrorCategory,
    /// A human message in domain terms, with no secrets (INV-NO-SECRETS).
    pub message: String,
    /// The rendered remedy; `None` only pre-render or for `remedy_exempt` codes.
    pub remedy: Option<Remedy>,
    /// The retry semantics (the registry pins the class per code).
    pub retry: Retry,
    /// Typed per-code context.
    pub context: ErrorContext,
    /// The cause chain; logged at origin, `--verbose` only, never serialized.
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl DkError {
    /// Construct an error for `code` with `message`, deriving the category and
    /// the pinned retry class from the code, with no remedy or context yet.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        DkError {
            code,
            category: code.category(),
            message: message.into(),
            remedy: None,
            retry: code.default_retry(),
            context: ErrorContext::new(),
            source: None,
        }
    }

    /// Attach context (also rendering the code's remedy template against it when
    /// the code is not `remedy_exempt`).
    #[must_use]
    pub fn with_context(mut self, context: ErrorContext) -> Self {
        if let Some(template) = self.code.remedy_template() {
            self.remedy = Some(template.render(&context));
        }
        self.context = context;
        self
    }

    /// Override the retry semantics (used to fill the concrete `after_ms` for
    /// the `after` codes).
    #[must_use]
    pub fn with_retry(mut self, retry: Retry) -> Self {
        self.retry = retry;
        self
    }

    /// Attach a cause chain (logged/`--verbose` only, never serialized).
    #[must_use]
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// The registered code.
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// The failure class.
    #[must_use]
    pub fn category(&self) -> ErrorCategory {
        self.category
    }

    /// The CLI exit code (error-model §2).
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        self.code.exit_code()
    }

    /// The HTTP status (error-model §2/§3).
    #[must_use]
    pub fn http_status(&self) -> u16 {
        self.code.http_status()
    }

    /// The rendered remedy, if any.
    #[must_use]
    pub fn remedy(&self) -> Option<&Remedy> {
        self.remedy.as_ref()
    }
}

impl fmt::Display for DkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for DkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl Serialize for DkError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The `drakkar.error/1` object (error-model §5.2). `source` is never
        // serialized; `http_status` is not part of the CLI envelope.
        let mut map = serializer.serialize_map(Some(8))?;
        map.serialize_entry("schema", ERROR_SCHEMA.0)?;
        map.serialize_entry("code", self.code.as_str())?;
        map.serialize_entry("category", self.category.as_str())?;
        map.serialize_entry("message", &self.message)?;
        map.serialize_entry("remedy", &self.remedy)?;
        map.serialize_entry("retry", &self.retry)?;
        map.serialize_entry("context", &self.context)?;
        map.serialize_entry("exit_code", &self.exit_code())?;
        map.end()
    }
}

#[derive(Deserialize)]
struct DkErrorWire {
    code: String,
    message: String,
    #[serde(default)]
    remedy: Option<Remedy>,
    retry: Retry,
    #[serde(default)]
    context: ErrorContext,
}

impl<'de> Deserialize<'de> for DkError {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = DkErrorWire::deserialize(deserializer)?;
        let code = ErrorCode::from_code_str(&wire.code).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown error code {:?}", wire.code))
        })?;
        Ok(DkError {
            code,
            category: code.category(),
            message: wire.message,
            remedy: wire.remedy,
            retry: wire.retry,
            context: wire.context,
            source: None,
        })
    }
}

#[cfg(test)]
mod tests;
