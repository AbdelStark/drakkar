# Configuration (`config.toml`)

`config.toml` is the only file DRAKKAR expects you to edit. Everything else under
the store is reconstructible from it plus the model cache, so it is the one file
worth backing up. It carries the `drakkar.config/1` schema
([data model §4.5](../spec/03-data-model.md#45-configuration-configtoml-drakkarconfig1)).

Default location: `~/.config/drakkar/config.toml` (honoring `XDG_CONFIG_HOME`). A
missing file is not an error — every key falls back to a built-in default.

## Commands

```
drakkar config path            # print the config file location
drakkar config get <key>       # print a key's effective value and its source layer
drakkar config set <key> <val> # validate, then write atomically (0600)
```

`config get` reports the value **and the layer it came from**, e.g.
`server.port = 9090 (env)`; add `--json` for a single `drakkar.config/1` object
(`schema` first). `config set` rejects an unknown key or an out-of-range value
naming the key, exits `2`, and does not touch the file. `server.api_key` is
redacted in every `get` rendering.

## Precedence

Each setting is resolved from four sources, highest priority first
([RFC-0008 CLI10](../rfcs/RFC-0008-cli-ux.md), LD23):

1. **Command-line flag** (e.g. `--port 9090`)
2. **Environment variable** `DRAKKAR_<SECTION>_<KEY>` (uppercase, dots → underscores)
3. **`config.toml`**
4. **Built-in default**

The environment mapping is mechanical: `server.port` reads `DRAKKAR_SERVER_PORT`,
`kv_cache.bits` reads `DRAKKAR_KV_CACHE_BITS`, and so on. A value set at a higher
tier fully replaces the lower tiers for that one key; unrelated keys are
unaffected.

## Keys and defaults

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `server.host` | string | `127.0.0.1` | A non-loopback bind requires an API key (AS18). |
| `server.port` | integer | `11711` | `1`–`65535`. |
| `server.api_key` | string | `""` | Bearer secret; see [API key](#api-key) below. |
| `server.hide_reasoning` | bool | `false` | Server-level reasoning-hiding override (AS11). |
| `server.responses_api` | bool | `false` | Enable `/v1/responses` (v0.3). |
| `models.default` | string | `""` | Used when an API `model` is `"default"`. |
| `storage.path` | string | `~/.drakkar` | The store root. |
| `storage.import_hf_cache` | enum | `clone` | `clone` \| `copy` \| `off`. |
| `kv_cache.disk` | bool | *(mode default)* | On for `serve`, off for one-shot `run` when unset. |
| `kv_cache.bits` | integer | `16` | `16` \| `8` \| `4`. |
| `kv_cache.disk_budget_gib` | integer | `8` | Disk-tier budget. |
| `kv_cache.ttl_min` | integer | `30` | RAM cached-block TTL, minutes. |
| `runtime.keep_alive` | duration | `30m` | Suffixed: `30m`, `90s`, `2h`, `500ms`. |
| `scheduler.max_concurrency` | integer | `8` | Maximum concurrent sequences; `>= 1`. |
| `telemetry` | enum | `off` | Only `off` is accepted. |

An unknown key is the error [`config.invalid_key`](error-codes.md); an
out-of-range or mistyped value is [`config.invalid_value`](error-codes.md). DRAKKAR
never silently coerces a bad value — it names the key and what it expected.

## API key

The server API key has its own precedence, using a dedicated, shorter
environment variable ([security §4](../spec/06-security.md#4-secrets-handling),
SEC28):

```
--api-key  >  DRAKKAR_API_KEY  >  server.api_key (in config.toml)
```

The loaded key is held as a redacting `Secret<String>`: it never appears in logs,
`--json` output, or `Debug` formatting. It is a static bearer secret — there is
no key-management system.

## File permissions

Because it may contain `server.api_key`, `config.toml` is written mode `0600`
(owner read/write only) on every mutation ([security §2.4](../spec/06-security.md#24-b4-local-filesystem),
SEC20). Writes are atomic: DRAKKAR writes a temporary file — created private from
the first byte — then renames it over the target, so an interrupted write leaves
the previous file intact. A rewrite also re-tightens the mode if the file had
been made group- or world-readable.
