# 15 — Compiled Workflow Executor

**Precondition:** model cards 01–13 are terminal and no machine-level safety stop is active. Production-only model blockers (for example separate Vulkan authorization) do not block this CPU/control-plane feature card.
**Task class:** CPU/control-plane feature closure; no model inference required for implementation acceptance
**Primary owner:** `uta-analyze` / Analysis Engine execution contract

## Read

```text
AGENTS.md
docs/agent-tasks/MODEL_GPU_WORK_POLICY.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
tasks/final-features/15_COMPILED_WORKFLOW_EXECUTOR.md
docs/design/architecture/UTA_STUDIO_CLI_PROCESS_BOUNDARY_v1.0.md
docs/design/integration/UTA_STUDIO_ANALYSIS_RUNTIME_INTEGRATION_DESIGN_v1.0.md
```

Inspect only relevant current source:

```text
app-core/src/workflow/**
app-core/src/backend_cli/**
app-core/src/analysis_engine_adapter.rs
app-core/src/analyzer/engine_run.rs
analysis-engine/src/contract/**
analysis-engine/src/planner/**
analysis-engine/src/engine.rs
analysis-engine/src/bin/** or uta-analyze worker entrypoint
native-inference/native-analyzer/** only as legacy compatibility evidence
```

## Problem to close

Studio already supports a typed user Workflow and compiles an immutable `WorkflowExecutionSnapshot`, but the old compatibility coordinator still has a fail-closed path equivalent to:

```text
native workflow execution ... has no fully validated component set in this build
```

The canonical product path must not solve this by reviving a direct Studio -> native-analyzer implementation seam. New Processing Studio execution must use:

```text
app-core AnalysisCliClient
  -> uta-analyze process protocol
  -> Analysis Engine workflow executor
```

## Required architecture

The exact user workflow snapshot must cross the process boundary as a versioned machine contract without linking backend crates into Studio.

Prefer a versioned AnalyzeRequest extension when compatible with the existing v1 extension mechanism, e.g. a backend-owned semantic equivalent of:

```text
extensions["uta.workflow_execution.v1"] = <workflow execution wire snapshot>
```

The exact key/schema may be chosen by the implementation, but it must be explicit, versioned, validated, and independently represented by local Studio wire DTOs and backend DTOs. Do not add a shared implementation dependency to avoid serialization work.

The backend must validate at minimum:

```text
workflow schema/version
workflow id/revision
definition digest
node IDs are unique
node binding capabilities are known
artifact bindings reference valid nodes/ports
execution policy values are known
runtime/model selections are compatible with backend capabilities
requested terminal artifacts are actually reachable
no hidden cycle or invalid dependency is accepted
```

Do not blindly trust a Studio-compiled graph across the process trust boundary.

## Execution semantics

Implement a real Analysis Engine workflow executor that maps the accepted compiled workflow onto existing Engine capabilities and typed artifacts.

Required semantics:

```text
source/decode nodes
ordered audio transformations
analyzer attachment to the selected audio artifact, not "latest vocal"
transcript/alignment/pitch/note/acoustic expert nodes
fusion nodes
candidate graph
Candidate VocalChart finalization
requested semantic stem outputs
```

Duplicate transformation node instances must remain distinct by instance ID. Priority must affect scheduling order only; it must not create a dependency edge. Disabled nodes do not execute. Conditional nodes are represented faithfully but disagreement-window scheduling is completed by card 16.

For card 15, a conditional node may remain deferred/pending according to typed execution state rather than being treated as Always.

## Preview = execution

Plan Preview and execution must use the same serialized request + workflow snapshot identity:

```text
request_id
request digest
workflow id/revision
definition digest
compiled node/artifact bindings
resolved parameters
```

Do not recompile from mutable current UI state after queueing. Do not silently substitute a different Workflow on execution.

## Studio responsibilities

Studio may need to serialize its existing `WorkflowExecutionSnapshot` into the local `AnalyzeRequestWireV1.extensions` representation and validate backend Plan/result facts. This is allowed.

Studio must not:

```text
execute nodes itself
resolve Runtime Manager policy itself
call Analysis Engine library APIs
copy backend scheduler/fusion logic
launch uta-runtime from Desktop
```

If Studio source changes are required, keep them limited to local domain -> local wire translation, queue snapshot persistence, Plan Preview presentation, and result validation/publication.

## Legacy compatibility path

Do not make `native-inference/native-analyzer` the new canonical Workflow executor. Legacy compatibility code may remain fail-closed for historical callers if still required, or may delegate through the canonical CLI only if doing so does not create recursion. New Processing Studio runs must enter `uta-analyze` through `AnalysisCliClient`.

## Tests

Use CPU-only typed/fake fixtures. Do not execute real models in this card.

Required tests include:

```text
workflow snapshot wire round-trip
unknown schema/version rejected
invalid node/artifact binding rejected
cycle/invalid dependency cannot cross trust boundary
exact queued snapshot/digest is executed
reorder changes execution order when dependencies permit
duplicate transformation instances remain distinct
priority does not invent dependency
analyzer binds to exact selected artifact
Disabled is not executed
Conditional is not silently treated as Always
requested outputs correspond to actually executed terminal nodes
cancellation between nodes cleans uncommitted output
stdout/NDJSON remains machine-only
```

Use fake deterministic capability executors or existing test seams rather than GPU workers.

## Capability/status outcome

Do not add a new public capability solely for the executor. The success condition is that a compiled Processing Studio Workflow can truthfully reach the existing Engine capability implementations through `uta-analyze`, instead of hitting the compatibility fail-closed placeholder.

## Durable completion update

Set card 15's current state/result in `tasks/remaining-models/STATE.md`. If this changes a durable process-boundary or execution-contract conclusion, update `docs/KEY_CONCLUSIONS.md` as well. Do not create a completion log under `docs/`.

Include:

```text
wire contract/version
backend executor entrypoint
Studio serialization path if changed
Preview/execution snapshot proof
CPU fixture tests
process-boundary scan result
remaining blocker, if any
```

Stop after this card.
