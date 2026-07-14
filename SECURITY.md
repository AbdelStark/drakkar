# Security Policy

## Reporting a vulnerability

**GitHub private vulnerability reporting is the exclusive channel.** Report
through
[the private advisory form](https://github.com/AbdelStark/drakkar/security/advisories/new).
Do not open a public issue, discussion, or pull request for anything you believe
is exploitable — if a vulnerability is reported publicly, we convert or lock it
into a private advisory rather than triage it in the open.

## Our commitments (SEC30)

- **Acknowledgement within 7 days** of a report reaching the private advisory.
- **Coordinated disclosure with a default 90-day window** from acknowledgement
  to public disclosure; we will agree an earlier or later date with you when the
  situation warrants it.
- **Credit** in the release notes for the fix, unless you ask us not to.
- **Asset-ranked severity**: we judge impact against the assets in the threat
  model ([docs/spec/06-security.md §1](docs/spec/06-security.md)) — the GPU
  memory contract, the artifact-as-data boundary, the loopback/API-key network
  edge, HF token confidentiality, and the KV disk tier — not against a generic
  score alone.

## Scope

DRAKKAR's security posture, threat model, and trust boundaries are specified in
[docs/spec/06-security.md](docs/spec/06-security.md), with the reporting process
in [§5](docs/spec/06-security.md#5-vulnerability-reporting). In brief:

- Model artifacts are treated as data, never code: safetensors and GGUF only, no
  pickle, no remote code execution paths. Parsing of untrusted artifacts is
  defensive and fuzzed.
- The server binds `127.0.0.1` by default; non-loopback binding requires an
  explicit flag plus an API key.
- No telemetry. The KV cache disk tier contains user prompts and is created mode
  `0600`.
- Hugging Face tokens are read from standard locations and never written to logs
  or state.

Reports about violations of any of these properties are in scope, as are
memory-safety issues in the FFI shim and dependency vulnerabilities.

## Supported versions

Pre-1.0, only the latest release receives security fixes.

## Maintainer note: enabling private reporting

Private vulnerability reporting is enabled on this repository (Settings →
Code security and analysis → Private vulnerability reporting), which surfaces the
advisory form linked above on the **Security** tab. If a fork or mirror needs it,
enable it there too:

```
gh api -X PUT repos/<owner>/<repo>/private-vulnerability-reporting
```
