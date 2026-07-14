//! `drakkar.config/1` load/merge/validate tests (data-model §4.5, RFC-0008).

use super::*;
use std::collections::BTreeMap;

const FIXTURE: &str = r#"
schema = "drakkar.config/1"
telemetry = "off"

[server]
host = "0.0.0.0"
port = 8080
api_key = "sk-secret"
hide_reasoning = true
responses_api = true

[models]
default = "qwen3-8b"

[storage]
path = "/data/drakkar"
import_hf_cache = "copy"

[kv_cache]
disk = true
bits = 8
disk_budget_gib = 64
ttl_min = 120

[runtime]
keep_alive = "45m"

[scheduler]
max_concurrency = 4
"#;

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn config_defaults_are_the_normative_v01_values() {
    // AC1 (defaults half): absent file → built-in defaults.
    let cfg = resolve(None, &BTreeMap::new(), &BTreeMap::new()).unwrap();
    assert_eq!(cfg.server.host, "127.0.0.1");
    assert_eq!(cfg.server.port, 11711);
    assert_eq!(cfg.kv_cache.bits, 16);
    assert_eq!(cfg.scheduler.max_concurrency, 8);
    assert_eq!(cfg.telemetry, Telemetry::Off);
    assert!(cfg.server.api_key.expose().is_empty());
    assert_eq!(cfg.kv_cache.disk, None);
    assert_eq!(
        cfg.runtime.keep_alive,
        std::time::Duration::from_secs(30 * 60)
    );
}

#[test]
fn config_load_populates_every_key_with_correct_types() {
    // AC1 (load half): a fixture populates every CLI10 key.
    let cfg = resolve(Some(FIXTURE), &BTreeMap::new(), &BTreeMap::new()).unwrap();
    assert_eq!(cfg.server.host, "0.0.0.0");
    assert_eq!(cfg.server.port, 8080);
    assert_eq!(cfg.server.api_key.expose(), "sk-secret");
    assert!(cfg.server.hide_reasoning);
    assert!(cfg.server.responses_api);
    assert_eq!(cfg.models.default, "qwen3-8b");
    assert_eq!(cfg.storage.path, "/data/drakkar");
    assert_eq!(cfg.storage.import_hf_cache, ImportHfCache::Copy);
    assert_eq!(cfg.kv_cache.disk, Some(true));
    assert_eq!(cfg.kv_cache.bits, 8);
    assert_eq!(cfg.kv_cache.disk_budget_gib, 64);
    assert_eq!(cfg.kv_cache.ttl_min, 120);
    assert_eq!(
        cfg.runtime.keep_alive,
        std::time::Duration::from_secs(45 * 60)
    );
    assert_eq!(cfg.scheduler.max_concurrency, 4);
}

#[test]
fn config_absent_keys_fall_back_to_defaults() {
    // A partial file fills only what it names; the rest are defaults.
    let partial = "[server]\nport = 9000\n";
    let cfg = resolve(Some(partial), &BTreeMap::new(), &BTreeMap::new()).unwrap();
    assert_eq!(cfg.server.port, 9000);
    assert_eq!(cfg.server.host, "127.0.0.1"); // default
    assert_eq!(cfg.scheduler.max_concurrency, 8); // default
}

#[test]
fn config_precedence_flag_over_env_over_file() {
    // AC2: file < env < flag.
    let file = "[server]\nport = 1111\n";
    // File only: 1111.
    let c0 = resolve(Some(file), &BTreeMap::new(), &BTreeMap::new()).unwrap();
    assert_eq!(c0.server.port, 1111);
    // Env overrides file.
    let e = env(&[("DRAKKAR_SERVER_PORT", "2222")]);
    let c1 = resolve(Some(file), &e, &BTreeMap::new()).unwrap();
    assert_eq!(c1.server.port, 2222);
    // Flag overrides env (and file).
    let mut flags = BTreeMap::new();
    flags.insert("server.port".to_owned(), "3333".to_owned());
    let c2 = resolve(Some(file), &e, &flags).unwrap();
    assert_eq!(c2.server.port, 3333);
}

#[test]
fn config_env_mapping_is_mechanical_for_underscored_sections() {
    // kv_cache.bits ⇔ DRAKKAR_KV_CACHE_BITS (dots → underscores, uppercased).
    let e = env(&[("DRAKKAR_KV_CACHE_BITS", "4")]);
    let cfg = resolve(None, &e, &BTreeMap::new()).unwrap();
    assert_eq!(cfg.kv_cache.bits, 4);
}

#[test]
fn config_unknown_key_is_invalid_key() {
    // AC3: unknown key → config.invalid_key.
    let bad = "[server]\nporta = 8080\n";
    let err = resolve(Some(bad), &BTreeMap::new(), &BTreeMap::new()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalidKey);
}

#[test]
fn config_telemetry_on_is_invalid_value() {
    // AC3: telemetry="on" → config.invalid_value (CLI16, off only).
    let bad = "telemetry = \"on\"\n";
    let err = resolve(Some(bad), &BTreeMap::new(), &BTreeMap::new()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalidValue);
}

#[test]
fn config_out_of_range_port_is_invalid_value() {
    // AC3: out-of-range port → config.invalid_value.
    let bad = "[server]\nport = 99999\n";
    let err = resolve(Some(bad), &BTreeMap::new(), &BTreeMap::new()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalidValue);
    // Zero is also rejected.
    let zero = "[server]\nport = 0\n";
    assert_eq!(
        resolve(Some(zero), &BTreeMap::new(), &BTreeMap::new())
            .unwrap_err()
            .code(),
        ErrorCode::ConfigInvalidValue
    );
}

#[test]
fn config_bad_bits_and_import_policy_are_invalid_value() {
    for bad in [
        "[kv_cache]\nbits = 3\n",
        "[storage]\nimport_hf_cache = \"nope\"\n",
    ] {
        assert_eq!(
            resolve(Some(bad), &BTreeMap::new(), &BTreeMap::new())
                .unwrap_err()
                .code(),
            ErrorCode::ConfigInvalidValue,
            "expected invalid_value for {bad:?}"
        );
    }
}

#[test]
fn config_newer_schema_major_is_rejected() {
    // DM9: a newer-major config.toml is config.invalid_value, not silent.
    let future = "schema = \"drakkar.config/2\"\n";
    assert_eq!(
        resolve(Some(future), &BTreeMap::new(), &BTreeMap::new())
            .unwrap_err()
            .code(),
        ErrorCode::ConfigInvalidValue
    );
}

#[test]
fn config_duration_suffixes_parse() {
    // AC5: "30m" / "90s" (and h/ms) parse to the correct Duration.
    use std::time::Duration;
    assert_eq!(
        parse_duration("k", "30m").unwrap(),
        Duration::from_secs(1800)
    );
    assert_eq!(parse_duration("k", "90s").unwrap(), Duration::from_secs(90));
    assert_eq!(
        parse_duration("k", "2h").unwrap(),
        Duration::from_secs(7200)
    );
    assert_eq!(
        parse_duration("k", "500ms").unwrap(),
        Duration::from_millis(500)
    );
    // A bad duration is a named error, not a silent zero.
    assert_eq!(
        parse_duration("k", "later").unwrap_err().code(),
        ErrorCode::ConfigInvalidValue
    );
    assert!(parse_duration("k", "30").is_err()); // no suffix
}

#[test]
fn config_set_writes_atomically_and_roundtrips() {
    // AC4: `set` writes atomically and the result reloads with the new value.
    let dir = std::env::temp_dir().join(format!("drakkar-cfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let _ = std::fs::remove_file(&path);

    set(&path, "server.port", "4242").unwrap();
    // No stray temp file lingers.
    assert!(!path.with_extension("toml.tmp").exists());
    let text = std::fs::read_to_string(&path).unwrap();
    let cfg = resolve(Some(&text), &BTreeMap::new(), &BTreeMap::new()).unwrap();
    assert_eq!(cfg.server.port, 4242);

    // A second set preserves the first and applies the second.
    set(&path, "scheduler.max_concurrency", "2").unwrap();
    let text2 = std::fs::read_to_string(&path).unwrap();
    let cfg2 = resolve(Some(&text2), &BTreeMap::new(), &BTreeMap::new()).unwrap();
    assert_eq!(cfg2.server.port, 4242);
    assert_eq!(cfg2.scheduler.max_concurrency, 2);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_set_revalidates_before_writing() {
    // AC4: a rejected value leaves the prior file intact (validate-before-write).
    let dir = std::env::temp_dir().join(format!("drakkar-cfg-rv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[server]\nport = 7000\n").unwrap();

    // Bad value: rejected, file untouched.
    let err = set(&path, "server.port", "not-a-port").unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalidValue);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[server]\nport = 7000\n"
    );

    // Bad key: rejected, file untouched.
    let err = set(&path, "server.nonsense", "x").unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalidKey);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[server]\nport = 7000\n"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_to_toml_roundtrips_including_api_key() {
    // The file serializer exposes the API key (writes only to the user file).
    let cfg = resolve(Some(FIXTURE), &BTreeMap::new(), &BTreeMap::new()).unwrap();
    let text = to_toml(&cfg);
    let back = resolve(Some(&text), &BTreeMap::new(), &BTreeMap::new()).unwrap();
    assert_eq!(back.server.api_key.expose(), "sk-secret");
    assert_eq!(back.server.port, 8080);
    assert_eq!(back.kv_cache.disk, Some(true));
    assert_eq!(back.storage.import_hf_cache, ImportHfCache::Copy);
    assert_eq!(back.runtime.keep_alive, cfg.runtime.keep_alive);
}

#[cfg(unix)]
#[test]
fn config_permissions_are_0600_after_set() {
    // SEC20: config.toml is written mode 0600 (it may hold server.api_key).
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("drakkar-cfg-perm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let _ = std::fs::remove_file(&path);

    // Fresh create is 0600.
    set(&path, "server.api_key", "sk-live-123").unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "fresh config must be 0600, got {mode:o}");

    // Rewriting a pre-existing world-readable file re-tightens it to 0600.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    set(&path, "server.port", "9090").unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "rewrite must re-tighten to 0600, got {mode:o}");

    // The written key survives the round-trip.
    let text = std::fs::read_to_string(&path).unwrap();
    let cfg = resolve(Some(&text), &BTreeMap::new(), &BTreeMap::new()).unwrap();
    assert_eq!(cfg.server.api_key.expose(), "sk-live-123");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_api_key_precedence_flag_over_env_over_file() {
    // SEC28: --api-key > DRAKKAR_API_KEY > server.api_key in config.
    let cfg = resolve(Some(FIXTURE), &BTreeMap::new(), &BTreeMap::new()).unwrap();
    assert_eq!(cfg.server.api_key.expose(), "sk-secret"); // the config-file tier

    // File only wins when nothing else is set.
    let k0 = resolve_api_key(None, &BTreeMap::new(), &cfg);
    assert_eq!(k0.expose(), "sk-secret");

    // DRAKKAR_API_KEY overrides the file value.
    let e = env(&[("DRAKKAR_API_KEY", "env-key")]);
    let k1 = resolve_api_key(None, &e, &cfg);
    assert_eq!(k1.expose(), "env-key");

    // --api-key overrides the env var (and the file).
    let k2 = resolve_api_key(Some("flag-key"), &e, &cfg);
    assert_eq!(k2.expose(), "flag-key");

    // Unset everywhere → empty secret, not a panic.
    let empty = Config::default();
    assert!(
        resolve_api_key(None, &BTreeMap::new(), &empty)
            .expose()
            .is_empty()
    );
}

#[test]
fn config_api_key_never_serializes_in_plaintext_via_debug() {
    // SEC27: the key is a Secret<String>; its Debug/redacted form hides it.
    let cfg = resolve(Some(FIXTURE), &BTreeMap::new(), &BTreeMap::new()).unwrap();
    let debug = format!("{:?}", cfg.server.api_key);
    assert!(
        !debug.contains("sk-secret"),
        "Secret Debug leaked the key: {debug}"
    );
}
