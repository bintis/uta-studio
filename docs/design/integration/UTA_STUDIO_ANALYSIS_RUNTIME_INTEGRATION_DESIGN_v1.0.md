# Uta! Studio — Analysis Engine / Runtime Manager Reintegration Design v1.0

**Status:** implementation design
**Date:** 2026-08-22
**Scope:** Uta! Studio desktop + `app-core` integration with `uta-analysis-engine` and `uta-runtime-manager`
**Primary source assumption:** `TrueSource` is an authorized **local filesystem file**.

**Architecture authority:** `docs/design/architecture/UTA_SEPARATED_ARCHITECTURE_DESIGN_v1.0.md` controls system/component boundaries; `docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md` controls audio-analysis semantics. This document is a supporting Studio integration specification and must not override either.

---

# 1. Purpose

This document defines how the Uta! Studio application integrates the standalone Analysis Engine and authoritative Runtime Manager that now exist in this repository.

It is intentionally Studio-specific. It does not replace the Analysis Engine or Runtime Manager architecture guides. It specifies the seam between those components and the existing Studio product model:

```text
Library / Song / TrueSource
        ↓
Studio product intent
        ↓
AnalyzeRequestV1
        ↓
uta-analysis-engine
        ↓
run-scoped output directory
        ↓
validated result manifest
        ↓
Studio Artifact Store + history
        ↓
Candidate review / editor / export
```

The central rule is:

> **Studio owns user intent, local source authorization, queue/history, artifact revisions, review and export. Analysis Engine owns analysis planning/execution. Runtime Manager owns resource lifecycle and production usability.**

---

# 2. Authority and relationship to existing guides

This design must be implemented consistently with:

```text
AGENTS.md
tasks/remaining-models/STATE.md
docs/KEY_CONCLUSIONS.md
docs/design/architecture/UTA_STUDIO_CLI_PROCESS_BOUNDARY_v1.0.md
docs/design/integration/UTA_STUDIO_ANALYSIS_SETTINGS_MODEL_SELECTION_EXECUTION_UX_DESIGN_v1.0.md
```

Where this document is more specific about the Studio integration seam, this document controls that seam. `docs/design/architecture/UTA_STUDIO_CLI_PROCESS_BOUNDARY_v1.0.md` controls the final Studio/backend dependency direction. `AGENTS.md` controls current repository rules, and `tasks/remaining-models/STATE.md` records current closure state.

It does not override:

- Runtime Manager resource validation policy;
- Analysis Engine request/result contracts;
- UTZ 0.3 format behavior;
- source-media read-only rules;
- native-only production inference;
- Wayland-only Linux packaging;
- explicit confirmation requirements for downloads/installations.

---

# 3. Current repository checkpoint

The repository already contains the following foundations.

## 3.1 Analysis Engine

`analysis-engine/` is independent from `app-core` and currently provides:

- versioned request/result/error/capability contracts;
- `AnalyzeRequestV1` with local-file audio sources;
- requirements and policy-aware planning;
- deterministic execution fingerprint infrastructure;
- SHA-256 source verification;
- FFmpeg streaming decode and canonical integer timeline facts;
- bounded NDJSON worker protocol;
- `uta-analysis-engine` / `uta-analyze` binaries;
- a compatibility worker for the old `uta-native-analyzer` command shape;
- migrated singing calibration/fusion/HSMM core.

The complete execution path is still fail-closed until all mandatory native capabilities, especially real GAME execution, are implemented and production-validated.

## 3.2 Runtime Manager

`runtime-manager/` is the authoritative resource catalog/resolver and already provides:

- typed resource identities;
- install/readiness/validation states;
- production vs benchmark policy;
- catalog-backed status/resolve/doctor APIs;
- managed-store / generation foundations;
- immutable generation leases;
- deterministic resource identities used by Engine fingerprints;
- mutation API foundations.

Studio read APIs already partially delegate to Runtime Manager.

## 3.3 Studio

Studio still contains a substantial legacy orchestration layer:

- `app-core/src/analysis_graph.rs`;
- `app-core/src/analysis_plan.rs`;
- `app-core/src/analyzer/*` queue/control/server/run logic;
- the old native-analyzer protocol adapter;
- Studio-owned analysis history and per-node attempts;
- immutable Artifact Store and revision lineage;
- Artifact Workbench;
- Processing Studio product workflows;
- Analysis graph UI;
- Models & runtime UI.

These Studio features are not to be discarded. Their ownership must be clarified and their execution truth must move to the new Engine/Runtime Manager boundaries.

---

# 4. Terminology

## 4.1 TrueSource

In this document, **TrueSource** means the user's canonical source media file for a song.

For the current product:

```text
Song.origin == LocalFile
Song.path   == TrueSource path
```

TrueSource is:

- local;
- user-owned/source media;
- read-only to Uta! Studio analysis;
- never moved, rewritten or deleted by analysis;
- not an Engine-generated stem;
- not a compatibility preview;
- not an Artifact Store revision;
- not a model/runtime resource.

Studio may create derived local files, but those are generated artifacts and must never be confused with TrueSource.

## 4.2 Library identity vs Engine source identity

These are intentionally different concepts.

Current Studio song identity:

```text
Song.file_hash
= BLAKE3-derived 32-hex application/library identity
```

Engine source identity:

```text
AudioSourceV1.sha256
= full 64-hex SHA-256 of the exact local file bytes
```

**Never copy `Song.file_hash` into `AudioSourceV1.sha256`.**

Studio keeps using `Song.file_hash` as the library/cache/database key. The Engine request compiler computes a real SHA-256 for the authorized local source.

## 4.3 Engine run root

A unique Studio-authorized directory created for one Engine request, for example:

```text
<studio-cache>/engine-runs/<request_id>/
```

The Engine may write only below this root for that request.

## 4.4 Candidate vs Authored chart

An Engine-generated VocalChart is a **Candidate**.

It never directly overwrites an Authored chart.

The existing editor/revision workflow remains authoritative for human edits.

---

# 5. Goals

The integration must achieve all of the following.

1. Studio compiles product/user intent into the canonical Engine request contract.
2. Studio uses the real local TrueSource path and a real SHA-256.
3. Studio preview and execution use the same exact request payload.
4. Analysis Engine is the only authority for analysis requirements and execution-plan semantics.
5. Runtime Manager is the only authority for resource install/readiness/policy usability.
6. Studio keeps queue/history/artifact revision/editor UX.
7. Engine output is validated before it enters the Studio Artifact Store.
8. A failed or cancelled run cannot partially activate new artifacts.
9. Missing GAME blocks only requests that actually require GAME.
10. Independent partial requests such as stems/transcript/pitch remain independently plannable.
11. No normal Studio read path downloads or mutates model/runtime state.
12. No production Python, HTTP or network-service inference path is introduced.
13. Existing source media remains read-only.
14. Existing Authored charts remain preserved across re-analysis.
15. The old compatibility wrapper is removed only after the new path is proven end to end.

---

# 6. Non-goals

This integration does not:

- move Runtime Manager into Studio;
- move model preprocessing or tensor construction into Studio;
- make the Engine a persistent queue scheduler;
- make Studio understand model filenames as workflow APIs;
- make `uta-runtime` the desktop integration mechanism;
- turn Engine into an HTTP service;
- make source media writable;
- redesign UTZ 0.3;
- silently replace GAME;
- claim granular Freeze/Bypass semantics are supported where Engine v1 cannot represent them.

---

# 7. Ownership boundaries

## 7.1 Studio owns

Studio owns:

```text
library indexing
Song identity
TrueSource authorization
user/session intent
Global Defaults / Song Profile / Run Override UI
Processing Studio product workflow definitions
queueing
run history
per-node presentation/history
Artifact Store
artifact revisions and pinning
Candidate vs Authored selection
editor
UTZ / UltraStar export UX
explicit install confirmation UX
data-root relocation
error presentation
```

Studio may compile these concepts into Engine contracts. It must not execute model-specific analysis itself.

## 7.2 Analysis Engine owns

Analysis Engine owns:

```text
AnalyzeRequest validation
semantic input routing
requirements
Engine Plan
runtime-resolution requests
FFmpeg decode for Engine execution
canonical integer timeline
native worker orchestration
separation
transcription
alignment
continuous pitch
GAME / note evidence
fusion
HSMM
Candidate VocalChart creation
PitchEvidence
SingingAnalysis
execution fingerprint
result manifest
Engine cancellation semantics
```

## 7.3 Runtime Manager owns

Runtime Manager owns:

```text
resource catalog
resource metadata
source/license metadata
installation state
integrity state
validation state
backend policy
managed store
generations
current pointers
leases
install/import/verify/repair/reinstall/remove
runtime/model executable resolution
production/benchmark usability
```

## 7.4 Desktop UI owns presentation only

The desktop may display Engine/Runtime Manager state but must not infer or override it.

Examples of forbidden duplicate truth:

```text
manifest exists => model usable
folder exists => model usable
model was selected => model usable
Studio boolean says installed => Engine may run it
```

---

# 8. Target component shape

The target local architecture uses **two explicit process boundaries**:

```text
┌─────────────────────────────────────────────────────────────┐
│ Uta! Studio desktop                                          │
│ UI / commands / editor / settings                          │
└──────────────────────────────┬──────────────────────────────┘
                               │ local in-process app API
┌──────────────────────────────▼──────────────────────────────┐
│ app-core                                                     │
│                                                              │
│ StudioAnalysisFacade                                         │
│  ├─ TrueSourceResolver                                       │
│  ├─ request JSON compiler                                    │
│  ├─ StudioPlanProjection                                     │
│  ├─ AnalysisQueue / History                                  │
│  ├─ AnalysisCliClient                                        │
│  ├─ RuntimeCliClient                                         │
│  └─ EngineResultCommitter                                    │
│                                                              │
│ ArtifactStore / library DB / editor domain                  │
└───────────────┬───────────────────────────────┬──────────────┘
                │                               │
                │ argv + JSON/NDJSON            │ stdio NDJSON
                ▼                               ▼
┌──────────────────────────────┐   ┌────────────────────────────┐
│ uta-runtime                  │   │ uta-analyze                │
│ Runtime Manager CLI          │   │ worker --stdio-json        │
└──────────────┬───────────────┘   └──────────────┬─────────────┘
               │                                  │
               ▼                                  ▼
       uta-runtime-manager                Analysis Engine
       store/policy/lifecycle             native worker supervision
                                                  │
                                                  ▼
                                       OpenVINO / Qwen / RoFormer
```

Studio does **not** link `uta-runtime-manager` or `uta-analysis-engine` as Rust dependencies.

No localhost server is added.

The detailed process contract is `docs/design/architecture/UTA_STUDIO_CLI_PROCESS_BOUNDARY_v1.0.md`.

---

# 9. New Studio integration module boundary

Prefer a focused `app-core` module boundary rather than spreading Engine translation logic across existing analyzer files.

Recommended shape:

```text
app-core/src/engine_integration/
├── mod.rs
├── source.rs
├── wire.rs
├── request.rs
├── analysis_cli.rs
├── runtime_cli.rs
├── preview.rs
├── events.rs
├── result.rs
├── artifact_commit.rs
└── mapping.rs
```

The exact filenames may adapt to current module size, but responsibilities must remain separated.

Do not put the entire integration into `analyzer/run.rs`.

---

# 10. TrueSource resolution

Every Engine-backed Studio run begins by resolving one exact local source.

Required algorithm:

```text
file_hash
  ↓
load Song
  ↓
require SongOrigin::LocalFile
  ↓
obtain Song.path
  ↓
canonicalize/validate local file
  ↓
verify non-empty regular file
  ↓
verify current Studio library identity has not silently changed
  ↓
compute full SHA-256
  ↓
construct Engine AudioSourceV1
```

The resolver must not:

- copy TrueSource into cache merely to analyze it;
- rewrite tags;
- transcode in Studio;
- mutate the Song path;
- silently re-key a changed song during a run.

If the file bytes no longer match the library's current `Song.file_hash`, fail with a source-identity-changed error and ask the caller to rescan/reindex. Do not analyze one set of bytes while attaching the result to another song identity.

---

# 11. Canonical primary Engine source

For a normal Studio song analysis:

```json
{
  "id": "true_source",
  "kind": "local_file",
  "path": "/absolute/authorized/song.flac",
  "sha256": "<real 64-hex SHA-256>",
  "role": "original_mix",
  "primary": true,
  "timeline": {
    "timebase": 1000000,
    "source_start": 0
  }
}
```

Studio must use canonical integer timebase values from the Engine contract.

Do not expose floating-point source offsets at this boundary.

---

# 12. Additional local audio sources

Engine v1 supports multiple local audio sources, but current planning primarily follows the designated primary source.

Studio may only provide additional sources when:

- they are exact immutable Artifact Store revisions;
- their semantic role is known;
- their SHA-256 is verified;
- their source timeline is known;
- the Engine contract and planner explicitly consume that role.

Do not add generated cache paths merely because they exist.

For initial Studio reintegration, normal full analysis should use TrueSource as the sole primary source unless a tested Engine flow explicitly requires otherwise.

---

# 13. Studio request compilation

Studio does not expose raw `AnalyzeRequestV1` construction to the UI.

Introduce a compiler:

```text
Studio product intent
+ resolved TrueSource
+ lyrics/constraints
+ product profile
+ requested artifacts
        ↓
EngineRequestCompiler
        ↓
AnalyzeRequestV1
```

The compiler is deterministic for the same resolved inputs except for the unique `request_id`.

---

# 14. Request ID

Every queued Engine run has one unique request ID.

Requirements:

- valid under Engine identifier rules;
- persisted with the queued request;
- present in history;
- present in logs;
- matched against every Engine event/result;
- never reused for a different request body.

Do not use `Song.file_hash` alone as `request_id`.

---

# 15. Lyrics route mapping

Current Studio has:

```text
TimedLrc
KnownLyrics
GeneratedLyrics
```

Engine v1 has:

```text
LyricsMode::None
LyricsMode::Reference
LyricsMode::Canonical
boundary_constraints
```

Initial mapping:

## 15.1 Generated lyrics

```text
Studio GeneratedLyrics
→ LyricsMode::None
→ no tokens
```

The Engine owns transcription and alignment as required by requested outputs.

## 15.2 Known authoritative lyrics

When the user supplied text is the canonical text to align:

```text
Studio KnownLyrics
→ LyricsMode::Canonical
→ stable token IDs
```

This prevents Studio from requiring a second transcript model merely because old configuration names mention ASR.

## 15.3 Reference lyrics

Use `LyricsMode::Reference` only when the text is explicitly a hint that the Engine may reconcile against generated transcript evidence.

Do not use `Reference` merely as another spelling of KnownLyrics.

## 15.4 Timed LRC

Timed LRC already contains timing evidence that the current Engine v1 contract cannot fully treat as a precomputed Alignment artifact.

Until a tested Engine contract supports this exact reuse semantics:

- preserve the existing Studio timed-lyrics import path;
- do not silently run ASR and call it equivalent;
- if Engine analysis is requested on top of timed LRC, translate times into explicit boundary constraints only when tokenization/timeline semantics are proven equivalent;
- otherwise mark the operation as not representable by Engine v1.

---

# 16. Boundary constraints

Studio may compile user-provided or imported timing guidance into `BoundaryConstraintV1` only when the source is known and the timeline is exact.

Rules:

- use canonical integer time;
- retain a stable `source` label;
- mark authoritative user/import timing `Hard` only when overlap/consistency validation passes;
- use `Soft` for hints;
- never invent confidence values to satisfy the schema;
- never convert imprecise UI display rounding back into hard timing.

---

# 17. Musical context

Existing song metadata may be supplied as Engine hints:

```text
BPM
key
time signature when known
```

Do not make Studio musical metadata authoritative over Engine evidence unless the Engine contract adds such an authority mode.

Current Engine context authority is a hint.

---

# 18. Product profile mapping

The Engine owns the meaning of:

```text
Fast
Balanced
Maximum
```

Studio settings may select one of these product profiles.

Old Studio settings such as:

```text
separator model name
ASR model name
alignment backend name
requested device
```

must not become stable Engine workflow identifiers.

During migration, old settings can inform the product-level profile compiler only where semantics are unambiguous. They must not force a candidate/unsupported resource past Runtime Manager policy.

## 18.1 Detailed user-selection UX is frozen separately

The detailed behavior of `Settings > Analysis`, model/provider preferences, Global → Song → Run inheritance, Processing Studio ordering, Run Analysis, and Plan Preview is defined by:

```text
docs/design/integration/UTA_STUDIO_ANALYSIS_SETTINGS_MODEL_SELECTION_EXECUTION_UX_DESIGN_v1.0.md
```

That specification is authoritative for the Studio UX seam. In particular:

- `Fast / Balanced / Maximum` is the top-level quality choice;
- target model choice is represented as `Automatic` or, once a versioned Engine preference contract exists, an explicit stable provider/resource preference; never a checkpoint filename/path;
- under current Engine v1, only preference semantics the request can actually encode may affect execution; do not expose an active explicit-provider selector yet;
- once the versioned contract exists, explicit provider preference is sticky and blocks rather than silently falling back when unavailable;
- `Automatic` may resolve another production-approved provider only when Engine policy permits it and the preview shows the actual resolution;
- Runtime Manager Production policy is a veto, not another preference tier;
- Global defaults, Song Profile and Run Override resolve independently per field as `Run > Song > Global`;
- Processing Studio may reorder safe product-semantic audio transformations but cannot violate Engine dependency order;
- Plan Preview shows the exact request, resolved provider/backend and request-specific blockers before queueing;
- `Settings > Models & runtime` remains lifecycle-only and never rewrites Analysis preferences.

The companion wireframes under `docs/design/ui/analysis-settings/` are coding references, not a replacement for existing visual-system rules.

---

# 19. Execution policy

Normal Studio release analysis uses:

```text
RuntimePolicy::Production
```

Benchmark policy is reserved for explicit diagnostics/development surfaces and must never be selected implicitly because production resolution failed.

No automatic production → benchmark fallback.

---

# 20. Requested-artifact compilation

Studio should request only outputs the current user action actually needs.

Examples:

## 20.1 Full candidate chart analysis

Typical request:

```text
vocal_chart      = true
pitch_evidence   = true
singing_analysis = true
transcript       = true
alignment        = true
stems            = as required by product/export intent
```

Do not automatically request Instrumental unless the user workflow/export path needs it.

## 20.2 Transcript only

```text
transcript = true
other outputs false
```

GAME must not block this request.

## 20.3 Alignment only with canonical lyrics

```text
lyrics.mode = canonical
alignment = true
transcript = false
```

Qwen ASR must not become a requirement merely because the old Studio profile contains an ASR engine field.

## 20.4 Pitch evidence only

```text
pitch_evidence = true
```

GAME must not block continuous F0.

## 20.5 Stem only

Request only the semantic stem roles needed.

Lead-vocal and instrumental branches remain independent.

---

# 21. Planning authority

Studio currently has a pure `analysis_plan.rs` planner that models UI DAG state.

After integration:

- Studio's graph remains a **product/presentation graph**;
- Engine `plan()` becomes authoritative for Engine execution nodes, capabilities and resource requirements;
- Runtime Manager status inside Engine plan is authoritative for resource usability;
- `AnalysisRequest.model_availability` must not remain an independent execution truth source.

Do not maintain two planners that can disagree about whether a model-backed Engine node can run.

---

# 22. Studio plan projection

Add a projection layer:

```text
AnalyzeRequestV1
     ↓
EnginePlan
     ↓
StudioPlanProjection
     ↓
Analysis graph UI / preview / blockers
```

The projection maps Engine execution nodes/capabilities onto the existing Studio graph where practical.

Initial recommended mapping:

| Studio presentation node | Engine node/capability |
| --- | --- |
| `preflight` | Studio TrueSource resolution + Engine `audio.decode` |
| `stems.vocals` | `extract-vocals` / `audio.extract_vocals` |
| `stems.instrumental` | `extract-instrumental` / `audio.extract_instrumental` |
| `vocals.denoise` | optional `audio.denoise` |
| `vocals.dereverb` | optional `audio.dereverb` |
| `lyrics.transcribe` | `transcript-evidence` + `transcript` |
| `lyrics.align` | `alignment-evidence` + `alignment` |
| `pitch.extract` | `pitch` / `pitch.track` |
| `chart.build_candidate` | GAME + singing fusion + candidate graph + vocal-chart finalize |

`stems.bind_analysis_outputs` remains a Studio compatibility/presentation alias and must not be presented as a separate native model execution step if the Engine has already selected the semantic analysis source.

---

# 23. Studio-owned graph nodes outside Engine v1

Not every current Studio graph node belongs to Engine v1.

Examples include existing key/rhythm/descriptors and some custom Processing Studio branches.

These remain Studio-owned/local features until a versioned Engine capability exists for them.

Do not force unrelated nodes into the singing Engine merely to make one graph appear uniform.

The Analysis page may compose:

```text
Studio-owned local nodes
+
Engine-backed analysis nodes
+
Artifact/export nodes
```

while keeping execution ownership explicit.

---

# 24. Preview must execute exactly what was confirmed

The current Studio product supports impact preview and confirmation.

Preserve that invariant with the new Engine.

Required flow:

```text
compile exact AnalyzeRequestV1
        ↓
validate
        ↓
Engine plan
        ↓
show preview/blockers
        ↓
user confirms
        ↓
queue the exact serialized request already previewed
```

Do not reconstruct a new request from current settings after confirmation.

If the source or required immutable inputs change before execution, fail preflight and require a new preview.

---

# 25. Queue persistence

Studio continues to own the queue.

The queue must evolve from only storing `file_hash` status to preserving the exact Engine request identity for Engine-backed runs.

Recommended migration fields:

```text
request_id
engine_request_json
engine_plan_json
queued_at_ms
```

These may be columns on `analysis_queue` or a normalized run-intent table referenced by it.

The important invariant is not the exact SQL shape. The invariant is:

> A confirmed request is durable and cannot be silently reconstructed differently at execution time.

---

# 26. Analysis history persistence

`analysis_history` remains the durable run record.

Add optional Engine-specific provenance fields rather than creating a competing second run table.

Recommended fields:

```text
request_id
engine_version
engine_request_json
engine_plan_json
engine_result_json
engine_fingerprint
```

Historical pre-Engine rows remain readable with these values absent.

Cancelled runs keep the existing dedicated cancelled semantics.

---

# 27. CLI process isolation

Studio executes analysis through:

```text
uta-analyze worker --stdio-json
```

and resource lifecycle/status through:

```text
uta-runtime <command> ... --output ndjson
```

not through the old loose `kind/audio_path/cache_path` command and not through direct Rust crate calls.

Request construction is Studio-owned serialization. Validation, requirements and planning are obtained by sending that serialized request to `uta-analyze`.

Runtime status/install/repair/remove facts are obtained from `uta-runtime` machine output.

There is no final `app-core -> uta-analysis-engine` or `app-core -> uta-runtime-manager` Cargo dependency.

---

# 28. Canonical CLI executable discovery

Studio has two packaged backend executable identities independent from the legacy wrapper.

Preferred development/package overrides:

```text
UTA_STUDIO_ANALYSIS_CLI_PATH
UTA_STUDIO_RUNTIME_CLI_PATH
```

They point at the exact packaged:

```text
uta-analyze
uta-runtime
```

Do not keep `UTA_STUDIO_NATIVE_ANALYZER_PATH` as the final Engine identity.

During migration it may remain only for the compatibility wrapper.

Studio never discovers individual model/checkpoint paths itself.

---

# 29. Engine worker handshake

The Studio supervisor must validate at least:

```text
type == ready
protocol == supported protocol version
protocol_identity == uta.analysis-engine.worker
component == uta-analysis-engine
required request/result contract versions are supported
```

The old `runtime_recipe_digest` handshake comparison belongs to the compatibility worker and must not be copied into the new Engine protocol as a substitute for per-resource provenance.

Resource/runtime recipe identities belong in Runtime Manager resolution and the Engine result fingerprint/provenance.

---

# 30. Studio CLI clients

Create focused process clients in `app-core`.

## AnalysisCliClient

Responsibilities:

- start `uta-analyze worker --stdio-json`;
- validate handshake;
- keep stdout NDJSON-only;
- drain stderr into bounded logs;
- send versioned commands;
- enforce one active request per worker unless protocol explicitly changes;
- match request IDs;
- translate process exit/protocol errors into typed Studio failures;
- restart only after a fault/normal lifecycle boundary;
- support protocol cancellation once the standalone Engine cancellation gate passes.

## RuntimeCliClient

Responsibilities:

- launch `uta-runtime` directly with argv, never through a shell;
- request JSON/NDJSON machine output;
- parse typed result/error frames;
- expose read-only and explicit-confirmation mutation operations to app-core;
- never infer Runtime Manager state from local folders/manifests.

Neither client contains Engine planner logic or Runtime Manager policy logic.

---

# 31. Cancellation prerequisite

The current Engine worker executes `analyze` synchronously and cannot yet truthfully service a concurrent `cancel` command while analysis is running.

Deep Studio execution integration must not claim mid-run cancellation until the standalone Engine implements and tests real cancellation.

Acceptable final shape:

```text
worker command loop
  ├─ receives analyze
  ├─ runs execution in controlled worker context
  └─ remains able to receive cancel for active request_id
```

Studio may retain queued-item cancellation now.

Do not relabel killing an unrelated shared process as fine-grained cancellation.

---

# 32. Engine event model

Studio should consume typed Engine lifecycle events, not infer model progress from strings.

The target event vocabulary from the Engine guide includes:

```text
ready
validation_result
requirements
plan
analysis_started
node_started
node_progress
node_completed
artifact
warning
degraded
error
done
cancelled
```

Every run/node event that refers to a request must carry the exact `request_id`.

Node events should expose stable Engine node ID and capability ID.

Do not make model filenames the event identity.

---

# 33. Event-to-Studio mapping

Existing Studio types remain useful:

```text
AnalysisProgressSnapshot
AnalysisStageRoute
analysis_node_attempts
```

Extend them with optional Engine fields where needed:

```text
engine_node_id
capability_id
request_id
```

Keep the existing Studio `node_id` as the presentation-graph ID when one Engine node maps into an existing Studio node.

When several Engine nodes map to one Studio presentation node, preserve raw Engine node identity separately rather than collapsing provenance.

Old history rows without new fields must continue to deserialize.

---

# 34. Progress semantics

The global progress rail and node progress must not be reconstructed from model stage order.

Use Engine-provided:

```text
node lifecycle
work units when known
explicit overall progress when provided
```

If overall progress is unavailable, Studio may display indeterminate global progress while still displaying exact node state.

Do not invent percentages to make the UI look complete.

---

# 35. Run output directory

For every Engine run, Studio creates a unique authorized output root.

Required properties:

- below Studio-generated/cache data;
- never inside the source media directory;
- never the shared Artifact Store destination itself;
- unique per request;
- no overwrite of another run;
- cleanup-safe after successful artifact capture;
- cleanup-safe after failed/cancelled run;
- crash leftovers recognizable by run ownership.

Example:

```text
cache/engine-runs/<request_id>/
```

---

# 36. Result manifest handoff

A successful Engine run is not considered committed merely because the child exits successfully.

Studio requires one valid `AnalysisResultManifestV1` matching the request.

The worker's successful terminal frame must unambiguously identify the manifest, for example by a confined relative manifest path or an equivalent typed result payload.

Studio then validates the result contract before any Artifact DB mutation.

---

# 37. Result validation

Before capture, Studio verifies:

```text
result contract/version
request_id exact match
status semantics
fingerprint syntax
required algorithm provenance
artifact relative paths
artifact path confinement after canonicalization
no symlink escape
artifact file exists
byte size matches
SHA-256 matches
media type is expected for semantic type
no duplicate semantic role where prohibited
```

Engine's own validation is necessary but not sufficient across a process trust boundary. Studio revalidates what it is about to commit.

---

# 38. Engine artifact → Studio artifact mapping

Initial semantic mapping:

| Engine result semantic | Studio `ArtifactKind` |
| --- | --- |
| `candidate_vocal_chart` | `CandidateChart` |
| `pitch_evidence` | `PitchEvidence` |
| `singing_analysis` | `EvidenceBundle` initially, or a dedicated future `SingingAnalysis` kind |
| `transcript` | `TranscriptEvidence` |
| `alignment` | `AlignmentEvidence` |
| `stem:guide_vocals` | `VocalStem` / exact tested vocal-stage kind |
| `stem:lead_vocal` | `AnalysisVocalStem` |
| `stem:instrumental` | `InstrumentalStem` |

If the existing Studio artifact taxonomy cannot represent an Engine semantic without losing important meaning, add a new `ArtifactKind` rather than mislabeling it.

Do not label lossy bytes as FLAC.

---

# 39. Preserve both artifact hashes

Existing Studio `ArtifactRevision.content_hash` uses the Studio BLAKE3-derived convention.

Engine `ArtifactRefV1.sha256` is full SHA-256.

Preserve both.

Do not replace existing revision IDs with Engine SHA-256 during this migration.

Add explicit Engine artifact metadata, either as nullable artifact columns or a normalized metadata table, including at minimum:

```text
revision_id
engine_sha256
media_type
semantic_type
request_id
engine_fingerprint
```

---

# 40. Preserve Engine run provenance

Do not overload existing `config_hash` or `algorithm_version` to hold the entire Engine result manifest.

Persist exact Engine provenance separately.

At minimum, retain:

```text
engine version
request JSON
plan JSON
result manifest JSON
execution fingerprint
resolved resource generations/content digests/runtime generations
algorithm component versions
```

This enables later audit and reproducibility without changing current ArtifactRevision identity semantics.

---

# 41. Atomic Studio artifact commit

Engine output publication and Studio Active Revision publication are two separate transactions.

Required Studio commit sequence:

```text
1. validate complete Engine result
2. verify all required files
3. capture every result file into immutable Artifact Store
4. prepare all ArtifactRevision rows/bindings
5. in one DB transaction:
      insert/update revisions
      record run/node artifact bindings
      update Engine provenance metadata
      switch Active Candidate/evidence revisions as appropriate
      finalize history status
6. only then remove run-temporary output
```

If any step before the database transaction fails:

- do not switch Active revisions;
- do not overwrite Authored chart;
- mark run failed;
- immutable orphan copies may be retained for later safe GC but must not become Active.

---

# 42. Candidate chart safety

An Engine Candidate chart must never overwrite the Authored chart.

On successful commit:

```text
new CandidateChart revision → may become Active Candidate
existing AuthoredChart revision → unchanged
```

The editor may then:

- inspect Candidate;
- compare with Authored;
- merge/rebase through existing authoring commands;
- explicitly save a new Authored revision.

Re-analysis must not destroy human corrections.

---

# 43. PitchEvidence remains separate

Do not turn continuous pitch into note targets in Studio.

Engine returns typed PitchEvidence separately from Candidate VocalChart.

Studio stores and exposes them as separate revisions.

The editor may visualize continuous pitch but must not treat it as the authored note track.

---

# 44. Runtime readiness redesign

Current `AnalysisRuntimeStatus.ready` effectively models one hard-coded baseline set.

That is too coarse once Engine requests are artifact-specific.

Final Studio readiness should have two layers.

## 44.1 Global component health

Examples:

```text
Analysis Engine executable available
worker protocol compatible
Runtime Manager catalog readable
FFmpeg resource usable
runtime store healthy
```

## 44.2 Request-specific readiness

Derived from:

```text
AnalyzeRequestV1
→ Engine requirements/plan
→ Runtime Manager status
```

A full chart may be blocked by GAME while a transcript-only or pitch-only request is ready.

Do not disable every Analysis action because one unrelated model is unavailable.

---

# 45. Models & runtime page

The page remains the explicit resource lifecycle UX. Its relationship to Analysis preferences and Plan Preview is further specified in `docs/design/integration/UTA_STUDIO_ANALYSIS_SETTINGS_MODEL_SELECTION_EXECUTION_UX_DESIGN_v1.0.md`.

It must never rewrite a stored Analysis preference merely because a resource is installed, removed, repaired or becomes unusable.

It should display Runtime Manager data, including:

```text
resource display name
purpose
origin/install state
integrity
validation state
usable under production policy
selected backend when resolved
reasons when unusable
license/source attribution
```

Install/repair/remove buttons call Runtime Manager library operations through app-core.

The desktop does not shell out to `uta-runtime`.

---

# 46. Explicit installation flow

When Engine plan reports missing required resources:

Studio may offer:

```text
Open Models & runtime
Review required resources
Install…
```

Studio must not:

- auto-download on Analyze;
- auto-install on page render;
- auto-install from `doctor`;
- switch to benchmark policy;
- select a substitute model silently.

Every network acquisition remains an explicit user action with confirmation.

---

# 47. Runtime mutation ownership

Existing Studio APIs such as:

```text
install_audio_model
reinstall_audio_model
remove_audio_model
trigger_setup
```

must progressively become thin UX adapters over Runtime Manager mutation APIs.

No Studio module may implement a second download/checksum/publish pipeline.

Studio-specific data-root relocation remains outside Runtime Manager.

---

# 48. Analysis page behavior

The Analysis settings and run UX must follow `docs/design/integration/UTA_STUDIO_ANALYSIS_SETTINGS_MODEL_SELECTION_EXECUTION_UX_DESIGN_v1.0.md`, including quality profile, provider-preference, inheritance and Plan Preview rules.

The Analysis page should render the Studio presentation graph from:

```text
product graph
+ exact Engine plan projection
+ current Engine events
+ Artifact Inventory
+ history/node attempts
```

It must stop inferring model readiness from cache folders or legacy booleans.

Blocked node details should state the actual capability/resource reason returned by Engine/Runtime Manager.

---

# 49. GAME blocked behavior

While GAME is not production-usable:

A request that needs note evidence / Candidate VocalChart remains blocked/fail-closed.

However:

```text
stem-only
transcript-only
alignment-only where representable
pitch-evidence-only
```

must be evaluated from their own Engine requirements and must not inherit a synthetic GAME dependency.

The UI should explain:

```text
Candidate chart unavailable: notes.game is not production-usable
```

rather than claiming the entire native runtime is missing.

---

# 50. Existing Studio Freeze / Disable / Bypass semantics

The current Studio graph supports richer per-node reuse controls than `AnalyzeRequestV1` can currently represent.

Do not fake equivalence.

## 50.1 Disable

A Studio node may only be disabled on an Engine-backed run if the resulting requested artifact set can be compiled into a valid Engine request without that capability.

Otherwise the action is unavailable for that run.

## 50.2 Freeze

Freezing an existing arbitrary pitch/transcript/chart artifact is not representable in Engine v1 as a generic immutable artifact input.

Do not claim it is reused when Engine actually recomputes it.

## 50.3 Bypass stem separation with Original Mix

The old Studio bypass means “treat Original Mix as the downstream analysis signal without separation.”

Engine v1 interprets an `original_mix` primary according to its own semantic routing and will separate when required.

Therefore this bypass is not currently equivalent and must not be implemented by falsely labeling Original Mix as `lead_vocal` or `clean_lead_vocal`.

## 50.4 Migration rule

For the first Engine-backed integration:

- support actions that compile truthfully to Engine v1;
- disable unsupported granular controls with an explicit explanation;
- do not silently fall back to the legacy analyzer;
- restore full reuse controls only through a future versioned Engine input-artifact/routing contract.

---

# 51. Processing Studio workflow migration

Processing Studio may keep persisted product workflow definitions.

Compile them to Engine **capability/artifact intent**, not concrete model identity.

Allowed stable product concepts include:

```text
need transcript
need alignment
need continuous pitch
need Candidate chart
need instrumental
quality profile
known lyrics vs generated lyrics
```

Do not make Engine understand the entire Studio workflow schema.

Do not make a persisted Studio model filename override Runtime Manager policy.

---

# 52. Compatibility `AudioProcessingPlanSnapshot`

Keep it temporarily for current UI/history compatibility.

It must stop being the canonical Engine public contract.

The final Engine request expresses:

```text
semantic source role
requested artifacts
profile
constraints
```

not an exact list of model filenames and private worker parameters.

---

# 53. Error taxonomy at the Studio boundary

Preserve Engine error codes and add Studio context without flattening everything into one string.

Recommended Studio categories:

```text
source_missing
source_identity_changed
request_invalid
capability_missing
resource_unavailable
runtime_policy_rejected
worker_protocol_error
worker_crashed
inference_failed
output_invalid
artifact_commit_failed
cancelled
```

UI copy may be friendly, but logs/history retain the original Engine error code/resource/capability/request ID.

---

# 54. Restart and crash behavior

If Studio or the Engine process crashes:

- TrueSource remains untouched;
- existing Active revisions remain untouched;
- Authored chart remains untouched;
- incomplete run output remains non-active;
- a persisted `analyzing` queue row is reconciled as interrupted/failed on restart unless a tested resume protocol exists;
- temporary run roots may be cleaned only after ownership is verified;
- Runtime Manager generations remain protected by leases while the process is alive.

Do not infer success from output files left behind by a crashed process.

---

# 55. Logging

Maintain one durable Studio run log per analysis history row.

Include:

- Studio request ID;
- Engine version/handshake;
- Engine stderr/log lines;
- structured lifecycle events;
- exact terminal error;
- artifact validation/commit summary;
- Runtime Manager resource identities from the result manifest.

Do not place copyrighted source media bytes in logs.

---

# 56. API capabilities

Keep existing Studio command names where practical.

The app API registry must reflect the new boundary.

At minimum verify/update capabilities for:

```text
analysis_runtime_status       read
preview_analysis_plan         read
run_analysis_request          mutation
cancel_analysis_run           mutation
analysis history reads        read
artifact inspection           read
artifact commit/edit actions  mutation
model/runtime status          read
model/runtime install         external
model/runtime repair/remove   mutation/external as appropriate
```

If new public commands are added for Engine capability/plan inspection, register them and classify them correctly.

`run_feature_diagnostics` remains non-destructive and must not install resources or alter real songs.

---

# 57. Security and path confinement

The Engine process is local but still treated as a separate execution boundary.

Studio must protect:

```text
TrueSource read-only path
run output root
Artifact Store root
Runtime Manager store
```

Never trust a result-relative path without canonical confinement verification.

Never let an Engine artifact path target:

- source directories;
- arbitrary user paths;
- model/runtime directories;
- existing Authored chart paths outside the authorized run root.

---

# 58. Source-media permissions

Studio must not require write permission to TrueSource for analysis.

Tests must include a read-only source file when the platform supports it.

Generated outputs belong under Studio-generated storage.

---

# 59. Packaging

The final package must ship:

```text
uta-studio
uta-analyze
uta-runtime
required packaged native worker executables
```

A `uta-analysis-engine` alias/component binary may remain packaged if independently useful, but Studio's canonical process boundary is `uta-analyze` + `uta-runtime`.

The wrapped Studio application receives exact packaged CLI paths and does not depend on PATH discovery.

Linux package remains Wayland-only.

During the compatibility period `uta-native-analyzer` may remain packaged, but it is removed after the new Studio worker path and standalone Engine gates pass.

---

# 60. Development overrides

Development executable overrides must remain explicit.

Preferred Studio backend CLI overrides:

```text
UTA_STUDIO_ANALYSIS_CLI_PATH
UTA_STUDIO_RUNTIME_CLI_PATH
```

Model/runtime file overrides remain Runtime Manager responsibilities behind `uta-runtime`/Analysis Engine and are not Studio model-selection inputs.

Do not add direct Studio environment variables for individual model files once Runtime Manager owns them.

---

# 61. Legacy analyzer retirement criteria

Do not remove `uta-native-analyzer` compatibility support until all are true:

```text
[ ] Studio preview compiles exact AnalyzeRequestV1
[ ] Studio uses `uta-analyze worker --stdio-json` directly
[ ] Studio runtime/model lifecycle calls use `uta-runtime --output json/ndjson`
[ ] app-core has no `uta-analysis-engine` or `uta-runtime-manager` Cargo dependency
[ ] app-core/desktop contain no `uta_analysis_engine::` or `uta_runtime_manager::` imports
[ ] source SHA-256 validation works end to end
[ ] request-specific Runtime Manager readiness works
[ ] real Engine artifacts commit to Artifact Store
[ ] Candidate/Authored preservation tests pass
[ ] history + node attempts persist Engine runs
[ ] cancellation works
[ ] crash/output-validation tests pass
[ ] packaged Linux Studio smoke uses the new Engine path
[ ] no production call site still depends on old loose protocol
```

After those pass, remove the wrapper from Studio routing first, then from packaging/source in a separate cleanup change.

---

# 62. Schema migration principles

Database changes must be additive/backward-readable.

Existing users may have:

- old analysis history;
- old ArtifactRevision rows;
- active Authored charts;
- legacy cached outputs.

Do not rewrite all historical rows merely to add Engine provenance.

New fields are nullable/defaulted for old data.

No migration touches source media.

---

# 63. Test strategy

## 63.1 Request compiler unit tests

Cover:

```text
local TrueSource
missing TrueSource
empty file
source changed since library identity
real SHA-256 distinct from Studio file_hash
GeneratedLyrics mapping
KnownLyrics mapping
reference lyrics mapping
Fast/Balanced/Maximum
artifact-specific requests
production policy forced
```

## 63.2 Plan/readiness tests

Cover:

```text
full chart blocked by GAME when unavailable
transcript-only does not require GAME
pitch-only does not require GAME
instrumental-only does not require lead isolation
lead analysis does not require instrumental unless requested
canonical lyrics alignment does not require ASR
candidate-only resources rejected in production
```

## 63.3 Analysis CLI client tests

Cover:

```text
valid ready handshake
wrong protocol
wrong component
missing contract version
stdout pollution
stderr capture
request_id mismatch
child exit
bounded frames
cancellation
restart after failure
```

## 63.4 Result-validation tests

Cover:

```text
valid confined artifact
../ escape
absolute path
symlink escape
wrong bytes
wrong SHA-256
wrong byte count
wrong media type
wrong request_id
malformed fingerprint
duplicate stem role
successful status with incomplete provenance
```

## 63.5 Atomic commit tests

Cover:

```text
all artifacts commit
one bad artifact => no Active switches
candidate becomes Active Candidate
Authored chart unchanged
same bytes deduplicate safely
Engine SHA-256 metadata preserved
Studio BLAKE3 revision identity preserved
history result/fingerprint persisted
```

## 63.6 UI tests

Cover:

```text
request-specific blockers
GAME blocks Candidate but not pitch/transcript actions
Models & runtime shows manager truth
no automatic download
unsupported Freeze/Bypass actions are explicitly unavailable
Candidate review does not overwrite Authored chart
```

## 63.7 Packaging/smoke

Cover:

```text
nix build path:.#uta-studio
wrapped uta-studio resolves packaged uta-analyze and uta-runtime
Wayland launch
uta-analyze worker handshake from package
uta-runtime JSON/NDJSON read command from package
real local WAV/FLAC source request
real Engine output commit once Engine standalone gate is satisfied
project-name scan
zero-script-runtime gate
source-size gate
```

---

# 64. Integration gate

The Analysis Engine guide's standalone reintegration gate remains authoritative.

Deep execution switch-over is allowed only after standalone Engine passes at least:

```text
request validation
requirements
runtime resolution
real separation baseline
real Qwen ASR/alignment
real RMVPE
real GAME
fusion
Candidate artifacts
real-song tests
cancellation
fingerprint
```

Before that gate, Studio work may safely implement and test:

- TrueSource resolver;
- request compiler;
- request-specific preview/readiness;
- Runtime CLI process delegation;
- schema additions;
- result validation/commit code against fixtures;
- worker-supervisor protocol tests against fixtures.

But normal Studio full analysis must remain fail-closed.

---

# 65. Initial Engine-backed feature scope

The first real Studio execution milestone should be intentionally narrow:

```text
local TrueSource
Production policy
full or artifact-specific Engine request
one active Engine request at a time
run-scoped output directory
validated result manifest
immutable Artifact Store commit
Candidate review
```

Do not combine this milestone with restoration of every historical per-node reuse control.

---

# 66. Follow-up contract work for full granular Studio reuse

After the first end-to-end Engine-backed Studio flow is stable, restore richer Studio semantics through a new **versioned** Engine contract rather than mutating v1 meaning.

Likely requirements include typed immutable artifact inputs such as:

```text
precomputed transcript/alignment
precomputed pitch evidence
frozen semantic audio stem
explicit route/substitution intent
artifact content identity/provenance
```

Any future `AnalyzeRequestV2` design must preserve:

- local-file confinement;
- exact hashes;
- semantic roles;
- immutable provenance;
- fail-closed behavior.

Do not add ad-hoc v1 extension fields that silently change planner semantics.

---

# 67. Expected final Studio flow

```text
User selects Analyze
        |
        v
Studio resolves local TrueSource
        |
        +-- verifies library identity
        +-- computes SHA-256
        |
        v
Studio compiles exact AnalyzeRequestV1 JSON
        |
        v
AnalysisCliClient -> uta-analyze validate / requirements / plan
        |
        +-- Engine resolves Runtime Manager internally
        |
        +-- RuntimeCliClient -> uta-runtime for lifecycle/status presentation
        |
        v
Studio presents exact preview
        |
      confirm
        |
        v
Studio queue persists exact request
        |
        v
AnalysisCliClient -> uta-analyze worker --stdio-json
        |
        v
native Analysis Engine execution
        |
        v
AnalysisResultManifestV1
        |
        v
Studio validates every artifact
        |
        v
immutable Artifact Store capture
        |
        v
atomic DB publication
        |
        +-- Candidate revisions active
        +-- Authored revision preserved
        +-- history/provenance recorded
        |
        v
Review / Editor / Export
```

---

# 68. Final invariants

The finished integration must satisfy all of these statements.

1. **TrueSource is always a local file and is read-only.**
2. **Studio `file_hash` is not Engine SHA-256.**
3. **Preview and execution use the exact same serialized Engine request.**
4. **Engine decides what capabilities/resources the analysis needs.**
5. **Runtime Manager decides what resources are usable.**
6. **Studio never overrides production resource policy.**
7. **Studio remains the owner of queue/history/artifact/editor state.**
8. **Engine writes only into a run-scoped authorized output directory.**
9. **Studio validates outputs before publication.**
10. **A failed/cancelled run cannot replace Active artifacts.**
11. **Candidate re-analysis cannot overwrite Authored work.**
12. **Continuous PitchEvidence remains separate from target notes.**
13. **Missing GAME blocks note/Candidate requests, not unrelated partial analysis.**
14. **No read path downloads models.**
15. **No HTTP or Python production fallback exists.**
16. **The legacy native-analyzer wrapper disappears only after the new path is proven.**
17. **Studio links neither `uta-analysis-engine` nor `uta-runtime-manager`.**
18. **Studio obtains Analysis truth only from the `uta-analyze` machine protocol.**
19. **Studio obtains Runtime Manager lifecycle truth only from the `uta-runtime` machine protocol.**
20. **Contract tests, not shared Rust types, protect the Studio/backend boundary.**

> **Studio compiles intent and preserves user work. `uta-analyze` performs analysis. `uta-runtime` exposes resource truth/lifecycle. The boundaries are versioned process protocols.**
