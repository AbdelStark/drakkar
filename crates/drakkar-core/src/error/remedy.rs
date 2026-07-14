//! Remedy templates and rendering (error-model §4/§5.1, RFC-0011 ER7).
//!
//! Every code that is not `remedy_exempt` (i.e. not an `internal.*` code) binds
//! one [`RemedyTemplate`]: a stable id plus a format string whose `{key}`
//! placeholders are filled from an [`ErrorContext`] (INV-REMEDY-ALWAYS). The
//! rendered result is a single line and, wherever possible, a runnable command.

use serde::{Deserialize, Serialize};

use super::{ErrorCode, ErrorContext};

/// A registered remedy template: a stable id and its `{placeholder}` format.
#[derive(Clone, Copy, Debug)]
pub struct RemedyTemplate {
    /// The stable template id, e.g. `"run_sibling"`.
    pub id: &'static str,
    /// The format string with `{key}` placeholders filled from context.
    pub format: &'static str,
}

impl RemedyTemplate {
    /// Render this template against `params`, substituting each `{key}` for the
    /// matching context value. Braces that do not enclose a known key are left
    /// literal (so remedy text containing JSON such as `{"type":"json_object"}`
    /// is preserved).
    #[must_use]
    pub fn render(&self, params: &ErrorContext) -> Remedy {
        Remedy {
            rendered: render_format(self.format, params),
            template: self.id,
            params: params.clone(),
        }
    }
}

fn render_format(format: &str, params: &ErrorContext) -> String {
    let mut out = String::with_capacity(format.len());
    let mut rest = format;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let key = &after[..close];
                if let Some(value) = params.get(key) {
                    out.push_str(&value.to_display());
                } else {
                    // Not a known placeholder — keep the literal brace.
                    out.push('{');
                    out.push_str(key);
                    out.push('}');
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

macro_rules! template {
    ($id:literal, $fmt:literal) => {
        RemedyTemplate {
            id: $id,
            format: $fmt,
        }
    };
}

// The v0.1 remedy templates, one per non-`internal` code (error-model §4). The
// six ER7 seed ids (`run_sibling`, `retry_after_or_reduce`, `reduce_context`,
// `resume_pull`, `prune_store`, `accept_license`) name their canonical codes.
const CLI_INVALID_ARGS: RemedyTemplate = template!(
    "cli_invalid_args",
    "Run 'drakkar {command} --help' for accepted flags and arguments."
);
const CLI_MISSING_MODEL_ARG: RemedyTemplate = template!(
    "cli_missing_model_arg",
    "This command needs a model reference: 'drakkar {command} <ref>'. 'drakkar ls' lists installed models."
);
const CONFIG_INVALID_KEY: RemedyTemplate = template!(
    "config_invalid_key",
    "Unknown config key '{key}'. 'drakkar config path' shows the file; 'drakkar doctor' lists valid keys."
);
const CONFIG_INVALID_VALUE: RemedyTemplate = template!(
    "config_invalid_value",
    "'{key}' expects {expected}; got '{value}'. 'drakkar config set {key} <value>' validates before writing."
);
const MODELS_NOT_FOUND: RemedyTemplate = template!(
    "models_not_found",
    "'{ref}' does not resolve to a servable model. Check the reference, or 'drakkar ls' for installed models."
);
const MODELS_NOT_INSTALLED: RemedyTemplate = template!(
    "models_not_installed",
    "Model '{model}' is not installed. Run 'drakkar pull {model}', or pick an installed model from GET /v1/models."
);
const MODELS_REPO_NOT_FOUND: RemedyTemplate = template!(
    "models_repo_not_found",
    "No repository '{repo}' on the hub. Check the spelling, or search: https://huggingface.co/models?search={repo}"
);
const MODELS_GATED_REPO_NO_TOKEN: RemedyTemplate = template!(
    "accept_license",
    "{repo} is gated. Accept the license at {acceptance_url}, then provide a token (HF_TOKEN, ~/.huggingface, or keychain; RFC-0006 MP2)."
);
const MODELS_UNSUPPORTED_ARCHITECTURE: RemedyTemplate = template!(
    "models_unsupported_architecture",
    "Architecture '{arch}' is not in the model-def layer of this build. Try a GGUF artifact ('drakkar pull {ref} --format gguf', backend B), or upgrade: architectures are added on a weekly cadence (RFC-0002 D3)."
);
const MODELS_PICKLE_REJECTED: RemedyTemplate = template!(
    "models_pickle_rejected",
    "{file} is a pickle checkpoint; pickle executes code on load and is never accepted (RFC-0001 A11, RFC-0006 MP6). Use a safetensors or GGUF export of this model; conversion guidance: {docs_url}."
);
const DOWNLOAD_NETWORK_FAILED: RemedyTemplate = template!(
    "resume_pull",
    "Download interrupted at {percent}% ({bytes_done} of {bytes_total}). Re-run the same command to resume; completed files are never re-fetched (RFC-0006 MP7)."
);
const DOWNLOAD_HUB_UNREACHABLE: RemedyTemplate = template!(
    "download_hub_unreachable",
    "Could not reach the hub: {cause}. Check connectivity and proxy settings; installed models keep working offline ('drakkar ls')."
);
const DOWNLOAD_INTEGRITY_MISMATCH: RemedyTemplate = template!(
    "download_integrity_mismatch",
    "{file} failed integrity verification (expected {expected}, got {actual}); the blob was discarded (RFC-0006 MP8). Re-run to re-fetch; if it persists, pin a known-good revision with '@{rev}'."
);
const DOWNLOAD_NO_SPACE: RemedyTemplate = template!(
    "prune_store",
    "Needs {needed_gib} GiB on {volume} (download + conversion workspace + output, RFC-0006 MP9); {free_gib} GiB free. 'drakkar prune' can reclaim {reclaimable_gib} GiB, or set storage.path to another volume."
);
const STORE_WRITE_FAILED: RemedyTemplate = template!(
    "store_write_failed",
    "Writing to the model store at {path} failed: {cause}. Check volume health and permissions; the store is reconstructible, so 'drakkar doctor' can verify and repair manifests."
);
const STORE_CORRUPT_BLOB: RemedyTemplate = template!(
    "store_corrupt_blob",
    "Blob {blob} at {path} failed digest verification: its content does not match its name (RFC-0006 MP8, INV-CAS). 'drakkar rm {model}' then re-pull; 'drakkar doctor' quarantines the bad blob."
);
const FIT_WONT_FIT: RemedyTemplate = template!(
    "run_sibling",
    "Needs {needed_gib} GiB even at the floor plan (lowest sane quant, 4k ctx, KV 8-bit); usable budget is {usable_gib} GiB (RFC-0004 FE19). Nearest sibling that fits: 'drakkar run {sibling}'. Override at your own risk with --force."
);
const FIT_CONTEXT_EXCEEDED: RemedyTemplate = template!(
    "reduce_context",
    "prompt + max_tokens = {requested} exceeds the admissible {max_admissible_tokens} at the current KV precision. Reduce the request, or reload with --kv-bits 8 (ctx ceiling per precision: 'drakkar fit {model}')."
);
const KV_POOL_EXHAUSTED: RemedyTemplate = template!(
    "retry_after_or_reduce",
    "KV pool at {pool_occupancy}% with no reclaimable blocks (RFC-0004 FE18). Retry after {retry_after_ms} ms, lower concurrency, or raise the pool via a smaller context ceiling."
);
const GRAMMAR_SCHEMA_COMPILE_FAILED: RemedyTemplate = template!(
    "grammar_schema_compile_failed",
    "The json_schema in response_format does not compile to a grammar: {reason}. Simplify the schema or use {\"type\":\"json_object\"} (RFC-0007 AS10)."
);
const SERVER_UNSUPPORTED_FIELD: RemedyTemplate = template!(
    "server_unsupported_field",
    "Remove '{field}' or check capabilities via GET /v1/models. DRAKKAR never silently ignores parameters (RFC-0007 AS2)."
);
const SERVER_MODEL_LOADING: RemedyTemplate = template!(
    "server_model_loading",
    "{model} is loading ({progress_percent}%, ~{eta_s} s at current SSD bandwidth). Retry after {retry_after_ms} ms."
);
const ENGINE_LOAD_FAILED: RemedyTemplate = template!(
    "engine_load_failed",
    "Loading {model} failed in the backend: {cause}. 'drakkar doctor' checks the store and environment; 'drakkar rm {model}' then re-pull rules out a damaged artifact."
);
const ENGINE_METAL_INIT_FAILED: RemedyTemplate = template!(
    "engine_metal_init_failed",
    "Metal device initialization failed: {cause}. 'drakkar doctor' reports GPU, macOS ({min_macos}+ required), and wired-limit status."
);
const ENGINE_INFERENCE_FAILED: RemedyTemplate = template!(
    "engine_inference_failed",
    "Generation failed mid-flight: {cause}. The sequence was aborted and its blocks freed. Recurrence on the same input is a bug — report it."
);
const BACKEND_METAL_FAULT: RemedyTemplate = template!(
    "backend_metal_fault",
    "The backend reported a Metal fault: {backend_message} (RFC-0010). 'drakkar doctor' checks GPU and driver state; recurrence on the same input is a bug — report it."
);
const BACKEND_CAPABILITY_ABSENT: RemedyTemplate = template!(
    "backend_capability_absent",
    "The backend lacks a required capability: {capability} (RFC-0010, gated by Capabilities). Upgrade DRAKKAR or choose an artifact this build can run ('drakkar fit {model}')."
);
const BACKEND_IO: RemedyTemplate = template!(
    "backend_io",
    "The backend failed a weight I/O operation on {path}: {backend_message} (RFC-0010). Check volume health; 'drakkar rm {model}' then re-pull rules out a damaged artifact."
);
const ABI_VERSION_MISMATCH: RemedyTemplate = template!(
    "abi_version_mismatch",
    "Backend shim ABI is {found}, this binary expects {expected} (RFC-0010 AB3). The installation is inconsistent — reinstall DRAKKAR (brew reinstall drakkar or re-download)."
);
const ABI_STRUCT_SIZE_MISMATCH: RemedyTemplate = template!(
    "abi_struct_size_mismatch",
    "An ABI struct is larger than the shim understands ({found} > {expected} bytes, RFC-0010 AB13). The installation is inconsistent — reinstall DRAKKAR."
);
const ABI_THREAD_VIOLATION: RemedyTemplate = template!(
    "abi_thread_violation",
    "A backend call crossed the one-thread contract (RFC-0010 AB6). This is a bug — open an issue with the log at {log_path}."
);
const ABI_INVALID_ARGUMENT: RemedyTemplate = template!(
    "abi_invalid_argument",
    "The control plane passed an invalid argument across the ABI (RFC-0010 AB9). This is a bug — open an issue with the log at {log_path}."
);

/// Every registered remedy template, for id-based lookup on deserialize. One per
/// non-`internal.*` code (32 codes); the three `internal.*` codes are exempt.
const ALL_TEMPLATES: [&RemedyTemplate; 32] = [
    &CLI_INVALID_ARGS,
    &CLI_MISSING_MODEL_ARG,
    &CONFIG_INVALID_KEY,
    &CONFIG_INVALID_VALUE,
    &MODELS_NOT_FOUND,
    &MODELS_NOT_INSTALLED,
    &MODELS_REPO_NOT_FOUND,
    &MODELS_GATED_REPO_NO_TOKEN,
    &MODELS_UNSUPPORTED_ARCHITECTURE,
    &MODELS_PICKLE_REJECTED,
    &DOWNLOAD_NETWORK_FAILED,
    &DOWNLOAD_HUB_UNREACHABLE,
    &DOWNLOAD_INTEGRITY_MISMATCH,
    &DOWNLOAD_NO_SPACE,
    &STORE_WRITE_FAILED,
    &STORE_CORRUPT_BLOB,
    &FIT_WONT_FIT,
    &FIT_CONTEXT_EXCEEDED,
    &KV_POOL_EXHAUSTED,
    &GRAMMAR_SCHEMA_COMPILE_FAILED,
    &SERVER_UNSUPPORTED_FIELD,
    &SERVER_MODEL_LOADING,
    &ENGINE_LOAD_FAILED,
    &ENGINE_METAL_INIT_FAILED,
    &ENGINE_INFERENCE_FAILED,
    &BACKEND_METAL_FAULT,
    &BACKEND_CAPABILITY_ABSENT,
    &BACKEND_IO,
    &ABI_VERSION_MISMATCH,
    &ABI_STRUCT_SIZE_MISMATCH,
    &ABI_THREAD_VIOLATION,
    &ABI_INVALID_ARGUMENT,
];

/// The remedy template bound to `code`, or `None` for the `remedy_exempt`
/// `internal.*` codes.
#[must_use]
pub fn template_for(code: ErrorCode) -> Option<&'static RemedyTemplate> {
    Some(match code {
        ErrorCode::CliInvalidArgs => &CLI_INVALID_ARGS,
        ErrorCode::CliMissingModelArg => &CLI_MISSING_MODEL_ARG,
        ErrorCode::ConfigInvalidKey => &CONFIG_INVALID_KEY,
        ErrorCode::ConfigInvalidValue => &CONFIG_INVALID_VALUE,
        ErrorCode::ModelsNotFound => &MODELS_NOT_FOUND,
        ErrorCode::ModelsNotInstalled => &MODELS_NOT_INSTALLED,
        ErrorCode::ModelsRepoNotFound => &MODELS_REPO_NOT_FOUND,
        ErrorCode::ModelsGatedRepoNoToken => &MODELS_GATED_REPO_NO_TOKEN,
        ErrorCode::ModelsUnsupportedArchitecture => &MODELS_UNSUPPORTED_ARCHITECTURE,
        ErrorCode::ModelsPickleRejected => &MODELS_PICKLE_REJECTED,
        ErrorCode::DownloadNetworkFailed => &DOWNLOAD_NETWORK_FAILED,
        ErrorCode::DownloadHubUnreachable => &DOWNLOAD_HUB_UNREACHABLE,
        ErrorCode::DownloadIntegrityMismatch => &DOWNLOAD_INTEGRITY_MISMATCH,
        ErrorCode::DownloadNoSpace => &DOWNLOAD_NO_SPACE,
        ErrorCode::StoreWriteFailed => &STORE_WRITE_FAILED,
        ErrorCode::StoreCorruptBlob => &STORE_CORRUPT_BLOB,
        ErrorCode::FitWontFit => &FIT_WONT_FIT,
        ErrorCode::FitContextExceeded => &FIT_CONTEXT_EXCEEDED,
        ErrorCode::KvPoolExhausted => &KV_POOL_EXHAUSTED,
        ErrorCode::GrammarSchemaCompileFailed => &GRAMMAR_SCHEMA_COMPILE_FAILED,
        ErrorCode::ServerUnsupportedField => &SERVER_UNSUPPORTED_FIELD,
        ErrorCode::ServerModelLoading => &SERVER_MODEL_LOADING,
        ErrorCode::EngineLoadFailed => &ENGINE_LOAD_FAILED,
        ErrorCode::EngineMetalInitFailed => &ENGINE_METAL_INIT_FAILED,
        ErrorCode::EngineInferenceFailed => &ENGINE_INFERENCE_FAILED,
        ErrorCode::BackendMetalFault => &BACKEND_METAL_FAULT,
        ErrorCode::BackendCapabilityAbsent => &BACKEND_CAPABILITY_ABSENT,
        ErrorCode::BackendIo => &BACKEND_IO,
        ErrorCode::AbiVersionMismatch => &ABI_VERSION_MISMATCH,
        ErrorCode::AbiStructSizeMismatch => &ABI_STRUCT_SIZE_MISMATCH,
        ErrorCode::AbiThreadViolation => &ABI_THREAD_VIOLATION,
        ErrorCode::AbiInvalidArgument => &ABI_INVALID_ARGUMENT,
        // `remedy_exempt`: the three `internal.*` codes carry the universal
        // bug-report remedy, not a specific actionable template
        // (INV-REMEDY-ALWAYS).
        ErrorCode::InternalPanic
        | ErrorCode::InternalInvariant
        | ErrorCode::InternalBudgetBreach => return None,
    })
}

fn template_by_id(id: &str) -> Option<&'static RemedyTemplate> {
    ALL_TEMPLATES.iter().copied().find(|t| t.id == id)
}

/// A rendered remedy (error-model §5.1): the single most useful next action,
/// carrying the template id and the params that filled it.
#[derive(Clone, PartialEq, Debug)]
pub struct Remedy {
    /// The single-line, runnable-where-possible rendered remedy.
    pub rendered: String,
    /// The registered template id, e.g. `"run_sibling"`.
    pub template: &'static str,
    /// The typed params that filled the template.
    pub params: ErrorContext,
}

#[derive(Serialize, Deserialize)]
struct RemedyWire {
    rendered: String,
    template: String,
    params: ErrorContext,
}

impl Serialize for Remedy {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RemedyWire {
            rendered: self.rendered.clone(),
            template: self.template.to_owned(),
            params: self.params.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Remedy {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = RemedyWire::deserialize(deserializer)?;
        let template = template_by_id(&wire.template)
            .map(|t| t.id)
            .ok_or_else(|| {
                serde::de::Error::custom(format!("unknown remedy template {:?}", wire.template))
            })?;
        Ok(Remedy {
            rendered: wire.rendered,
            template,
            params: wire.params,
        })
    }
}

/// Whether `code` is one of the six ER7 seed template ids (a documentation
/// anchor; every non-exempt code binds a template).
#[must_use]
pub fn is_seed_template(id: &str) -> bool {
    matches!(
        id,
        "run_sibling"
            | "retry_after_or_reduce"
            | "reduce_context"
            | "resume_pull"
            | "prune_store"
            | "accept_license"
    )
}
