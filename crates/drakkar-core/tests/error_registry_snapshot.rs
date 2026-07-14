//! Golden tuple snapshot for the error-code registry (error-model §1/§4,
//! RFC-0011 Testing Strategy, INV-SINGLE-TAXONOMY).
//!
//! The registry (`ErrorCode`) and the normative §4 table can never drift:
//!
//! - **Compile-time completeness.** `ErrorCode::category` and
//!   `error::mapping::http_status` are exhaustive matches with no wildcard arm,
//!   so adding a variant without mapping it fails to compile. (This cannot be a
//!   trybuild UI test because the variant would have to be added inside
//!   `drakkar-core` itself; it is enforced by the exhaustive matches and noted
//!   here.)
//! - **Value stability.** This test writes each variant's
//!   `(as_str, category, exit, http)` tuple and asserts the produced set equals
//!   the committed golden snapshot. Mutating any tuple (e.g. a code's HTTP
//!   status) fails the assertion with a diff.
//!
//! Regenerate the snapshot after an intentional additive change with
//! `UPDATE_SNAPSHOT=1 cargo test -p drakkar-core --test error_registry_snapshot`.

use drakkar_core::ALL_ERROR_CODES;

fn snapshot_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/error_registry_snapshot.txt")
}

fn produce_snapshot() -> String {
    let mut out = String::from("# code | category | exit | http (drakkar.errors/1)\n");
    for code in ALL_ERROR_CODES {
        out.push_str(&format!(
            "{} | {} | {} | {}\n",
            code.as_str(),
            code.category().as_str(),
            code.exit_code(),
            code.http_status(),
        ));
    }
    out
}

#[test]
fn error_registry_snapshot_matches_golden() {
    let produced = produce_snapshot();
    let path = snapshot_path();

    if std::env::var_os("UPDATE_SNAPSHOT").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &produced).unwrap();
        if std::env::var_os("UPDATE_SNAPSHOT").is_none() {
            panic!(
                "golden snapshot did not exist; wrote {} — re-run to verify",
                path.display()
            );
        }
        return;
    }

    let golden = std::fs::read_to_string(&path).expect("read golden snapshot");
    assert_eq!(
        produced,
        golden,
        "error registry diverged from the committed golden snapshot ({}). \
         If this change is intended and additive, regenerate with UPDATE_SNAPSHOT=1.",
        path.display()
    );
}

#[test]
fn snapshot_covers_all_36_codes() {
    assert_eq!(ALL_ERROR_CODES.len(), 36);
    assert_eq!(produce_snapshot().lines().count(), 37); // header + 36 rows
}
