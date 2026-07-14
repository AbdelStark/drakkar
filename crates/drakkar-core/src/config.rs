//! `drakkar.config/1` — the one user-edited configuration file (data-model
//! §4.5, RFC-0008 CLI10/CLI11, LD23).
//!
//! `config.toml` is the only file a user edits; everything else under the store
//! is reconstructible (A8). This module implements the schema, the four-level
//! precedence resolver (**flags > `DRAKKAR_*` env > file > defaults**), the
//! mechanical env mapping (`server.port` ⇔ `DRAKKAR_SERVER_PORT`), type/range
//! validation returning named errors, and the atomic `set` writer. It is a
//! library consumed by the CLI and server; it performs no I/O beyond the file
//! path it is handed.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::error::{DkError, ErrorCode, ErrorContext};
use crate::ids::SchemaTag;
use crate::secret::Secret;

/// The schema tag config files carry (optional; absence reads as major 1, DM7).
pub const CONFIG_SCHEMA: SchemaTag = SchemaTag("drakkar.config/1");

/// The `[storage].import_hf_cache` policy (MP11, LD4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportHfCache {
    /// Hard-link / clonefile from the HF cache (default).
    Clone,
    /// Copy from the HF cache.
    Copy,
    /// Do not import from the HF cache.
    Off,
}

/// The `telemetry` setting. The only accepted value in v1 is `off` (CLI16).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Telemetry {
    /// No telemetry.
    Off,
}

/// The `[server]` section.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Bind host (LD22; non-loopback requires an api key, AS18).
    pub host: String,
    /// Bind port.
    pub port: u16,
    /// API key (redacted; empty = unset).
    pub api_key: Secret<String>,
    /// Server-level reasoning-hiding override (AS11).
    pub hide_reasoning: bool,
    /// Enable `/v1/responses` (v0.3, LD5).
    pub responses_api: bool,
}

/// The `[models]` section.
#[derive(Clone, Debug)]
pub struct ModelsConfig {
    /// The reference used when an API `model` is `"default"` (AS3).
    pub default: String,
}

/// The `[storage]` section.
#[derive(Clone, Debug)]
pub struct StorageConfig {
    /// The store root (custom volume supported from v0.1, LD14).
    pub path: String,
    /// The HF-cache import policy.
    pub import_hf_cache: ImportHfCache,
}

/// The `[kv_cache]` section.
#[derive(Clone, Copy, Debug)]
pub struct KvCacheConfig {
    /// Disk tier enable; `None` = the mode default (on for `serve`, off for
    /// one-shot `run`, KV17).
    pub disk: Option<bool>,
    /// KV precision in bits (16 | 8 | 4, KV13).
    pub bits: u8,
    /// Disk-tier budget in GiB (KV19).
    pub disk_budget_gib: u32,
    /// RAM cached-block TTL in minutes (KV20).
    pub ttl_min: u32,
}

/// The `[runtime]` section.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeConfig {
    /// Idle-unload keep-alive for `serve` (AS17).
    pub keep_alive: Duration,
}

/// The `[scheduler]` section.
#[derive(Clone, Copy, Debug)]
pub struct SchedulerConfig {
    /// Maximum concurrent sequences (AS14).
    pub max_concurrency: u32,
}

/// The resolved DRAKKAR configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// `[server]`.
    pub server: ServerConfig,
    /// `[models]`.
    pub models: ModelsConfig,
    /// `[storage]`.
    pub storage: StorageConfig,
    /// `[kv_cache]`.
    pub kv_cache: KvCacheConfig,
    /// `[runtime]`.
    pub runtime: RuntimeConfig,
    /// `[scheduler]`.
    pub scheduler: SchedulerConfig,
    /// `telemetry`.
    pub telemetry: Telemetry,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig {
                host: "127.0.0.1".to_owned(),
                port: 11711,
                api_key: Secret::new(String::new()),
                hide_reasoning: false,
                responses_api: false,
            },
            models: ModelsConfig {
                default: String::new(),
            },
            storage: StorageConfig {
                path: "~/.drakkar".to_owned(),
                import_hf_cache: ImportHfCache::Clone,
            },
            kv_cache: KvCacheConfig {
                disk: None,
                bits: 16,
                disk_budget_gib: 8,
                ttl_min: 30,
            },
            runtime: RuntimeConfig {
                keep_alive: Duration::from_secs(30 * 60),
            },
            scheduler: SchedulerConfig { max_concurrency: 8 },
            telemetry: Telemetry::Off,
        }
    }
}

/// The normative v0.1 key set (CLI10/DM32) plus the optional `schema` tag. Any
/// other key in a file is a `config.invalid_key` error (DM33).
pub const KNOWN_KEYS: [&str; 16] = [
    "schema",
    "server.host",
    "server.port",
    "server.api_key",
    "server.hide_reasoning",
    "server.responses_api",
    "models.default",
    "storage.path",
    "storage.import_hf_cache",
    "kv_cache.disk",
    "kv_cache.bits",
    "kv_cache.disk_budget_gib",
    "kv_cache.ttl_min",
    "runtime.keep_alive",
    "scheduler.max_concurrency",
    "telemetry",
];

fn invalid_key(key: &str) -> DkError {
    DkError::new(
        ErrorCode::ConfigInvalidKey,
        format!("unknown config key '{key}'"),
    )
    .with_context(ErrorContext::new().with_str("key", key))
}

fn invalid_value(key: &str, value: &str, expected: &str) -> DkError {
    DkError::new(
        ErrorCode::ConfigInvalidValue,
        format!("'{key}' expects {expected}; got '{value}'"),
    )
    .with_context(
        ErrorContext::new()
            .with_str("key", key)
            .with_str("expected", expected)
            .with_str("value", value),
    )
}

/// Parse a suffixed duration string (`"30m"`, `"90s"`, `"2h"`, `"500ms"`, DM4).
///
/// # Errors
/// Returns `config.invalid_value` for an unparseable duration.
pub fn parse_duration(key: &str, s: &str) -> Result<Duration, DkError> {
    let t = s.trim();
    let split = t.find(|c: char| !c.is_ascii_digit());
    let bad = || invalid_value(key, s, "a duration like '30m', '90s', '2h', or '500ms'");
    let split = split.ok_or_else(bad)?;
    let (num, suffix) = t.split_at(split);
    let n: u64 = num.parse().map_err(|_| bad())?;
    Ok(match suffix {
        "ms" => Duration::from_millis(n),
        "s" => Duration::from_secs(n),
        "m" => Duration::from_secs(n.saturating_mul(60)),
        "h" => Duration::from_secs(n.saturating_mul(3600)),
        _ => return Err(bad()),
    })
}

fn scalar_to_string(v: &toml::Value) -> Option<String> {
    match v {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(i) => Some(i.to_string()),
        toml::Value::Boolean(b) => Some(b.to_string()),
        toml::Value::Float(f) => Some(f.to_string()),
        _ => None,
    }
}

/// Parse a `config.toml` string into a flat `section.key -> string` map, gating
/// the schema major and rejecting unknown keys.
fn load_file_map(s: &str) -> Result<BTreeMap<String, String>, DkError> {
    let table: toml::Table = toml::from_str(s).map_err(|e| {
        invalid_value(
            "config.toml",
            &e.to_string().replace('\n', " "),
            "valid TOML",
        )
    })?;
    let mut map = BTreeMap::new();
    for (section, val) in &table {
        match val {
            toml::Value::Table(inner) => {
                for (k, v) in inner {
                    let key = format!("{section}.{k}");
                    let sv = scalar_to_string(v)
                        .ok_or_else(|| invalid_value(&key, "table/array", "a scalar value"))?;
                    map.insert(key, sv);
                }
            }
            other => {
                let sv = scalar_to_string(other)
                    .ok_or_else(|| invalid_value(section, "table/array", "a scalar value"))?;
                map.insert(section.clone(), sv);
            }
        }
    }
    // Schema major gate (DM9): a newer major is config.invalid_value.
    if let Some(tag) = map.get("schema") {
        match SchemaTag::parse(tag) {
            Some(parsed) if parsed.major <= 1 => {}
            Some(parsed) => {
                return Err(invalid_value(
                    "schema",
                    tag,
                    &format!("major <= 1 (found {})", parsed.major),
                ));
            }
            None => return Err(invalid_value("schema", tag, "drakkar.config/<major>")),
        }
    }
    // Unknown keys are errors, not silent (DM33).
    for key in map.keys() {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            return Err(invalid_key(key));
        }
    }
    Ok(map)
}

fn env_var_for(key: &str) -> String {
    format!("DRAKKAR_{}", key.to_uppercase().replace('.', "_"))
}

/// Resolve a configuration from the four sources in precedence order
/// (**flags > `DRAKKAR_*` env > file > defaults**, LD23). `env` is the raw
/// environment map (e.g. `DRAKKAR_SERVER_PORT`) and `flags` is a
/// `section.key -> value` map from the command line.
///
/// # Errors
/// Returns `config.invalid_key`/`config.invalid_value` for unknown keys or
/// out-of-range/mistyped values.
pub fn resolve(
    file: Option<&str>,
    env: &BTreeMap<String, String>,
    flags: &BTreeMap<String, String>,
) -> Result<Config, DkError> {
    let file_map = match file {
        Some(s) => load_file_map(s)?,
        None => BTreeMap::new(),
    };
    let get = |key: &str| -> Option<String> {
        flags
            .get(key)
            .cloned()
            .or_else(|| env.get(&env_var_for(key)).cloned())
            .or_else(|| file_map.get(key).cloned())
    };

    let mut cfg = Config::default();

    if let Some(v) = get("server.host") {
        cfg.server.host = v;
    }
    if let Some(v) = get("server.port") {
        let n: u32 = v
            .parse()
            .map_err(|_| invalid_value("server.port", &v, "a port in 1..=65535"))?;
        if !(1..=65535).contains(&n) {
            return Err(invalid_value("server.port", &v, "a port in 1..=65535"));
        }
        cfg.server.port = n as u16;
    }
    if let Some(v) = get("server.api_key") {
        cfg.server.api_key = Secret::new(v);
    }
    if let Some(v) = get("server.hide_reasoning") {
        cfg.server.hide_reasoning = parse_bool("server.hide_reasoning", &v)?;
    }
    if let Some(v) = get("server.responses_api") {
        cfg.server.responses_api = parse_bool("server.responses_api", &v)?;
    }
    if let Some(v) = get("models.default") {
        cfg.models.default = v;
    }
    if let Some(v) = get("storage.path") {
        cfg.storage.path = v;
    }
    if let Some(v) = get("storage.import_hf_cache") {
        cfg.storage.import_hf_cache = match v.as_str() {
            "clone" => ImportHfCache::Clone,
            "copy" => ImportHfCache::Copy,
            "off" => ImportHfCache::Off,
            _ => {
                return Err(invalid_value(
                    "storage.import_hf_cache",
                    &v,
                    "'clone' | 'copy' | 'off'",
                ));
            }
        };
    }
    if let Some(v) = get("kv_cache.disk") {
        cfg.kv_cache.disk = Some(parse_bool("kv_cache.disk", &v)?);
    }
    if let Some(v) = get("kv_cache.bits") {
        let n: u8 = v
            .parse()
            .map_err(|_| invalid_value("kv_cache.bits", &v, "16 | 8 | 4"))?;
        if ![16, 8, 4].contains(&n) {
            return Err(invalid_value("kv_cache.bits", &v, "16 | 8 | 4"));
        }
        cfg.kv_cache.bits = n;
    }
    if let Some(v) = get("kv_cache.disk_budget_gib") {
        cfg.kv_cache.disk_budget_gib = v
            .parse()
            .map_err(|_| invalid_value("kv_cache.disk_budget_gib", &v, "a non-negative integer"))?;
    }
    if let Some(v) = get("kv_cache.ttl_min") {
        cfg.kv_cache.ttl_min = v
            .parse()
            .map_err(|_| invalid_value("kv_cache.ttl_min", &v, "a non-negative integer"))?;
    }
    if let Some(v) = get("runtime.keep_alive") {
        cfg.runtime.keep_alive = parse_duration("runtime.keep_alive", &v)?;
    }
    if let Some(v) = get("scheduler.max_concurrency") {
        let n: u32 = v
            .parse()
            .map_err(|_| invalid_value("scheduler.max_concurrency", &v, "a positive integer"))?;
        if n == 0 {
            return Err(invalid_value(
                "scheduler.max_concurrency",
                &v,
                "a positive integer",
            ));
        }
        cfg.scheduler.max_concurrency = n;
    }
    if let Some(v) = get("telemetry") {
        if v != "off" {
            return Err(invalid_value(
                "telemetry",
                &v,
                "'off' (the only accepted value in v1)",
            ));
        }
        cfg.telemetry = Telemetry::Off;
    }

    Ok(cfg)
}

fn parse_bool(key: &str, v: &str) -> Result<bool, DkError> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid_value(key, v, "true | false")),
    }
}

/// Validate that `key` is a settable config key and `value` is well-typed and
/// in range — the check `drakkar config set` runs before writing (CLI11/DM33).
///
/// # Errors
/// `config.invalid_key` for an unknown/unsettable key, `config.invalid_value`
/// for a bad value.
pub fn validate_key_value(key: &str, value: &str) -> Result<(), DkError> {
    if key == "schema" || !KNOWN_KEYS.contains(&key) {
        return Err(invalid_key(key));
    }
    // Reuse the resolver's validation by resolving a one-key flag overlay.
    let mut flags = BTreeMap::new();
    flags.insert(key.to_owned(), value.to_owned());
    resolve(None, &BTreeMap::new(), &flags).map(|_| ())
}

/// Serialize a [`Config`] back to `config.toml` text (exposing the API key, so
/// this output is written only to the user-owned file).
#[must_use]
pub fn to_toml(cfg: &Config) -> String {
    let hf = match cfg.storage.import_hf_cache {
        ImportHfCache::Clone => "clone",
        ImportHfCache::Copy => "copy",
        ImportHfCache::Off => "off",
    };
    // Top-level bare keys must precede every table header in TOML.
    let mut out = format!("schema = \"{}\"\n", CONFIG_SCHEMA.0);
    out.push_str("telemetry = \"off\"\n\n");
    out.push_str("[server]\n");
    out.push_str(&format!("host = \"{}\"\n", cfg.server.host));
    out.push_str(&format!("port = {}\n", cfg.server.port));
    out.push_str(&format!("api_key = \"{}\"\n", cfg.server.api_key.expose()));
    out.push_str(&format!("hide_reasoning = {}\n", cfg.server.hide_reasoning));
    out.push_str(&format!("responses_api = {}\n\n", cfg.server.responses_api));
    out.push_str("[models]\n");
    out.push_str(&format!("default = \"{}\"\n\n", cfg.models.default));
    out.push_str("[storage]\n");
    out.push_str(&format!("path = \"{}\"\n", cfg.storage.path));
    out.push_str(&format!("import_hf_cache = \"{hf}\"\n\n"));
    out.push_str("[kv_cache]\n");
    if let Some(disk) = cfg.kv_cache.disk {
        out.push_str(&format!("disk = {disk}\n"));
    }
    out.push_str(&format!("bits = {}\n", cfg.kv_cache.bits));
    out.push_str(&format!(
        "disk_budget_gib = {}\n",
        cfg.kv_cache.disk_budget_gib
    ));
    out.push_str(&format!("ttl_min = {}\n\n", cfg.kv_cache.ttl_min));
    out.push_str("[runtime]\n");
    out.push_str(&format!(
        "keep_alive = \"{}s\"\n\n",
        cfg.runtime.keep_alive.as_secs()
    ));
    out.push_str("[scheduler]\n");
    out.push_str(&format!(
        "max_concurrency = {}\n",
        cfg.scheduler.max_concurrency
    ));
    out
}

/// Set one key and write `config.toml` atomically (temp + rename, CLI11),
/// validating the key/value before touching the file so a rejected value leaves
/// the prior file intact.
///
/// # Errors
/// `config.invalid_key`/`config.invalid_value` (before any write), or a
/// `store.write_failed` on an I/O failure.
pub fn set(path: &std::path::Path, key: &str, value: &str) -> Result<(), DkError> {
    validate_key_value(key, value)?;

    // Load the current file (or defaults), apply the one key, re-serialize.
    let existing = std::fs::read_to_string(path).ok();
    let mut flags = BTreeMap::new();
    flags.insert(key.to_owned(), value.to_owned());
    let cfg = resolve(existing.as_deref(), &BTreeMap::new(), &flags)?;
    let text = to_toml(&cfg);

    let write_err = |cause: std::io::Error| {
        DkError::new(
            ErrorCode::StoreWriteFailed,
            format!("writing {} failed: {cause}", path.display()),
        )
        .with_context(
            ErrorContext::new()
                .with_str("path", path.display().to_string())
                .with_str("cause", cause.to_string()),
        )
    };
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, &text).map_err(write_err)?;
    // Atomic replace: rename(2) either fully replaces or leaves the old file.
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        write_err(e)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests;
