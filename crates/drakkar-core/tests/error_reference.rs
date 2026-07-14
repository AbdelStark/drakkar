//! Generator + drift check for the public error-code reference page (§4,
//! RFC-0011).
//!
//! The page is generated from the `ErrorCode` registry so docs and code stay in
//! lockstep: a missing or extra code, or any changed row, fails the drift check.
//! Regenerate after an intentional additive change with
//! `UPDATE_DOCS=1 cargo test -p drakkar-core --test error_reference`.

use drakkar_core::{ALL_ERROR_CODES, ErrorCode, Retry};

/// The closed thirteen-prefix set, in the order they appear in the reference.
const PREFIXES: [&str; 13] = [
    "cli", "config", "models", "download", "store", "fit", "kv", "engine", "backend", "abi",
    "grammar", "server", "internal",
];

fn retry_class(code: ErrorCode) -> &'static str {
    match code.default_retry() {
        Retry::Terminal => "terminal",
        Retry::AfterBackoff => "after_backoff",
        Retry::After { .. } => "after",
    }
}

fn remedy_template(code: ErrorCode) -> &'static str {
    match code.remedy_template() {
        Some(t) => t.id,
        // The internal.* codes are remedy-exempt: their remedy is the universal
        // bug-report instruction (INV-REMEDY-ALWAYS).
        None => "bug-report",
    }
}

fn generate() -> String {
    let mut out = String::new();
    out.push_str("# Error code reference (`drakkar.errors/1`)\n\n");
    out.push_str(
        "> Generated from the `ErrorCode` registry in `drakkar-core`. Do not edit by hand;\n\
         > regenerate with `UPDATE_DOCS=1 cargo test -p drakkar-core --test error_reference`.\n\n",
    );
    out.push_str(
        "Every failure DRAKKAR can produce carries one of these stable codes. Codes are\n\
         **append-only and never reused or re-categorized** within registry major 1\n\
         (RV12, INV-ADDITIVE-REGISTRY): a code's category, HTTP status, and exit code never\n\
         change. Consumers that see an unknown code fall back to the `category` field, which\n\
         is closed.\n\n",
    );
    out.push_str("## Exit codes\n\n");
    out.push_str(
        "The CLI exit code is a coarse classifier derived from the category (public-API §2.3,\n\
         RFC-0008 CLI8); the dotted code string is the precise contract. Read it from\n\
         `--json` output rather than scraping messages.\n\n",
    );
    out.push_str("| Category | CLI exit | HTTP default |\n");
    out.push_str("| --- | --- | --- |\n");
    for cat in [
        drakkar_core::ErrorCategory::Usage,
        drakkar_core::ErrorCategory::ModelNotFound,
        drakkar_core::ErrorCategory::Infeasible,
        drakkar_core::ErrorCategory::Network,
        drakkar_core::ErrorCategory::Format,
        drakkar_core::ErrorCategory::Engine,
        drakkar_core::ErrorCategory::Disk,
        drakkar_core::ErrorCategory::Internal,
    ] {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            cat.as_str(),
            cat.exit_code(),
            cat.http_default()
        ));
    }
    out.push_str("\nExit code 1 is deliberately unassigned and never emitted intentionally.\n\n");

    out.push_str("## Codes by subsystem\n\n");
    for prefix in PREFIXES {
        let codes: Vec<ErrorCode> = ALL_ERROR_CODES
            .iter()
            .copied()
            .filter(|c| c.as_str().split_once('.').map(|(p, _)| p) == Some(prefix))
            .collect();
        if codes.is_empty() {
            continue;
        }
        out.push_str(&format!("### `{prefix}`\n\n"));
        out.push_str("| Code | Category | Surfaces | HTTP | Retry | Remedy template |\n");
        out.push_str("| --- | --- | --- | --- | --- | --- |\n");
        for code in codes {
            out.push_str(&format!(
                "| `{}` | `{}` | {} | {} | `{}` | `{}` |\n",
                code.as_str(),
                code.category().as_str(),
                code.surface().as_str(),
                code.http_status(),
                retry_class(code),
                remedy_template(code),
            ));
        }
        out.push('\n');
    }
    out
}

fn page_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/error-codes.md")
}

#[test]
fn error_reference_page_matches_registry() {
    let generated = generate();
    let path = page_path();

    if std::env::var_os("UPDATE_DOCS").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
        if std::env::var_os("UPDATE_DOCS").is_none() {
            panic!(
                "reference page did not exist; wrote {} — re-run",
                path.display()
            );
        }
        return;
    }

    let committed = std::fs::read_to_string(&path).expect("read committed reference");
    assert_eq!(
        generated, committed,
        "docs/reference/error-codes.md is out of sync with the ErrorCode registry. \
         Regenerate with UPDATE_DOCS=1."
    );
}

#[test]
fn reference_covers_every_code_once() {
    let generated = generate();
    for code in ALL_ERROR_CODES {
        let needle = format!("| `{}` |", code.as_str());
        assert_eq!(
            generated.matches(&needle).count(),
            1,
            "{} should appear exactly once",
            code.as_str()
        );
    }
}
