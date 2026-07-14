# Security Policy

## Reporting a vulnerability

Report vulnerabilities through
[GitHub private vulnerability reporting](https://github.com/AbdelStark/drakkar/security/advisories/new).
Do not open a public issue for anything you believe is exploitable.

You can expect an acknowledgment within 72 hours and a status update within 14 days.
Coordinated disclosure is preferred; we will credit reporters in release notes unless you
ask otherwise.

## Scope

DRAKKAR's security posture, threat model, and trust boundaries are specified in
[docs/spec/06-security.md](docs/spec/06-security.md). In brief:

- Model artifacts are treated as data, never code: safetensors and GGUF only, no pickle,
  no remote code execution paths. Parsing of untrusted artifacts is defensive and fuzzed.
- The server binds 127.0.0.1 by default; non-loopback binding requires an explicit flag
  plus an API key.
- No telemetry. The KV cache disk tier contains user prompts and is created mode 0600.
- Hugging Face tokens are read from standard locations and never written to logs or state.

Reports about violations of any of these properties are in scope, as are memory-safety
issues in the FFI shim and dependency vulnerabilities.

## Supported versions

Pre-1.0, only the latest release receives security fixes.
