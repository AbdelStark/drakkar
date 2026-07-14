# RFC-0006: Model Acquisition and Format Pipeline

- Status: Accepted
- Authors: abdelstark
- Created: 2026-07-14
- Target milestone: v0.1 (convert/quantization UX polish: v0.3)

## Summary

"Run a model by pasting a Hugging Face link" is the headline promise. This RFC specifies the
subsystem behind it, owned by the `drakkar-models` crate (area: `models`): reference
resolution across every accepted input form, artifact selection across the format zoo,
downloading with resume and integrity verification, a content-addressed local store with
Hugging Face cache interop, the in-process conversion/quantization pipeline that turns any
reasonable HF repo into a servable local artifact, and the tokenizer/chat-template layer that
determines what the engine actually sees. The feasibility engine (RFC-0004) is in the loop
before any bytes move: resolution fetches metadata only, the fit verdict selects the
artifact, and disk preflight gates the download. Requirements are MP1–MP19; the acceptance
criteria AC1–AC5 from the source draft are folded into
[Testing Strategy](#testing-strategy).

## Motivation

The PRD makes this pipeline load-bearing for the product's first impression:

- [PRD](../../PRD.md#goals-v10-horizon) G1 requires one-command run: `drakkar run
  <hf-link-or-alias>` from cold start to interactive chat. Every stage in this RFC sits on
  that critical path; any manual step (picking a quant repo by hand, converting weights in a
  Python environment, cleaning up a broken download) breaks G1.
- PRD P1 requires the CLI to accept a full HF URL, `org/repo`, `hf.co/org/repo`, or a curated
  alias and resolve it to a runnable artifact. PRD P9 requires content-addressed storage,
  resumable downloads, integrity verification, and compatibility with an existing Hugging
  Face cache to avoid re-downloads.
- [PRD §2.3](../../PRD.md#23-where-existing-tools-fall-short) identifies Python packaging
  friction as the top install failure class in incumbent stacks (mlx-lm, vllm-metal,
  vllm-mlx, vMLX all need a Python 3.12 arm64 environment). The conversion pipeline in this
  RFC is what lets DRAKKAR quantize a 70B checkpoint on-device inside one signed binary with
  zero runtime dependencies — the concrete answer to that gap, and a direct consequence of
  RFC-0002 S2 ([Stack Selection](RFC-0002-stack-selection.md#proposed-design)).
- PRD M1 (brew install to first token in under 5 minutes, download-dominated) means every
  non-download stage here — resolution, selection, integrity, registration — must complete
  in seconds.

Feasibility-first ordering is the differentiator: no incumbent runs a fit computation before
the download (PRD §2.3, first row). This RFC is where that ordering is enforced
mechanically, not just by convention.

## Goals

- Resolve every accepted reference form (MP1) to a runnable local artifact with zero flags on
  a suitable machine, including bf16-only repos that need on-device quantization.
- Move no weight bytes before the fit preflight (RFC-0004 FE1) has produced a verdict and the
  disk preflight (MP9) has passed.
- Store artifacts content-addressed with blob-level dedup, safe concurrent access, and
  garbage collection that never breaks an installed model (MP10–MP12).
- Reuse an existing HF hub cache at clone-level disk cost and zero network cost when it
  already holds the needed files, without ever mutating it (MP11, LD4).
- Convert safetensors bf16/fp16 to MLX quantized artifacts in-process, streaming per-tensor,
  such that a 70B fp16 → 4-bit conversion succeeds on a 48 GB machine (MP13, RFC-0003 IC8).
- Load tokenizers and chat templates such that the rendered prompt and the stream parser
  agree with the model's training format, observably (MP17–MP19).
- Treat downloaded artifacts as data, never code (RFC-0001 A11): safetensors and GGUF only,
  defensive parsing, no pickle, no remote code.

## Non-Goals

- A model marketplace or curation service; Hugging Face is the registry (PRD N4). The alias
  table is a convenience mapping, not a catalog.
- Uploading converted artifacts to the hub on the user's behalf (MP16). Converted artifacts
  register locally; publishing them is the user's action with standard tools.
- Calibration-based quantization schemes (AWQ/GPTQ-style, activation-aware low-bit) as
  on-device conversion targets in v1 (MP15). v1 imports pre-quantized artifacts of those
  families where the backend supports the layout, and otherwise recommends official quants.
- Training, fine-tuning, or LoRA merge tooling (PRD N2).
- Mirroring or re-hosting weights; DRAKKAR downloads from the hub (or reads a local path) and
  nothing else.

## Proposed Design

### Pipeline overview

`drakkar pull` / `drakkar run` execute the stages below in order. Each stage has a named
failure mode ([Failure modes](#failure-modes)) and each boundary is a testable seam.

```text
reference ──▶ resolve ──▶ metadata fetch ──▶ fit preflight ──▶ artifact select
 (MP1)        (MP1)         (MP2)             (RFC-0004 FE1)      (MP4–MP5)
                                                                    │
     register ◀── convert? ◀── verify ◀── download/clone ◀── disk preflight
     (MP10)       (MP13)       (MP8)       (MP7, MP11)         (MP9)
        │
        ▶ tokenizer + template load (MP17–MP18) ──▶ engine load (RFC-0003)
```

Invariants (referenced throughout; violations are release blockers):

- **INV-MP-ORDER.** No weight bytes are downloaded or cloned before the fit preflight has
  produced a verdict and the disk preflight has passed. (`--yes` skips the confirmation
  prompt, never the computation.)
- **INV-MP-STORE.** Blobs in the store are immutable and content-addressed; a blob's name is
  the sha256 of its bytes; nothing rewrites a blob in place.
- **INV-MP-HFRO.** DRAKKAR never writes to, renames within, or deletes from the HF hub
  cache. Interop is strictly read-side (clonefile/hard-link/copy).
- **INV-MP-ATOMIC.** A model becomes visible (manifest written) only after every referenced
  blob is fully present and verified; all publications are temp-file + atomic rename on the
  same volume.
- **INV-MP-NOCODE.** No artifact ever causes code execution: no pickle, no
  `trust_remote_code`, no template-defined I/O (RFC-0001 A11).
- **INV-MP-GC.** Garbage collection never removes a blob referenced by any manifest,
  including manifests written concurrently with the collection.

### Reference resolution

- MP1. Accepted forms, normalized to `(repo_id, revision)`: full URLs
  (`https://huggingface.co/Qwen/Qwen3-8B`, `hf.co/...`, including `/tree/<rev>` and file deep
  links), bare `org/repo`, `org/repo@revision`, curated aliases (`qwen3:8b`, `gpt-oss:20b`)
  from a shipped, user-extensible alias table, and local paths (directory with `config.json`
  + safetensors, or a `.gguf` file).
- MP2. Resolution fetches only metadata first (model card header, `config.json`, file
  listing, safetensors index): enough for the fit preflight (RFC-0004 FE1) without
  downloading weights. Gated/private repos use the standard HF token discovery (env,
  `~/.huggingface`, keychain — RFC-0001 A12; tokens never appear in logs); a gated repo
  without a token produces a named error with the acceptance URL.
- MP3. Sibling discovery: given a base repo, the resolver locates known-good quantized
  siblings (same model, mlx-community and original-org quant repos, GGUF repos) via a
  shipped mapping plus HF search, so remedies in the fit report ("use the official 4-bit")
  are one keypress, not a research project.

Input grammar (parsed in this precedence order; first match wins):

```text
model_ref  := local_path | url | alias | repo_spec
url        := scheme? host "/" org "/" repo suffix?
scheme     := "https://" | "http://"
host       := "huggingface.co" | "hf.co" | "www.huggingface.co"
suffix     := "/tree/" rev
            | ("/blob/" | "/resolve/") rev "/" path      # file deep link
repo_spec  := org "/" repo ("@" rev)?                    # rev: branch, tag, or commit sha
alias      := name (":" tag)?                            # no "/", no scheme; e.g. qwen3:8b
local_path := existing dir containing config.json        # safetensors/MLX layout
            | existing file ending ".gguf"
```

Normalized output type (in `drakkar-core`, consumed by fit, models, and CLI):

```rust
pub enum ResolvedRef {
    /// A hub repo pinned to a revision. `file_hint` is set for file deep links
    /// (e.g. a single .gguf inside a multi-quant repo) and biases artifact selection.
    Hub { repo_id: String, revision: Revision, file_hint: Option<String> },
    LocalDir { path: PathBuf },   // config.json + safetensors/MLX weights
    LocalGguf { path: PathBuf },
}

pub enum Revision { Main, Named(String), Sha(String) }  // Named = branch or tag
```

Rules:

- A `Revision::Main` resolves to the commit sha at metadata-fetch time and is recorded as
  `Sha` in the manifest; the store never contains a floating revision (this makes KV12 cache
  keys and dedup well-defined).
- File deep links to a specific `.gguf` set `file_hint`, which pins artifact selection to
  that file (MP4 route 4) — the user asked for exactly that quant.
- Local paths bypass resolution and download entirely but still run the fit preflight and
  register in the store by reference (a manifest whose blobs are `link` entries pointing at
  the external path; `drakkar ls` marks them `external`).

Alias table (LD3 — resolved): the table ships inside the binary, is user-extensible, and is
refreshed only by an explicit `drakkar alias update` (fetches a signed manifest; never
automatic, never at resolve time — resolution works fully offline). Layering, later wins:
built-in table → `~/.config/drakkar/aliases.toml` → `drakkar alias add`. Per RFC-0008 LD16,
a user alias that shadows a shipped one wins, with a one-line warning on use. Entry schema:

```toml
# ~/.config/drakkar/aliases.toml   (same schema as the built-in table)
[alias."qwen3:8b"]
repo    = "Qwen/Qwen3-8B"                    # canonical source repo
prefer  = "mlx-community/Qwen3-8B-4bit"      # optional: known-good quant shortcut
gguf    = "Qwen/Qwen3-8B-GGUF"               # optional: Backend B sibling
```

The sibling-discovery mapping (MP3) uses the same file format, keyed by canonical repo, and
ships in the binary next to the alias table; `drakkar alias update` refreshes both. When the
mapping has no entry, the resolver falls back to a bounded HF search (same-org name-pattern
match) and labels results `unverified` in the remedy list.

### Artifact selection policy

- MP4. Preference order for Backend A (MLX): (1) MLX-format repo at the fit-recommended bits
  (mlx-community hosts ~4,800 conversions), (2) original safetensors bf16/fp16 if it fits
  directly, (3) original safetensors + on-device quantization
  ([Conversion](#conversion-and-on-device-quantization)). Backend B path: (4) GGUF repo at
  the closest quant when no MLX/safetensors route exists or the architecture is unsupported
  in A.
- MP5. The selector is fit-driven: it asks `drakkar-fit` for the target bpw and picks the
  artifact whose effective bpw is closest without exceeding the plan. `--quant` / `--format`
  override. The chosen route is always displayed (provenance is part of "honest speed").
- MP6. Multi-file GGUF splits, sharded safetensors, and consolidated single files are all
  handled; `.pth`/pickle checkpoints are rejected (security, RFC-0001 A11) with a pointer to
  conversion guidance.

Selection contract (pure function; unit-tested against a fixture matrix):

```rust
pub struct ArtifactChoice {
    pub route: Route,             // Mlx | SafetensorsDirect | ConvertOnDevice | Gguf
    pub repo_id: String,          // may differ from input repo (sibling)
    pub revision: Revision,
    pub files: Vec<RepoFile>,     // exact file set to fetch
    pub effective_bpw: f32,       // what fit arithmetic used (RFC-0004 FE5/FE6)
    pub post_steps: Vec<Step>,    // e.g. Quantize { bits, group, recipe }
}

pub fn select(desc: &ModelDescriptor, plan: &FitPlan, overrides: &SelectOverrides)
    -> Result<ArtifactChoice, SelectError>;
```

`SelectError` variants map one-to-one to named errors: `NoRoute` (no artifact fits even with
remedies — surfaces the fit report's remedy list), `UnsupportedArchitecture` (no backend
handles the graph — named error, never arbitrary code, RFC-0001 A11), `PickleOnly` (repo has
only `.pth`/pickle — rejected with conversion guidance). The chosen route, source repo, and
effective bpw are printed at normal verbosity and included in `--json` output
(RFC-0008 CLI6).

### Download

- MP7. Parallel ranged downloads with per-file resume; Xet/CDN-aware via the hf-hub crate;
  default 4 connections, saturating typical links without starving the machine. Progress UX
  per RFC-0008 (bytes, ETA, post-download step preview).
- MP8. Integrity: verify per-file sizes and ETag/sha256 from the hub where offered;
  safetensors headers parsed defensively (bounded header size, no allocations from untrusted
  lengths); GGUF metadata parsed with the same discipline.
- MP9. Disk preflight: required = download + (conversion workspace if any) + output; refuse
  with a clear number when the volume lacks space, before starting.

Mechanics:

- Downloads land in `<store>/tmp/<manifest-key>/` on the same volume as the store (so the
  final rename is atomic, INV-MP-ATOMIC). Each file carries a `.part` suffix plus a JSON
  sidecar recording expected size, ETag/sha256, and completed byte ranges; resume validates
  the sidecar against the hub's current metadata and restarts only files whose remote
  changed.
- Integrity policy: when the hub provides a sha256 (LFS/Xet files), it is verified and
  becomes the blob name directly. When only an ETag/size is offered, the file is verified
  against those and the blob name is the locally computed sha256. A mismatch after retry is
  the named error `download.integrity_mismatch` naming the file and both digests; the `.part`
  is discarded, never registered.
- Defensive parsing bounds: safetensors header length is read as a u64 and rejected above
  100 MB (well above any real model's header) before any allocation; JSON header parsing uses
  the `safetensors` crate's zero-copy path. GGUF metadata: KV count, string lengths, and
  tensor counts are each bounds-checked against the file size before allocation
  (`drakkar-gguf`, LD24).
- Disk preflight formula, evaluated against the store volume (which may be external, LD14):
  `required = Σ download_bytes + conversion_workspace + output_bytes − reusable_bytes`,
  where `conversion_workspace` ≈ one shard (RFC-0003 IC8 streaming bound), `output_bytes`
  comes from the fit estimator's quantized-size arithmetic (RFC-0004 FE5/FE6), and
  `reusable_bytes` counts already-present blobs and clonable HF-cache files (clones cost
  ~0 bytes on APFS, full bytes elsewhere — the preflight checks the filesystem type).
  Refusal is the named error `download.no_space` with required vs available in bytes
  and the store path, before any transfer starts.
- Concurrency: a per-manifest-key advisory lock (`<store>/locks/<key>.lock`, `flock`)
  serializes pulls of the same model across processes (foreground `run` + daemon,
  RFC-0008 CLI13). The second process blocks with a "waiting for concurrent pull" progress
  line, then observes the completed manifest and skips the download. Pulls of different
  models proceed in parallel; the store lock is only taken for manifest/GC mutations, not
  for the duration of a download.

### Storage layout

- MP10. Content-addressed store: `~/.drakkar/models/blobs/sha256-*` with human-readable
  manifests `~/.drakkar/models/manifests/<org>/<repo>/<rev>.json` mapping names to blobs
  (Ollama-style, proven). Identical tensors shared across revisions dedupe by construction.
- MP11. HF cache interop: if `HF_HOME`'s hub cache already holds needed files, they are
  **hard-linked or reflinked (APFS clonefile)** into the store rather than re-downloaded;
  DRAKKAR never mutates the HF cache (INV-MP-HFRO). `storage.import_hf_cache = "clone" |
  "copy" | "off"` (LD4 — default `clone`).
- MP12. `drakkar ls` lists installed models with size, format, quant, last-used; `drakkar rm`
  removes manifests and garbage-collects unreferenced blobs; `drakkar prune` reports
  reclaimable space first.

Store root: `storage.path` in config (RFC-0008 CLI10), default `~/.drakkar/models`,
supported on any volume — including external — from v0.1 (LD14). Layout:

```text
<storage.path>/
  blobs/sha256-<hex>            # immutable content-addressed files (INV-MP-STORE)
  manifests/<org>/<repo>/<rev>.json
  manifests/_local/<name>.json  # registered local paths and drakkar-converted outputs
  tmp/<manifest-key>/           # in-flight downloads/conversions (same volume)
  locks/<manifest-key>.lock     # advisory pull locks
```

Manifest schema (versioned, additive-only within the major):

```json
{
  "schema": "drakkar.manifest/1",
  "repo_id": "mlx-community/Qwen3-8B-4bit",
  "revision": "9f1c2ab…",
  "format": "mlx",
  "quant": { "bits": 4, "group": 64, "recipe": "mlx-affine-v1" },
  "files": [
    { "name": "model-00001-of-00002.safetensors",
      "blob": "sha256-3e2a…", "size": 5312452608 },
    { "name": "tokenizer.json", "blob": "sha256-77b1…", "size": 11422378 },
    { "name": "config.json",    "blob": "sha256-0c4d…", "size": 1204 }
  ],
  "tokenizer_hash": "sha256-77b1…",
  "template": { "source": "repo", "override": null, "hash": "sha256-a9e0…" },
  "provenance": { "source": "hub", "parent": null,
                  "created": "2026-07-14T00:00:00Z" },
  "last_used": "2026-07-14T00:00:00Z"
}
```

`provenance.source ∈ {hub, converted, external}`; converted artifacts set `parent` to the
source manifest key so `drakkar ls` can show lineage and `rm` can warn when removing a
conversion's source. `tokenizer_hash` and `template.hash` feed KV cache keys
(RFC-0005 KV12).

Interop semantics (LD4): with `clone`, files present in the HF hub cache are reflinked via
APFS `clonefile(2)` (fall back to hard-link on same-volume non-APFS, fall back to copy
across volumes — the fallback is reported in progress output, since it changes disk cost).
Cloned blobs are still renamed to their sha256 name after verification; the HF cache remains
byte-identical throughout (INV-MP-HFRO). `copy` forces full copies (for users who plan to
delete the HF cache); `off` disables interop entirely.

Garbage collection: `rm` deletes the manifest, then collects blobs whose reference count
across all manifests is zero. GC takes the store lock, snapshots the manifest set, and only
deletes blobs unreferenced in that snapshot AND older than the oldest in-flight `tmp/` entry
(so a concurrent pull's not-yet-published manifest can never lose blobs — INV-MP-GC).
`prune` runs the same computation read-only and prints reclaimable bytes per model. Both
honor `--json` (RFC-0008 CLI6).

### Conversion and on-device quantization

- MP13. Converter runs in-process (Rust orchestration, backend kernels for quantize):
  safetensors(bf16) → MLX affine at recipe bits/group (RFC-0004 FE6 recipes shared with the
  estimator). Streaming per-tensor (RFC-0003 IC8): peak memory ≈ one shard + output tensor; a
  70B fp16 → 4-bit conversion MUST succeed on a 48 GB machine.
- MP14. Throughput target: conversion bounded by SSD read + quantize compute; ≥ 1 GB/s of
  input on M4-class (est., benchmark in RFC-0009); progress and cancellation supported;
  output lands atomically (temp + rename, INV-MP-ATOMIC).
- MP15. Calibration-based schemes (AWQ/GPTQ-style import, activation-aware low-bit) are
  v1.x: v1 imports pre-quantized artifacts of those families where the backend supports the
  layout, and otherwise recommends official quants (the honest answer beats a bad on-device
  2-bit).
- MP16. `drakkar convert <model> --bits B --group G [--recipe R]` exposes the pipeline
  directly; converted artifacts register in the store like downloads and may be pushed back
  to the hub by the user with standard tools (out of scope to upload for them in v1).

Recipe tables are the single source of truth shared between the fit estimator (RFC-0004 FE6)
and the converter, so the predicted artifact size and the produced artifact size agree
(AC4 bounds the divergence at 1%). A recipe names, per tensor class: bits, group size, and
exceptions (embeddings/lm_head at their recipe bits, norms fp32, designated sensitive layers
at 8-bit). v0.1 ships the `mlx-affine-v1` recipe family (4/5/6/8-bit, group 32/64).
Cancellation (Ctrl-C or API) deletes the `tmp/` workspace and leaves the store untouched;
re-running restarts conversion from the last completed shard recorded in the workspace
sidecar.

### Chat templates, tokenizers, tool formats

- MP17. Tokenizers load via the HF tokenizers crate from `tokenizer.json` (fast path) with
  sentencepiece fallback (`tokenizer.model` converted at load through the same crate's
  compatibility layer); the tokenizer hash feeds cache keys (RFC-0005 KV12).
- MP18. Chat templates: the repo's Jinja template is executed by a sandboxed minijinja
  environment with the standard HF template API surface (no filesystem, network, or process
  access; bounded recursion and output size — INV-MP-NOCODE); a curated override table
  patches known-broken templates per model+revision (versioned, tested). Tool-call and
  reasoning-block dialects are declared per model family in the model-def layer and drive
  both prompt rendering and stream parsing (RFC-0007
  [§ Tools, structured output, reasoning content](RFC-0007-api-server.md#proposed-design)).
- MP19. `drakkar run` prints which template and tool dialect are active at debug verbosity;
  mismatch bugs are the top silent-quality-killer in local serving and MUST be observable.

The template override table ships in the binary (same update channel as the alias table,
LD3: refreshed only by `drakkar alias update`), keyed by `(repo_id, revision-pattern)`, and
every entry carries the reason and a regression fixture (see
[Testing Strategy](#testing-strategy)). A template that fails to render — and for which no
curated override exists in this build's model-def layer — is the named error
`models.unsupported_architecture` with the Jinja error position; the engine never silently
falls back to a generic template, because a wrong-but-working prompt is worse than a visible
failure (MP19).

### Failure modes

Every failure below maps to a stable error code in the taxonomy (`drakkar-core`,
RFC-0011; rendering rules per RFC-0008 CLI15; exit codes per CLI8).

| Stage | Failure | Named error | System response |
| --- | --- | --- | --- |
| Resolve | Unparseable reference | `models.not_found` | Show the accepted forms (MP1 grammar), nearest-alias suggestion |
| Resolve | Alias not found | `models.not_found` | List close matches; hint `drakkar alias update` |
| Resolve | Repo/revision does not exist | `models.repo_not_found` | Verbatim hub response, no retry |
| Resolve | Gated repo, no/insufficient token | `models.gated_repo_no_token` | Print the acceptance URL and token setup instructions (MP2) |
| Resolve | Network unreachable | `download.hub_unreachable` | If the model is already in the store, proceed offline; else fail with retry hint |
| Select | No supported artifact/architecture | `models.unsupported_architecture` | Name the architecture; suggest GGUF sibling if one exists (MP3) |
| Select | Pickle-only repo | `models.pickle_rejected` | Refuse (RFC-0001 A11); point at conversion guidance (MP6) |
| Preflight | Insufficient disk | `download.no_space` | Refuse before transfer with required vs available bytes (MP9) |
| Download | Size/digest mismatch after retry | `download.integrity_mismatch` | Discard `.part`; name file and digests; never register (MP8) |
| Download | Interrupted (signal, network) | — (not an error on rerun) | Resume from byte ranges on next invocation (MP7, AC2) |
| Verify | Malformed safetensors/GGUF header | `download.integrity_mismatch` | Reject before allocation (MP8); name the file and offset |
| Convert | Cancelled | — | Delete workspace; store untouched; resumable per shard |
| Convert | Out of memory | `internal.budget_breach` | Should not occur within IC8 envelope; if it does, report shard and suggest smaller group/external help |
| Template | Render failure | `models.unsupported_architecture` | Fail visibly with Jinja position; never substitute a generic template (MP19) |
| Store | Concurrent pull of same model | — | Second process waits on the lock, then reuses the result |
| Store | GC vs concurrent pull race | — | Prevented by INV-MP-GC (snapshot + tmp-age guard) |

### External dependencies

All pinned exactly per release via the workspace lockfile (RFC-0002 discipline; LD24 crate
map). Version constraints are the majors tracked at pin time:

| Crate | Constraint | Reason |
| --- | --- | --- |
| `hf-hub` | 0.x, pinned minor | Hub API, Xet/CDN-aware ranged downloads, standard token discovery (MP2, MP7); reimplementing Xet is not product |
| `tokenizers` | 0.x, pinned minor | `tokenizer.json` fast path + sentencepiece compatibility (MP17); byte-exact parity with upstream tokenization |
| `safetensors` | 0.x, pinned minor | Zero-copy header parsing with the bounded-allocation discipline MP8 requires |
| `minijinja` | 2.x | Sandboxed Jinja with the HF template API surface (MP18); no I/O by construction |
| `sha2` | 0.10.x | Blob content addressing (MP10) |
| `fs2` (or direct `flock`) | 0.4.x | Advisory pull locks (cross-process, MP7 concurrency) |

GGUF parsing is in-house in `drakkar-gguf` (LD24, cargo feature): the format is simple
enough that owning the bounded parser is cheaper than auditing a third-party one to the MP8
standard.

## Alternatives Considered

- **Use the HF hub cache as THE store.** Point DRAKKAR at `HF_HOME` and download through
  hf-hub's own cache layout, no separate store. Rejected: the hub cache is not
  content-addressed (no cross-revision dedup, no integrity-by-name), has no reference
  counting so safe GC is impossible (`drakkar rm` could break the user's other tools, or
  nothing would ever be reclaimable), and couples our on-disk layout to an external tool's
  internals — a layout change upstream would be a data migration for us. The chosen design
  (LD4) gets the disk win anyway: `clone` interop reflinks existing cache files at ~zero
  disk cost while our store keeps its own invariants (INV-MP-STORE, INV-MP-GC) and never
  mutates theirs (INV-MP-HFRO).
- **git/git-lfs clones of model repos.** `git clone` each repo; revisions come free.
  Rejected: LFS materializes a second full copy of every weight file (`.git/lfs/objects` +
  working tree), doubling disk for 40 GB artifacts; clone is slower than parallel ranged
  HTTP and resumes poorly mid-file; and it drags a git dependency into a single-binary
  product (S2-adjacent). Content addressing plus recorded revision shas give the same
  pinning without the duplication.
- **On-the-fly conversion at load time.** Skip the stored converted artifact; quantize
  bf16 → 4-bit during every model load (mistral.rs-style ISQ). Rejected as the default: it
  re-pays the conversion cost (minutes for large models, MP14) on every cold load, defeating
  P12's SSD-bounded load target, and it makes load-time memory the sum of streaming
  conversion and engine warmup instead of mmap-only. Converting once at pull time (MP13)
  amortizes the cost to zero and makes the artifact's bpw a stored, inspectable fact the fit
  engine can trust. The store's dedup means the one-time output cost is the only cost.
- **Python conversion sidecar.** Shell out to `mlx_lm.convert` for the safetensors → MLX
  path — battle-tested, zero implementation cost. Rejected: it violates RFC-0002 S2
  (any Python requirement reintroduces the top install-failure class the product exists to
  eliminate, PRD §2.3) and would make conversion the one feature that breaks on a clean
  machine. The in-process converter reuses the backend's own quantize kernels through the
  same C ABI shim (RFC-0001), so there is no second numerics implementation to keep in sync.

## Drawbacks

- **Disk duplication when clone is unavailable.** On non-APFS store volumes (some external
  drives), `clone` degrades to copy, and users with a populated HF cache pay the bytes
  twice. The degradation is reported at pull time and `drakkar prune` accounts for it, but
  the cost is real; the alternative (mutating or adopting the HF cache) violates INV-MP-HFRO
  and was rejected above.
- **Alias and override staleness between releases.** The alias table, sibling mapping, and
  template override table ship in the binary (LD3), so a model released the day after a
  DRAKKAR release has no alias and a newly discovered broken template no override until the
  user runs `drakkar alias update` or upgrades. This is the accepted trade against making
  resolution depend on the network; the explicit-refresh command is the mitigation, and full
  URLs/`org/repo` forms always work regardless.
- **Curated mappings are ongoing maintenance.** Sibling discovery (MP3) and template
  overrides (MP18) are curated tables that track a weekly-moving ecosystem. The cost is
  bounded (entries are small, fixture-tested, and batched into releases) but it is a
  permanent editorial commitment, not a one-time build.
- **Two artifact copies during conversion.** The convert route briefly needs source shards +
  output on disk (bounded by the MP9 preflight formula), which can refuse on small internal
  SSDs even when the final artifact would fit. `storage.path` on an external volume (LD14)
  is the documented remedy.

## Migration / Rollout

- **v0.1 "First light".** Full resolution grammar (MP1) with the shipped alias table (LD3);
  metadata-first fetch and fit-preflight ordering (MP2, INV-MP-ORDER); artifact selection
  routes 1–3 (MLX repos, direct safetensors, on-device quantization) — route 4 (GGUF)
  resolves and errors with "GGUF backend lands in v0.2" per LD24 feature gating; download
  with resume, integrity, disk preflight (MP7–MP9); content-addressed store, HF-cache clone
  interop, `ls`/`rm`/`prune` (MP10–MP12); basic on-device quantization: bf16/fp16 → MLX
  affine 4/5/6/8-bit recipes (MP13–MP14) via `pull`/`run` auto-convert and a minimal
  `drakkar convert`; tokenizers and templates with the initial override table (MP17–MP19).
  `storage.path` on any volume works from day one (LD14). Manifest schema `drakkar.manifest/1`.
- **v0.2 "Convoy".** GGUF route live behind the `drakkar-gguf` cargo feature shipping in the
  default binary (selection route 4, multi-file splits); sibling-remedy integration with the
  fit report's remedy ranking (MP3 one-keypress flow); `drakkar alias update` channel for
  alias/sibling/override tables; conversion throughput benchmarked against the ≥ 1 GB/s
  target on the RFC-0009 fleet (MP14 `est.` becomes measured).
- **v0.3 "Fleet".** `convert` UX polish per the roadmap: recipe presets, `--dry-run` size
  preview using the shared estimator tables, batch conversion, lineage display in `ls`;
  documented external-volume workflows for `storage.path` (the key itself exists since v0.1,
  LD14 — v0.3 adds docs and `doctor` checks for external-volume pitfalls: filesystem type,
  clone support, disconnect behavior).
- **v1.0 "Harbor".** No new pipeline surface; the desktop app consumes the same store via
  the C ABI. Any manifest schema evolution ships as `drakkar.manifest/2` with a one-shot
  forward migration on first write; `/1` remains readable for the entire 1.x series.

Schema/compat rules: manifests and `--json` outputs are additive-only within a major schema
version; blobs never migrate (content addressing is version-free); the alias/override table
format is versioned with the same additive rule.

## Testing Strategy

Folded acceptance criteria (from the source draft) — each is a CI-gated integration test:

- AC1. Each of these resolves and runs with zero flags on a suitable machine: a full HF URL
  to an mlx-community 4-bit repo; `Qwen/Qwen3-8B` (bf16 source, auto-quantized); a GGUF repo
  URL (v0.2); a local GGUF file (v0.2); an alias.
- AC2. Kill during download at 60%, rerun: completes with no re-downloaded completed files
  (byte-range accounting asserted from the resume sidecars).
- AC3. HF-cache interop: pre-seeded hub cache yields zero network bytes for weights and
  clone-level disk cost (asserted via `st_blocks` on APFS).
- AC4. 70B bf16 → 4-bit on a 48 GB machine: succeeds within the RFC-0003 IC8 memory
  envelope; output bpw matches the estimator within 1%.
- AC5. Pickle checkpoint: rejected with the documented error (`models.pickle_rejected`) and
  remedy text.

Additional named suites:

- **Resolver table tests (unit).** One table row per MP1 form — every URL variant
  (`https`, bare host, `/tree/`, `/blob/`, `/resolve/`, file deep link), `org/repo`,
  `org/repo@branch|tag|sha`, alias with and without tag, user-alias shadowing (LD16 warning
  asserted), local dir, local `.gguf`, plus rejection rows (`models.not_found` for both an
  unparseable reference and an unknown alias). The table is the grammar's executable spec.
- **Integrity-failure injection (integration).** A local hub stub serves a corrupted shard
  (flipped byte, truncation, wrong length header); assert `download.integrity_mismatch`
  names the file, no manifest is written, and no partial blob is registered
  (INV-MP-ATOMIC).
- **Disk-preflight boundary tests (unit + integration).** Preflight formula evaluated at
  available = required − 1 byte (refuse, `download.no_space` with correct numbers),
  = required (proceed), and with `reusable_bytes` from pre-seeded blobs and clonable HF-cache
  files on APFS vs non-APFS fixtures (fallback path changes the number).
- **Template-override regression corpus (golden fixture).** Every override-table entry
  carries a fixture: (messages, tools) input → rendered prompt golden output, run against
  both the broken upstream template (must differ) and the override (must match golden).
  Adding an override without a fixture fails CI.
- **Tokenizer-hash stability test (golden fixture).** Known `tokenizer.json` and
  `tokenizer.model` fixtures hash to pinned values across releases (KV12 cache keys must
  not churn on a dependency bump); a hash change without a manifest schema note fails CI.
- **Concurrent pull locking test (integration).** Two processes pull the same model
  simultaneously against the hub stub: exactly one downloads, the second blocks then reuses
  the manifest; total network bytes equal one download.
- **Store GC property test.** Generator produces arbitrary interleavings of pull, `rm`,
  `prune`, GC, and in-flight `tmp/` entries; property: no blob referenced by any surviving
  or concurrently-published manifest is ever deleted (INV-MP-GC), and every unreferenced
  blob is eventually collected.
- **Defensive-parser fuzzing.** Fuzz safetensors headers and GGUF metadata (structured
  fuzzing on lengths/counts); property: no allocation above the declared bounds, no panic,
  malformed inputs always yield `download.integrity_mismatch`.
- **Conversion soak (RFC-0009 fleet).** MP14 throughput measured per chip class with peak
  RSS asserted against the IC8 envelope; results feed the published benchmark manifests
  (LD18).

## Open Questions

None. The source draft's open questions are resolved:

- Alias table governance → resolved as LD3: ship-in-binary, user-extensible, explicit
  `drakkar alias update` refresh only ([Reference resolution](#reference-resolution)).
- Store location on external volumes → resolved as LD14: `storage.path` supported from
  v0.1; v0.3 adds documentation and `doctor` checks
  ([Migration / Rollout](#migration--rollout)).
- HF cache sharing vs read-only linking (PRD OQ5) → resolved as LD4: `clone` default,
  read-only, never mutate ([Storage layout](#storage-layout)).

## References

- [PRD](../../PRD.md) — G1, P1–P3, P9, §2.3, §8 roadmap
- [RFC-0001: Architecture](RFC-0001-architecture.md) — A11 (artifacts are data), A12 (token handling)
- [RFC-0002: Stack Selection](RFC-0002-stack-selection.md) — S2 (single binary), D1 (crate map)
- [RFC-0003: Inference Core](RFC-0003-inference-core.md) — IC8 (streaming quantization), IC9 (recipes)
- [RFC-0004: Feasibility Engine](RFC-0004-feasibility-engine.md) — FE1 (descriptor), FE5/FE6 (size arithmetic, shared recipes)
- [RFC-0005: KV Cache](RFC-0005-kv-cache.md) — KV12 (correctness keys)
- [RFC-0007: API Server](RFC-0007-api-server.md) — tool-call and reasoning dialects
- [RFC-0008: CLI and UX](RFC-0008-cli-ux.md) — CLI6/CLI8/CLI10/CLI15, LD16 alias shadowing
- [RFC-0009: Performance](RFC-0009-performance.md) — conversion throughput benchmark, LD18 manifests
- [RFC-0011: Error Taxonomy](RFC-0011-error-taxonomy.md) — stable error codes for the failure-mode table
- [Error model](../spec/04-error-model.md) — rendering and exit-code mapping
- huggingface/hf-hub, tokenizers, and safetensors crates; safetensors format spec
- mlx-community organization scale (~4,800 models, WWDC26 figure); mlx_lm.convert flow (Apple MLR post)
- Ollama blob/manifest store design (prior art); mistral.rs ISQ (in-situ quantization) precedent
- ggml-org GGUF spec; LM Studio model-folder discovery precedent (mlx-serve README)
