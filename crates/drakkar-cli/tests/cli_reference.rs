//! Generator + drift check for the CLI reference page (RFC-0008, public-API §2).
//!
//! The reference is generated from the same declarative `clap` command tree that
//! backs argument parsing, `--help`, and completions, so it cannot drift: a
//! command or flag added to the tree without regenerating the page fails this
//! test (API2 — reachable-but-undocumented is a release blocker). Regenerate
//! with `UPDATE_DOCS=1 cargo test -p drakkar-cli --test cli_reference`.

use clap::{Arg, Command, CommandFactory};
use drakkar_cli::cli::Cli;

/// Ship milestone and stability per command (public-API §2.1); clap does not
/// carry these, so they live here alongside the generator.
fn milestone_stability(name: &str) -> (&'static str, &'static str) {
    match name {
        "ps" => ("v0.2", "stable"),
        "bench" | "convert" => ("v0.2", "experimental → stable v0.3"),
        _ => ("v0.1", "stable"),
    }
}

fn arg_flag(arg: &Arg) -> String {
    let mut parts = Vec::new();
    if let Some(long) = arg.get_long() {
        parts.push(format!("`--{long}`"));
    }
    if let Some(short) = arg.get_short() {
        parts.push(format!("`-{short}`"));
    }
    if parts.is_empty() {
        // Positional.
        format!("`<{}>`", arg.get_id().as_str().to_uppercase())
    } else {
        parts.join(", ")
    }
}

fn help_text(arg: &Arg) -> String {
    arg.get_help()
        .map(|h| h.to_string())
        .unwrap_or_default()
        .replace('\n', " ")
}

fn synopsis(name: &str, sub: &Command) -> String {
    let mut s = format!("drakkar {name}");
    for arg in sub.get_arguments().filter(|a| a.is_positional()) {
        let id = arg.get_id().as_str();
        if arg.is_required_set() {
            s.push_str(&format!(" <{id}>"));
        } else {
            s.push_str(&format!(" [{id}]"));
        }
    }
    if !sub.get_arguments().all(|a| a.is_positional()) {
        s.push_str(" [OPTIONS]");
    }
    if sub.has_subcommands() {
        s.push_str(" <SUBCOMMAND>");
    }
    s
}

fn render_command(out: &mut String, name: &str, sub: &Command, depth: usize) {
    let heading = "#".repeat(depth);
    out.push_str(&format!("{heading} `{}`\n\n", synopsis(name, sub)));
    if let Some(about) = sub.get_about() {
        out.push_str(&format!("{}\n\n", about.to_string().replace('\n', " ")));
    }
    let (milestone, stability) = milestone_stability(name);
    out.push_str(&format!(
        "- Milestone: {milestone} · Stability: {stability}\n\n"
    ));

    let local: Vec<&Arg> = sub
        .get_arguments()
        .filter(|a| !a.is_global_set() && a.get_id().as_str() != "help")
        .collect();
    if !local.is_empty() {
        out.push_str("| Argument / flag | Description |\n| --- | --- |\n");
        for arg in local {
            out.push_str(&format!("| {} | {} |\n", arg_flag(arg), help_text(arg)));
        }
        out.push('\n');
    }
    for subsub in sub.get_subcommands().filter(|s| s.get_name() != "help") {
        render_command(
            out,
            &format!("{name} {}", subsub.get_name()),
            subsub,
            depth + 1,
        );
    }
}

fn generate() -> String {
    let cmd = Cli::command();
    let mut out = String::new();
    out.push_str("# CLI reference\n\n");
    out.push_str(
        "> Generated from the `drakkar-cli` command tree. Do not edit by hand;\n\
         > regenerate with `UPDATE_DOCS=1 cargo test -p drakkar-cli --test cli_reference`.\n\n",
    );
    if let Some(about) = cmd.get_about() {
        out.push_str(&format!("{about}\n\n"));
    }

    out.push_str("## Global flags\n\n");
    out.push_str(
        "Accepted on every command; stdout carries machine output only under `--json`, \
         logs and progress always go to stderr.\n\n",
    );
    out.push_str("| Flag | Description |\n| --- | --- |\n");
    for arg in cmd.get_arguments().filter(|a| a.is_global_set()) {
        out.push_str(&format!("| {} | {} |\n", arg_flag(arg), help_text(arg)));
    }
    out.push_str(
        "\n`NO_COLOR` (or a non-TTY stdout) disables ANSI. `DRAKKAR_*` environment variables \
         override config file values; precedence is flags > `DRAKKAR_*` env > \
         `~/.config/drakkar/config.toml` > built-in defaults (LD23).\n\n",
    );

    out.push_str("## Exit codes\n\n");
    out.push_str("| Code | Meaning |\n| --- | --- |\n");
    for (code, meaning) in [
        (0, "Success"),
        (2, "Usage error (bad flags/args)"),
        (3, "Model or reference not found"),
        (4, "Won't fit (feasibility failure without `--force`)"),
        (5, "Download/network failure"),
        (
            6,
            "Engine/runtime failure (load, Metal, inference); also the panic wrapper",
        ),
        (7, "Disk/space failure"),
    ] {
        out.push_str(&format!("| {code} | {meaning} |\n"));
    }
    out.push_str("\nCode 1 is never emitted intentionally. See the ");
    out.push_str("[error-code reference](error-codes.md) for the per-code mapping.\n\n");

    out.push_str("## Commands\n\n");
    for sub in cmd.get_subcommands().filter(|s| s.get_name() != "help") {
        render_command(&mut out, sub.get_name(), sub, 3);
    }
    out
}

fn page_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/cli.md")
}

#[test]
fn cli_reference_matches_command_tree() {
    let generated = generate();
    let path = page_path();
    if std::env::var_os("UPDATE_DOCS").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
        if std::env::var_os("UPDATE_DOCS").is_none() {
            panic!(
                "cli reference did not exist; wrote {} — re-run",
                path.display()
            );
        }
        return;
    }
    let committed = std::fs::read_to_string(&path).expect("read committed cli reference");
    assert_eq!(
        generated, committed,
        "docs/reference/cli.md is out of sync with the command tree (API2: a reachable \
         command/flag must be documented). Regenerate with UPDATE_DOCS=1."
    );
}

#[test]
fn every_command_is_documented() {
    let generated = generate();
    for sub in Cli::command().get_subcommands() {
        let name = sub.get_name();
        if name == "help" {
            continue;
        }
        assert!(
            generated.contains(&format!("drakkar {name}")),
            "command `{name}` is missing from the reference"
        );
    }
}
