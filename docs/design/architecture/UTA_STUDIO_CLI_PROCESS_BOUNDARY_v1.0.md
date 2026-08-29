# Uta! Studio — CLI Process Boundary Contract v1.0

**Status:** mandatory architecture contract
**Date:** 2026-08-22
**Applies to:** Uta! Studio integration with `uta-analyze` and `uta-runtime`

---

# 1. Purpose

Uta! Studio integrates Analysis Engine and Runtime Manager through **process boundaries**, not Rust crate dependencies.

The final architecture is:

```text
                         packaged local processes

Uta! Studio / app-core
        |
        | JSON / NDJSON only
        |
        +--------------------------+
        |                          |
        v                          v
uta-analyze                  uta-runtime
Analysis CLI                 Runtime Manager CLI
        |                          |
        |                          |
        v                          v
Analysis Engine              Runtime Manager
native workers               resource store / policy
```

The central rule is:

> **Studio may launch the two packaged CLIs and consume their versioned machine protocols. Studio must not link either implementation crate.**

This is a stronger boundary than ordinary module separation. It is intended to keep Studio, Analysis Engine, and Runtime Manager independently buildable and replaceable.

---

# 2. Compile-time dependency rule

Final Studio code must not have Cargo dependencies on:

```text
uta-analysis-engine
uta-runtime-manager
```

Therefore final `app-core/Cargo.toml` and `desktop/Cargo.toml` must not contain path/package dependencies on either crate.

Final Studio source must not import implementation namespaces:

```text
uta_analysis_engine::
uta_runtime_manager::
```

A final verification scan must return empty outside tests/fixtures explicitly marked as protocol-source snapshots:

```text
app-core/**
desktop/**
```

The Engine may internally use Runtime Manager as a Rust library because both are backend components. That does not grant Studio access to the library.

---

# 3. Why Studio owns local wire DTOs

A process boundary requires the consumer to own its representation of the external protocol.

Studio therefore defines small local wire DTOs under an app-core protocol/client module, for example:

```text
AnalysisWorkerReadyV1
AnalysisValidateFrameV1
AnalysisRequirementsFrameV1
AnalysisPlanFrameV1
AnalysisErrorFrameV1
AnalysisResultManifestWireV1
RuntimeCliResultV1<T>
RuntimeCliErrorV1
RuntimeResourceStatusWireV1
```

These are **wire DTOs**, not duplicated business logic.

Studio must not copy Engine planner logic or Runtime Manager policy logic. It only serializes commands and deserializes returned facts.

Contract drift is detected by process-level contract tests against the real packaged/debug CLI, not prevented by sharing Rust types.

---

# 4. Analysis process boundary

The canonical Studio-facing Analysis executable is:

```text
uta-analyze
```

Studio uses the persistent worker mode:

```text
uta-analyze worker --stdio-json
```

Wire rules:

```text
stdin   NDJSON commands
stdout  NDJSON machine frames only
stderr  human/debug logs only
```

The worker must emit a `ready` frame before accepting work.

At minimum Studio verifies:

```text
protocol version
protocol identity
component identity
engine version
supported request/result contract versions
```

Studio must fail closed on protocol/contract incompatibility.

---

# 5. Analysis commands used by Studio

Studio may send the versioned worker commands defined by Analysis Engine, including:

```text
hello
capabilities
validate
requirements
plan
analyze
cancel
quit
```

The canonical request payload is serialized `AnalyzeRequestV1` JSON.

Studio does not call Engine library functions such as:

```text
Planner::plan
AnalysisEngine::analyze
AnalysisEngine::capabilities
```

Those names are backend implementation details.

An `analyze` command returns `analysis_started`, followed by zero or more typed lifecycle frames and one correlated terminal frame. Implemented lifecycle types are `node_started`, measured `node_progress`, `node_completed`, `node_failed`, `artifact`, `warning`, and `degraded`. Every lifecycle frame carries the request ID, raw Engine node ID, capability ID, implementation and Engine timestamp; model ID and Processing Studio presentation-node ID are present when applicable. Studio must reject malformed/cross-request frames and must not turn node order or human stderr into a percentage.

---

# 6. Exact preview/execution rule

Studio compiles the request locally as JSON according to the public request contract, then sends that exact payload to the Analysis CLI.

Required flow:

```text
Studio product intent
    ↓
serialized AnalyzeRequestV1 JSON
    ↓
validate command
    ↓
plan command
    ↓
Plan Preview
    ↓
user confirms
    ↓
analyze command with the exact same serialized request
```

Do not reconstruct the request from current settings after confirmation.

The Studio-owned request snapshot may be persisted as JSON bytes/text plus Studio metadata.

---

# 7. Analysis result boundary

Analysis Engine writes only beneath the Studio-authorized run output directory.

The worker returns machine events and a result manifest/reference according to the Analysis protocol.

Studio validates returned data independently before publication:

```text
request_id
contract/version
relative confined paths
file existence
byte count
media type
status
fingerprint
provenance shape
```

Only after validation may Studio capture output into its immutable Artifact Store.

Studio never trusts a Rust object returned in-process because there is no in-process Engine call in the final architecture.

---

# 8. Analysis cancellation

Cancellation is a protocol operation:

```text
{"type":"cancel", ...}
```

Studio does not reach into Engine worker internals or child worker handles.

If the Analysis CLI cannot cancel a running request truthfully, the UI must not claim it can.

The Analysis CLI owns killing/reaping its native child workers.

---

# 9. Runtime Manager process boundary

The canonical Studio-facing Runtime Manager executable is:

```text
uta-runtime
```

Studio launches it as a local child process and requests machine output.

For one-shot operations use:

```text
uta-runtime <command> ... --output ndjson
```

or `--output json` where a single JSON document is explicitly more suitable.

Studio must not invoke Runtime Manager library functions directly.

---

# 10. Runtime commands used by Studio

Read-only operations may include:

```text
list
show
status
paths
plan
verify
doctor
smoke
resolve
```

Explicit user-confirmed mutation operations may include:

```text
setup
install
import
repair
reinstall
remove
```

The CLI remains the only Studio-facing lifecycle boundary.

Studio UI actions become adapters such as:

```text
list_audio_models
    -> spawn uta-runtime list/status --output ndjson

analysis_runtime_status
    -> query Analysis CLI requirements/plan
       + query uta-runtime status where needed for lifecycle presentation

install_audio_model
    -> spawn uta-runtime install ... --yes --output ndjson

repair_audio_model
    -> spawn uta-runtime repair ... --yes --output ndjson

remove_audio_model
    -> spawn uta-runtime remove ... --yes --output ndjson
```

Studio must not implement acquisition, hashing, generation publication, repair, or policy decisions itself.

---

# 11. Runtime CLI machine protocol

For NDJSON mode every stdout line is a machine frame.

Current/final result framing must remain versionable and typed. At minimum Studio must be able to distinguish:

```text
result
error
```

Long-running mutations may additionally emit typed progress frames when implemented:

```text
started
progress
artifact/download progress
verification
completed
error
```

Until progress frames exist, Studio may show an indeterminate operation state. It must not parse human stderr text to infer lifecycle state.

`stderr` is logs only.

---

# 12. Runtime policy ownership

Runtime Manager owns:

```text
installed
integrity
runtime ready
validation state
production usable
selected backend
blocked reason
```

Studio only displays returned facts.

Studio must not recreate logic such as:

```text
folder exists => usable
manifest exists => usable
installed => production ready
candidate => fallback automatically
```

---

# 13. Executable discovery

Packaged Studio resolves executable paths from package-owned configuration/environment and validated executable discovery.

Recommended canonical Studio variables:

```text
UTA_STUDIO_ANALYSIS_CLI_PATH
UTA_STUDIO_RUNTIME_CLI_PATH
```

Packaged Linux wrapper sets exact packaged paths.

Development may discover workspace binaries where explicitly supported.

Do not expose individual model files to Studio through new environment variables.

External analysis tools that are not Studio-facing CLIs remain Runtime Manager resources. In particular, AI-judgment fusion uses `tool:fusion_agent_adapter`: Studio must not put the adapter executable path into `AnalyzeRequest` or workflow DTOs. Runtime Manager owns configure/status/resolve for that path; Engine consumes the resolved tool only after normal request validation.

---

# 14. Process security rules

Studio must launch executables directly with `std::process::Command` or equivalent native process API.

Do not execute a shell command string.

Do not interpolate model/resource/user paths into shell scripts.

All arguments are separate argv values.

Source media remains read-only.

Runtime mutation confirmation remains explicit.

No HTTP control server is introduced.

AI-judgment adapters are direct child processes of Analysis Engine after Runtime Manager resolution. They may use the network according to the user's configured external provider, but the Uta protocol sends only bounded fusion candidate metadata in v1—not source audio bytes, arbitrary project files, the library DB, model files, or unrelated user content. A configured adapter is external software running with the user's OS permissions; Uta does not claim it is sandboxed.

---

# 15. Failure semantics

Studio distinguishes process/protocol errors from domain errors.

Process boundary categories include:

```text
executable_missing
spawn_failed
unexpected_exit
protocol_mismatch
contract_mismatch
stdout_pollution
malformed_frame
frame_too_large
request_id_mismatch
timeout/cancel failure
```

Domain error codes from Analysis/Runtime Manager remain preserved in logs/history and are mapped to localized UI copy.

Do not flatten every failure into `runtime unavailable`.

---

# 16. No shared implementation truth

The following are forbidden in final Studio code:

```text
RuntimeManager::...
Planner::...
AnalysisEngine::...
ResourceCatalog::...
ValidationState logic copied from Runtime Manager
Engine capability-to-model mapping copied into Studio
```

Studio may have presentation enums/DTOs for returned wire values, but their semantics are descriptive, not authoritative.

---

# 17. Contract tests replace crate coupling

Because Studio no longer gets compile-time type checking from backend crates, process-level contract tests are mandatory.

Analysis contract tests must launch the real `uta-analyze` binary and verify at least:

```text
ready handshake
validate request
requirements response
plan response
error response
bounded NDJSON
stdout purity
request_id correlation
contract version rejection
```

Runtime contract tests must launch the real `uta-runtime` binary and verify at least:

```text
list/status/plan JSON shape
read-only exit codes
NDJSON result/error framing
Production policy state
mutation requires explicit confirmation
unknown resource error
stdout purity
```

Fixtures may be used for UI tests, but final contract compatibility is proven against executable processes.

---

# 18. Packaging contract

The final Uta! Studio package must include or resolve:

```text
uta-studio
uta-analyze
uta-runtime
```

The wrapped Studio executable receives exact CLI paths.

The package smoke test must prove Studio can launch both CLIs from the packaged artifact.

The retired compatibility wrapper is not packaged or routed. `uta-analyze` is the only Studio-facing analysis process.

---

# 19. Completed process-boundary migration

Studio owns local wire DTOs and communicates through `AnalysisCliClient` and `RuntimeCliClient`. Direct backend crate imports and compatibility execution routes are forbidden and remain at zero.

---

# 20. Ownership consequence for parallel agents

The process boundary makes implementation ownership simple:

```text
Analysis PI
    writes analysis-engine/**

Runtime PI
    writes runtime-manager/**

Studio Integration PI
    writes app-core/** and packaging seam
    consumes only CLI protocols

Studio UX PI
    writes desktop/src/studio/** + i18n
    consumes only app-core Studio APIs
```

No backend PI needs to edit Studio source to integrate its feature.

No Studio PI needs to edit backend source to consume a feature.

---

# 21. Final invariant

A clean final dependency graph is:

```text
uta-analysis-engine  ---> uta-runtime-manager library (backend-internal, if needed)

uta-analyze          ---> uta-analysis-engine
uta-runtime          ---> uta-runtime-manager

uta-studio/app-core  -X-> uta-analysis-engine crate
uta-studio/app-core  -X-> uta-runtime-manager crate

uta-studio/app-core  ---> uta-analyze process protocol
uta-studio/app-core  ---> uta-runtime process protocol

desktop              ---> app-core only
```

> **Backend implementation coupling stops at the CLI boundary. Studio consumes contracts, not crates.**
