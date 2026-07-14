//! `drakkar config get|set|path` — the CLI-owned layer of the configuration
//! precedence chain (RFC-0008 §5, CLI10–CLI11, LD23).
//!
//! The config *schema*, TOML parse/serialize, file+env overlay, defaults, and
//! atomic writer live in `drakkar-core::config` (issue #126/#52); this module
//! resolves the file path, layers the process environment on top, and renders
//! the dual human/`--json` output. `get` reports the effective value and the
//! precedence layer it came from; `set` validates and writes atomically;
//! `path` prints the file location.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use drakkar_core::config;
use drakkar_core::{DkError, ErrorCode};
use serde::Serialize;

use crate::cli::ConfigAction;
use crate::output::{self, CommandOutput, OutputMode};

/// Compute the config path from the two environment inputs (XDG first, then
/// `$HOME/.config`). Pure so it is unit-testable without touching the real
/// environment.
fn config_path_from(xdg: Option<OsString>, home: Option<OsString>) -> PathBuf {
    if let Some(x) = xdg.filter(|x| !x.is_empty()) {
        return PathBuf::from(x).join("drakkar").join("config.toml");
    }
    let home = home.map(PathBuf::from).unwrap_or_default();
    home.join(".config").join("drakkar").join("config.toml")
}

/// `~/.config/drakkar/config.toml`, honoring `XDG_CONFIG_HOME` (SEC20).
#[must_use]
pub fn default_config_path() -> PathBuf {
    config_path_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

/// The `DRAKKAR_*` slice of the process environment, as a plain map.
fn drakkar_env() -> BTreeMap<String, String> {
    std::env::vars()
        .filter(|(k, _)| k.starts_with("DRAKKAR_"))
        .collect()
}

/// `config get` / `config set` result: one key's effective value and source.
#[derive(Serialize, Debug)]
struct ConfigValue<'a> {
    schema: &'static str,
    key: &'a str,
    value: serde_json::Value,
    source: &'static str,
}

impl CommandOutput for ConfigValue<'_> {
    fn render_human(&self, color: bool) -> String {
        let rendered = match &self.value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => "(unset)".to_owned(),
            other => other.to_string(),
        };
        if color {
            format!(
                "\x1b[1m{}\x1b[0m = {} \x1b[2m({})\x1b[0m",
                self.key, rendered, self.source
            )
        } else {
            format!("{} = {} ({})", self.key, rendered, self.source)
        }
    }
}

/// `config path` result.
#[derive(Serialize)]
struct ConfigPath {
    schema: &'static str,
    path: String,
}

impl CommandOutput for ConfigPath {
    fn render_human(&self, _color: bool) -> String {
        self.path.clone()
    }
}

/// Resolve one key's effective value and source across the file, environment,
/// and flag layers — the pure core of `config get`, unit-testable.
fn value_output<'a>(
    key: &'a str,
    file: Option<&str>,
    env: &BTreeMap<String, String>,
    flags: &BTreeMap<String, String>,
) -> Result<ConfigValue<'a>, DkError> {
    let (value, source) = config::effective(key, file, env, flags)?;
    Ok(ConfigValue {
        schema: config::CONFIG_SCHEMA.0,
        key,
        value,
        source: source.as_str(),
    })
}

fn emit_result<T: CommandOutput>(out: &T, mode: OutputMode, color: bool) -> Result<(), DkError> {
    match output::emit_stdout(out, mode, color) {
        Ok(()) => Ok(()),
        // A closed pipe (e.g. `| head`) is the reader's choice, not our failure.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(DkError::new(
            ErrorCode::InternalInvariant,
            format!("writing output failed: {e}"),
        )),
    }
}

/// Dispatch a `drakkar config` action.
///
/// # Errors
/// `config.invalid_key`/`config.invalid_value` (exit 2) for a bad key/value on
/// `get`/`set`; `store.write_failed` if the file cannot be written.
pub fn run(action: &ConfigAction, mode: OutputMode, color: bool) -> Result<(), DkError> {
    let path = default_config_path();
    match action {
        ConfigAction::Path => {
            let out = ConfigPath {
                schema: config::CONFIG_SCHEMA.0,
                path: path.display().to_string(),
            };
            emit_result(&out, mode, color)
        }
        ConfigAction::Get { key } => {
            let file = std::fs::read_to_string(&path).ok();
            let out = value_output(key, file.as_deref(), &drakkar_env(), &BTreeMap::new())?;
            emit_result(&out, mode, color)
        }
        ConfigAction::Set { key, value } => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    DkError::new(
                        ErrorCode::StoreWriteFailed,
                        format!("creating {} failed: {e}", parent.display()),
                    )
                })?;
            }
            // Validates type/range and writes 0600 atomically; unknown key or bad
            // value returns a Usage-category error (exit 2) before any write.
            config::set(&path, key, value)?;
            // Report the new effective value (now sourced from the file).
            let file = std::fs::read_to_string(&path).ok();
            let out = value_output(key, file.as_deref(), &drakkar_env(), &BTreeMap::new())?;
            emit_result(&out, mode, color)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn config_path_prefers_xdg_then_home() {
        let p = config_path_from(Some("/x/cfg".into()), Some("/home/u".into()));
        assert_eq!(p, PathBuf::from("/x/cfg/drakkar/config.toml"));
        let p = config_path_from(None, Some("/home/u".into()));
        assert_eq!(p, PathBuf::from("/home/u/.config/drakkar/config.toml"));
        // An empty XDG value falls back to HOME.
        let p = config_path_from(Some("".into()), Some("/home/u".into()));
        assert_eq!(p, PathBuf::from("/home/u/.config/drakkar/config.toml"));
    }

    #[test]
    fn get_reports_value_and_source_layer() {
        // Default when nothing is set.
        let d = value_output("server.port", None, &BTreeMap::new(), &BTreeMap::new()).unwrap();
        assert_eq!(d.value, serde_json::json!(11711));
        assert_eq!(d.source, "default");

        // File value, reported as file-sourced.
        let f = value_output(
            "server.port",
            Some("[server]\nport = 8080\n"),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(f.value, serde_json::json!(8080));
        assert_eq!(f.source, "file");

        // Env beats file.
        let e = value_output(
            "server.port",
            Some("[server]\nport = 8080\n"),
            &map(&[("DRAKKAR_SERVER_PORT", "9090")]),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(e.value, serde_json::json!(9090));
        assert_eq!(e.source, "env");
    }

    #[test]
    fn get_json_has_schema_first() {
        let out = value_output("server.host", None, &BTreeMap::new(), &BTreeMap::new()).unwrap();
        let mut buf = Vec::new();
        output::emit(&out, OutputMode::Json, false, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.starts_with("{\"schema\":\"drakkar.config/1\""),
            "schema must be first: {s}"
        );
        assert_eq!(s.lines().count(), 1);
    }

    #[test]
    fn get_redacts_the_api_key() {
        let out = value_output(
            "server.api_key",
            Some("[server]\napi_key = \"sk-secret\"\n"),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(out.value, serde_json::json!("[redacted]"));
        assert!(!out.render_human(false).contains("sk-secret"));
    }

    #[test]
    fn get_unknown_key_is_a_usage_error() {
        // AC: unknown key names the key and is a usage error (exit 2).
        let err =
            value_output("server.nonsense", None, &BTreeMap::new(), &BTreeMap::new()).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConfigInvalidKey);
        assert_eq!(err.code().exit_code(), 2);
    }

    #[test]
    fn set_via_core_roundtrips_and_rejects_unknown_key() {
        // The `set` path the CLI drives: validate, atomic 0600 write, read back.
        let dir = std::env::temp_dir().join(format!("drakkar-cli-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let _ = std::fs::remove_file(&path);

        config::set(&path, "server.port", "12321").unwrap();
        let file = std::fs::read_to_string(&path).unwrap();
        let out = value_output(
            "server.port",
            Some(&file),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(out.value, serde_json::json!(12321));
        assert_eq!(out.source, "file");

        // Unknown key is rejected with no file mutation semantics tested in core.
        assert_eq!(
            config::set(&path, "server.bogus", "x").unwrap_err().code(),
            ErrorCode::ConfigInvalidKey
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    proptest! {
        /// T5: over random (default, file, env, flag) assignments for a key, the
        /// effective value equals the highest-precedence present source and the
        /// reported source names that layer.
        #[test]
        fn t5_precedence_holds_for_random_assignments(
            file in proptest::option::of(1u16..=65535),
            env in proptest::option::of(1u16..=65535),
            flag in proptest::option::of(1u16..=65535),
        ) {
            let file_str = file.map(|p| format!("[server]\nport = {p}\n"));
            let env_map = match env {
                Some(p) => map(&[("DRAKKAR_SERVER_PORT", &p.to_string())]),
                None => BTreeMap::new(),
            };
            let flag_map = match flag {
                Some(p) => map(&[("server.port", &p.to_string())]),
                None => BTreeMap::new(),
            };

            let out = value_output("server.port", file_str.as_deref(), &env_map, &flag_map).unwrap();

            let (want_val, want_src) = match (flag, env, file) {
                (Some(p), _, _) => (p, "flag"),
                (None, Some(p), _) => (p, "env"),
                (None, None, Some(p)) => (p, "file"),
                (None, None, None) => (11711, "default"),
            };
            prop_assert_eq!(out.value, serde_json::json!(want_val));
            prop_assert_eq!(out.source, want_src);
        }
    }
}
