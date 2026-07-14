//! Single-mapping-site check (RFC-0011 ER2, INV-SINGLE-TAXONOMY).
//!
//! The category→exit and code→HTTP mappings live only in
//! `drakkar-core::error::mapping`. This test scans every crate's `src/` for HTTP
//! status literals outside that module (test code is allow-listed) and fails on
//! any hit, so a status number can never be hardcoded a second time. The CI
//! pipeline (#132) runs this via `cargo test`.

use std::path::{Path, PathBuf};

/// The distinctive HTTP status literals that must appear only in the mapping
/// module. (Exit codes 2–7 are too common to scan for meaningfully; the mapping
/// module is their single site by construction, and the golden snapshot pins
/// every code's exit value.)
const STATUS_CODES: [u16; 8] = [400, 404, 413, 422, 429, 500, 503, 507];

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/drakkar-core; the workspace root is two up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn is_excluded(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // The one allowed mapping site, and test code (which asserts on statuses).
    name == "mapping.rs"
        || name == "tests.rs"
        || path.components().any(|c| c.as_os_str() == "tests")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) != Some("target") {
                collect_rs_files(&path, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Return the status literals appearing as standalone integer tokens on `line`,
/// or an empty vec for comment lines (which never carry mapping logic).
fn status_literals_in_line(line: &str) -> Vec<u16> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with('#') {
        return Vec::new();
    }
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let prev_ident =
                start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
            let next_ident =
                i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_');
            if !prev_ident && !next_ident {
                if let Ok(n) = line[start..i].parse::<u16>() {
                    if STATUS_CODES.contains(&n) {
                        found.push(n);
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    found
}

#[test]
fn no_http_status_literals_outside_the_mapping_module() {
    let root = workspace_root();
    let mut files = Vec::new();
    for crate_dir in std::fs::read_dir(root.join("crates"))
        .expect("crates dir")
        .flatten()
    {
        collect_rs_files(&crate_dir.path().join("src"), &mut files);
    }
    assert!(!files.is_empty(), "found no source files to scan");

    let mut violations = Vec::new();
    for file in &files {
        if is_excluded(file) {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap_or_default();
        for (lineno, line) in text.lines().enumerate() {
            // Test modules are conventionally the trailing `#[cfg(test)] mod`
            // block; their arbitrary integers (token counts, memory sizes) are
            // not HTTP statuses, so stop scanning the file at the first one.
            if line.contains("#[cfg(test)]") {
                break;
            }
            // An explicit escape hatch for a legitimate non-status use of one of
            // these integers (e.g. a duration in ms).
            if line.contains("status-scan-allow") {
                continue;
            }
            for code in status_literals_in_line(line) {
                violations.push(format!("{}:{} -> {}", file.display(), lineno + 1, code));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "HTTP status literals found outside drakkar-core::error::mapping \
         (route them through code.http_status()):\n{}",
        violations.join("\n")
    );
}

#[test]
fn scanner_would_trip_on_a_hardcoded_status() {
    // The fixture that proves the gate has teeth: a status literal used as a
    // value outside the mapping module must be detected.
    assert_eq!(
        status_literals_in_line("        ErrorCode::Foo => 413,"),
        vec![413]
    );
    assert_eq!(status_literals_in_line("    let status = 500;"), vec![500]);
    // Comments and identifiers with digits must not trip it.
    assert!(status_literals_in_line("// see RFC-0007 status 429").is_empty());
    assert!(status_literals_in_line("let as10422 = foo;").is_empty());
    assert!(status_literals_in_line("let x = 42;").is_empty());
}
