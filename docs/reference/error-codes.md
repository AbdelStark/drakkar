# Error code reference (`drakkar.errors/1`)

> Generated from the `ErrorCode` registry in `drakkar-core`. Do not edit by hand;
> regenerate with `UPDATE_DOCS=1 cargo test -p drakkar-core --test error_reference`.

Every failure DRAKKAR can produce carries one of these stable codes. Codes are
**append-only and never reused or re-categorized** within registry major 1
(RV12, INV-ADDITIVE-REGISTRY): a code's category, HTTP status, and exit code never
change. Consumers that see an unknown code fall back to the `category` field, which
is closed.

## Exit codes

The CLI exit code is a coarse classifier derived from the category (public-API §2.3,
RFC-0008 CLI8); the dotted code string is the precise contract. Read it from
`--json` output rather than scraping messages.

| Category | CLI exit | HTTP default |
| --- | --- | --- |
| `usage` | 2 | 400 |
| `model_not_found` | 3 | 404 |
| `infeasible` | 4 | 422 |
| `network` | 5 | 503 |
| `format` | 6 | 422 |
| `engine` | 6 | 500 |
| `disk` | 7 | 507 |
| `internal` | 6 | 500 |

Exit code 1 is deliberately unassigned and never emitted intentionally.

## Codes by subsystem

### `cli`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `cli.invalid_args` | `usage` | cli | 400 | `terminal` | `cli_invalid_args` |
| `cli.missing_model_arg` | `usage` | cli | 400 | `terminal` | `cli_missing_model_arg` |

### `config`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `config.invalid_key` | `usage` | both | 400 | `terminal` | `config_invalid_key` |
| `config.invalid_value` | `usage` | both | 400 | `terminal` | `config_invalid_value` |

### `models`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `models.not_found` | `model_not_found` | both | 404 | `terminal` | `models_not_found` |
| `models.not_installed` | `model_not_found` | both | 404 | `terminal` | `models_not_installed` |
| `models.repo_not_found` | `model_not_found` | both | 404 | `terminal` | `models_repo_not_found` |
| `models.gated_repo_no_token` | `model_not_found` | both | 404 | `terminal` | `accept_license` |
| `models.unsupported_architecture` | `format` | both | 422 | `terminal` | `models_unsupported_architecture` |
| `models.pickle_rejected` | `format` | both | 422 | `terminal` | `models_pickle_rejected` |
| `models.invalid_metadata` | `format` | both | 422 | `terminal` | `models_invalid_metadata` |

### `download`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `download.network_failed` | `network` | cli | 503 | `after_backoff` | `resume_pull` |
| `download.hub_unreachable` | `network` | both | 503 | `after_backoff` | `download_hub_unreachable` |
| `download.integrity_mismatch` | `format` | cli | 422 | `terminal` | `download_integrity_mismatch` |
| `download.no_space` | `disk` | cli | 507 | `terminal` | `prune_store` |

### `store`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `store.write_failed` | `disk` | both | 507 | `terminal` | `store_write_failed` |
| `store.corrupt_blob` | `format` | both | 422 | `terminal` | `store_corrupt_blob` |

### `fit`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `fit.wont_fit` | `infeasible` | both | 422 | `terminal` | `run_sibling` |
| `fit.context_exceeded` | `infeasible` | both | 413 | `terminal` | `reduce_context` |

### `kv`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `kv.pool_exhausted` | `infeasible` | http | 429 | `after` | `retry_after_or_reduce` |

### `engine`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `engine.load_failed` | `engine` | both | 500 | `terminal` | `engine_load_failed` |
| `engine.metal_init_failed` | `engine` | both | 500 | `terminal` | `engine_metal_init_failed` |
| `engine.inference_failed` | `engine` | both | 500 | `terminal` | `engine_inference_failed` |

### `backend`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `backend.metal_fault` | `engine` | both | 500 | `terminal` | `backend_metal_fault` |
| `backend.capability_absent` | `engine` | both | 500 | `terminal` | `backend_capability_absent` |
| `backend.io` | `engine` | both | 500 | `terminal` | `backend_io` |

### `abi`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `abi.version_mismatch` | `internal` | both | 500 | `terminal` | `abi_version_mismatch` |
| `abi.struct_size_mismatch` | `internal` | both | 500 | `terminal` | `abi_struct_size_mismatch` |
| `abi.thread_violation` | `internal` | both | 500 | `terminal` | `abi_thread_violation` |
| `abi.invalid_argument` | `internal` | both | 500 | `terminal` | `abi_invalid_argument` |

### `grammar`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `grammar.schema_compile_failed` | `usage` | both | 422 | `terminal` | `grammar_schema_compile_failed` |

### `server`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `server.unsupported_field` | `usage` | http | 400 | `terminal` | `server_unsupported_field` |
| `server.model_loading` | `engine` | http | 503 | `after_backoff` | `server_model_loading` |

### `internal`

| Code | Category | Surfaces | HTTP | Retry | Remedy template |
| --- | --- | --- | --- | --- | --- |
| `internal.panic` | `internal` | both | 500 | `terminal` | `bug-report` |
| `internal.invariant` | `internal` | both | 500 | `terminal` | `bug-report` |
| `internal.budget_breach` | `internal` | both | 500 | `terminal` | `bug-report` |

