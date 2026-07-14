//! The dual-rendering output framework (RFC-0008 UX-I2/UX-I3, CLI6).
//!
//! Each command's result struct renders to both a human formatter and a
//! schema-versioned JSON object. Under `--json` exactly one JSON object is
//! written to stdout, `schema` first, and nothing else — logs and progress go
//! to stderr. Serializing the struct directly (not via `serde_json::Value`)
//! preserves field order, so `schema` stays first when it is the struct's first
//! field.

use std::io::{self, Write};

use crate::cli::GlobalFlags;

/// How machine/human output should be rendered, derived from the global flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputMode {
    /// Human-readable output (default).
    Human,
    /// A single JSON object on stdout (`--json`).
    Json,
    /// JSON Lines streaming events (`--stream-json`).
    StreamJson,
}

impl OutputMode {
    /// Resolve the output mode from the global flags (`--stream-json` wins over
    /// `--json`, which wins over the human default).
    #[must_use]
    pub fn from_flags(flags: &GlobalFlags) -> Self {
        if flags.stream_json {
            OutputMode::StreamJson
        } else if flags.json {
            OutputMode::Json
        } else {
            OutputMode::Human
        }
    }
}

/// A command result that can render to both a human string and a
/// schema-versioned JSON object (CLI6). Implementors declare `schema` as their
/// first serialized field so it renders first under `--json`.
pub trait CommandOutput: serde::Serialize {
    /// Render the human-facing form. `color` is `true` when ANSI is enabled.
    fn render_human(&self, color: bool) -> String;
}

/// Write a command's output to `stdout` in `mode`. Under `--json`/`--stream-json`
/// exactly one JSON object is written to stdout; human output goes to stdout too,
/// while all progress/logs are expected on stderr.
///
/// # Errors
/// Returns an I/O error if writing to `out` fails.
pub fn emit<T: CommandOutput, W: Write>(
    output: &T,
    mode: OutputMode,
    color: bool,
    out: &mut W,
) -> io::Result<()> {
    match mode {
        OutputMode::Human => writeln!(out, "{}", output.render_human(color)),
        // Direct struct serialization preserves declaration order (schema first).
        OutputMode::Json | OutputMode::StreamJson => {
            let json = serde_json::to_string(output).map_err(io::Error::other)?;
            writeln!(out, "{json}")
        }
    }
}

/// Emit to the process stdout.
///
/// # Errors
/// Returns an I/O error if writing to stdout fails.
pub fn emit_stdout<T: CommandOutput>(output: &T, mode: OutputMode, color: bool) -> io::Result<()> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    emit(output, mode, color, &mut lock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Demo {
        schema: &'static str,
        value: u32,
    }

    impl CommandOutput for Demo {
        fn render_human(&self, color: bool) -> String {
            if color {
                format!("\x1b[1mvalue\x1b[0m = {}", self.value)
            } else {
                format!("value = {}", self.value)
            }
        }
    }

    fn demo() -> Demo {
        Demo {
            schema: "drakkar.demo/1",
            value: 42,
        }
    }

    #[test]
    fn json_output_is_one_object_schema_first() {
        let mut buf = Vec::new();
        emit(&demo(), OutputMode::Json, false, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        // Exactly one line (one object) and schema is the first key.
        assert_eq!(s.lines().count(), 1);
        assert!(s.starts_with("{\"schema\":\"drakkar.demo/1\""), "got: {s}");
    }

    #[test]
    fn human_output_respects_color_gate() {
        let mut plain = Vec::new();
        emit(&demo(), OutputMode::Human, false, &mut plain).unwrap();
        assert!(!String::from_utf8(plain).unwrap().contains('\x1b'));

        let mut colored = Vec::new();
        emit(&demo(), OutputMode::Human, true, &mut colored).unwrap();
        assert!(String::from_utf8(colored).unwrap().contains('\x1b'));
    }

    #[test]
    fn mode_from_flags() {
        let mut flags = GlobalFlags::default();
        assert_eq!(OutputMode::from_flags(&flags), OutputMode::Human);
        flags.json = true;
        assert_eq!(OutputMode::from_flags(&flags), OutputMode::Json);
        flags.stream_json = true;
        assert_eq!(OutputMode::from_flags(&flags), OutputMode::StreamJson);
    }
}
