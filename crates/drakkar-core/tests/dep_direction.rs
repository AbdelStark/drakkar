//! Dependency-direction gate (architecture §3.1 DEP1–DEP7, RFC-0002 D1).
//!
//! The strict crate layering is a review contract that must be mechanically
//! enforced: a PR introducing a forbidden edge fails here. This reads the live
//! workspace graph via `cargo metadata` and asserts the layer rules; the pure
//! `check_layering` function is unit-tested against synthetic graphs so the
//! gate's teeth are proven without a fixture branch.
//!
//! Run locally with `cargo test -p drakkar-core --test dep_direction`.

use std::collections::BTreeMap;

/// The layer of each workspace crate (architecture §3.1). A crate may depend
/// only on strictly-lower layers, plus `drakkar-core` always.
fn layer(crate_name: &str) -> Option<u8> {
    Some(match crate_name {
        "drakkar-core" | "drakkar-fit" | "drakkar-grammar" => 0,
        "drakkar-engine" | "drakkar-models" | "drakkar-mlx-sys" => 1,
        "drakkar-sched" | "drakkar-mlx" | "drakkar-gguf" => 2,
        "drakkar-server" => 3,
        "drakkar-cli" => 4,
        _ => return None,
    })
}

const BACKEND_CRATES: [&str; 2] = ["drakkar-mlx", "drakkar-gguf"];

/// Return every DEP1–DEP6 violation in a workspace graph (crate → its
/// normal workspace-crate dependencies). Empty means the graph is legal.
fn check_layering(edges: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut violations = Vec::new();
    for (krate, deps) in edges {
        let Some(kl) = layer(krate) else {
            continue;
        };
        // DEP2: drakkar-core depends on no workspace crate.
        if krate == "drakkar-core" && !deps.is_empty() {
            violations.push(format!(
                "DEP2: drakkar-core must have no workspace deps, has {deps:?}"
            ));
        }
        // DEP6: drakkar-mlx-sys depends on no workspace crate.
        if krate == "drakkar-mlx-sys" && !deps.is_empty() {
            violations.push(format!(
                "DEP6: drakkar-mlx-sys must have no workspace deps, has {deps:?}"
            ));
        }
        for dep in deps {
            let Some(dl) = layer(dep) else {
                continue;
            };
            // DEP2: drakkar-fit / drakkar-grammar depend on drakkar-core only.
            if (krate == "drakkar-fit" || krate == "drakkar-grammar") && dep != "drakkar-core" {
                violations.push(format!(
                    "DEP2: {krate} may depend on drakkar-core only, not {dep}"
                ));
            }
            // DEP4/DEP3: only the composition root drakkar-cli names a backend crate.
            if BACKEND_CRATES.contains(&dep.as_str()) && krate != "drakkar-cli" {
                violations.push(format!(
                    "DEP4: only drakkar-cli may depend on {dep}; {krate} does"
                ));
            }
            // DEP1: dependencies must sit in a strictly-lower layer, except the
            // always-allowed drakkar-core.
            if dep != "drakkar-core" && dl >= kl {
                violations.push(format!(
                    "DEP1: {krate} (layer {kl}) may not depend on {dep} (layer {dl}) — same or higher layer"
                ));
            }
        }
    }
    violations
}

/// Parse the live workspace graph: crate → its normal (non-dev, non-build)
/// workspace-crate dependencies, via `cargo metadata`.
fn workspace_edges() -> BTreeMap<String, Vec<String>> {
    let output = std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run cargo metadata");
    assert!(output.status.success(), "cargo metadata failed");
    let meta: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid metadata");

    let members: std::collections::BTreeSet<String> = meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_owned())
        .collect();

    let mut edges = BTreeMap::new();
    for pkg in meta["packages"].as_array().unwrap() {
        let name = pkg["name"].as_str().unwrap().to_owned();
        let mut deps = Vec::new();
        for dep in pkg["dependencies"].as_array().unwrap() {
            // Normal deps only (skip dev/build).
            let kind = dep["kind"].as_str(); // null for normal, "dev"/"build" otherwise
            let dep_name = dep["name"].as_str().unwrap();
            if kind.is_none() && members.contains(dep_name) {
                deps.push(dep_name.to_owned());
            }
        }
        deps.sort();
        deps.dedup();
        edges.insert(name, deps);
    }
    edges
}

#[test]
fn workspace_layering_is_legal() {
    let edges = workspace_edges();
    // Sanity: all eleven crates are present.
    assert_eq!(
        edges.len(),
        11,
        "expected 11 workspace crates, found {}",
        edges.len()
    );
    let violations = check_layering(&edges);
    assert!(
        violations.is_empty(),
        "dependency-direction violations (architecture §3.1 DEP1–DEP7):\n{}",
        violations.join("\n")
    );
}

#[test]
fn gate_trips_on_a_backend_to_engine_edge() {
    // drakkar-engine depending on a backend crate is a DEP3/DEP4 violation.
    let mut edges = BTreeMap::new();
    edges.insert("drakkar-engine".to_owned(), vec!["drakkar-mlx".to_owned()]);
    let v = check_layering(&edges);
    assert!(
        v.iter().any(|s| s.contains("DEP4")),
        "should flag the backend edge: {v:?}"
    );
}

#[test]
fn gate_trips_on_a_same_layer_edge() {
    // drakkar-sched depending on drakkar-mlx (both layer 2) is a DEP1 violation.
    let mut edges = BTreeMap::new();
    edges.insert("drakkar-sched".to_owned(), vec!["drakkar-mlx".to_owned()]);
    let v = check_layering(&edges);
    assert!(
        v.iter().any(|s| s.contains("DEP1") || s.contains("DEP4")),
        "should flag: {v:?}"
    );
}

#[test]
fn gate_trips_on_core_gaining_a_dep_and_fit_widening() {
    let mut edges = BTreeMap::new();
    edges.insert("drakkar-core".to_owned(), vec!["drakkar-fit".to_owned()]);
    edges.insert(
        "drakkar-fit".to_owned(),
        vec!["drakkar-core".to_owned(), "drakkar-engine".to_owned()],
    );
    let v = check_layering(&edges);
    assert!(
        v.iter()
            .any(|s| s.contains("DEP2") && s.contains("drakkar-core")),
        "core dep: {v:?}"
    );
    assert!(
        v.iter()
            .any(|s| s.contains("DEP2") && s.contains("drakkar-fit")),
        "fit widening: {v:?}"
    );
}

#[test]
fn legal_edges_produce_no_violations() {
    let mut edges = BTreeMap::new();
    edges.insert("drakkar-core".to_owned(), vec![]);
    edges.insert("drakkar-fit".to_owned(), vec!["drakkar-core".to_owned()]);
    edges.insert(
        "drakkar-engine".to_owned(),
        vec!["drakkar-core".to_owned(), "drakkar-fit".to_owned()],
    );
    edges.insert(
        "drakkar-cli".to_owned(),
        vec!["drakkar-core".to_owned(), "drakkar-mlx".to_owned()],
    );
    edges.insert("drakkar-mlx-sys".to_owned(), vec![]);
    assert!(check_layering(&edges).is_empty());
}
