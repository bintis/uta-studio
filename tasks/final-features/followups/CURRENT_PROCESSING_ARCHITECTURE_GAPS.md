# Current Processing Architecture Gaps

**Date:** 2026-08-28

**Purpose:** Direct implementation handoff for the next coding agent.

**Implementation status:** `COMPLETED` on 2026-08-28.

**Durable review status:** Closed. Follow-ups 21E–21H are `READY`; Card 21 revision 6 reran the full current-source parity audit and is `READY`. `tasks/remaining-models/STATE.md` and `docs/KEY_CONCLUSIONS.md` carry the durable result. The reserved whole-workspace/Nix/final packaged acceptance remains a later explicit release pass.

**Scope:** Current-source gaps only. Fix the implementation; do not redesign the entire analysis stack.

The “Current source problem” sections below preserve the pre-implementation findings that motivated this handoff.

## 0. Current architecture to preserve

The current processing architecture is fundamentally sound and should **not** be replaced with a generic all-purpose DAG runtime.

```text
Desktop / Processing Studio
        |
        v
app-core WorkflowDefinition
        |
        v
compiled workflow wire + AnalyzeRequest
        |
        | stdio JSON/NDJSON process boundary
        v
uta-analyze / Analysis Engine
        |
        +--> Planner -> EnginePlan
        |
        +--> RuntimeManager library -> model/runtime/tool resolution + leases
        |
        +--> native workers
        |
        v
Evidence -> Candidate pool -> final decision -> CanonicalSingingTrack
        |
        v
optional quantization -> Candidate VocalChart
        |
        v
Studio validates result -> immutable Artifact revisions -> DB activation
```

Important current boundaries to keep:

- Studio communicates with `uta-analyze` / `uta-runtime` through process protocols.
- Analysis Engine may use `uta_runtime_manager` directly as a backend library.
- Preview freezes an exact request/plan snapshot and queue execution reuses that snapshot.
- Runtime Manager owns model/runtime/tool readiness and executable resolution.
- Native workers remain supervised child processes with bounded machine protocols.
- AI judgment is a **candidate selector**, not an evidence generator.
- Candidate output never silently overwrites Authored chart truth.

Do **not** turn this work into a rewrite of `engine.rs` or a universal DAG executor.

---

# Work package A — AI judgment convergence

## A1. P0 — Exact Plan Preview currently loses `fusion_mode`

### Current source problem

The request wire correctly carries:

```text
fusion_mode = algorithm | ai_judgment
```

and Analysis Engine execution correctly reads that mode.

However backend `CompiledWorkflowExecutionPlanV1` currently contains:

```rust
identity
nodes
terminal_outputs
fusion_policy
```

but **does not contain `fusion_mode`**.

At the Studio boundary, `WorkflowExecutionPlanWireV1` *does* have `fusion_mode` with `#[serde(default)]`, so an Engine plan that omits the field is silently decoded as `Algorithm`.

This means an AI request can execute AI judgment while the returned Plan projection appears to be Algorithm.

`validate_workflow_plan_identity()` also does not currently compare request `fusion_mode` against planned `fusion_mode`.

### Required fix

1. Add `fusion_mode` to backend `CompiledWorkflowExecutionPlanV1`.
2. Populate it from validated `WorkflowExecutionV1`.
3. Make the serialized real `uta-analyze plan` response carry the exact mode.
4. Make app-core reject request/plan `fusion_mode` mismatch.
5. Do not rely on `serde(default)` to hide a missing mode for exact-plan validation.
6. Plan Preview must visibly show `Algorithm` or `AI judgment`.

### Main files

```text
analysis-engine/src/workflow_executor.rs
analysis-engine/src/workflow.rs
analysis-engine/src/planner/plan.rs
app-core/src/backend_cli/analysis_wire.rs
app-core/src/analysis_engine_adapter.rs
desktop/src/studio/analysis_preview.rs
```

### Acceptance

Add a real packaged/debug CLI round-trip test:

```text
AI workflow request
-> uta-analyze plan
-> returned workflow_execution.fusion_mode == ai_judgment
-> app-core exact-plan validation passes
```

Also test a deliberately mismatched plan and confirm app-core fails closed.

---

## A2. P0 — Fusion agent executable is still Studio-owned raw path

### Current source problem

Current path flow is effectively:

```text
AppConfig.fusion_agent_executable
-> EngineRunDraft
-> AnalysisRequestIntent
-> AnalyzeRequest.execution_policy.fusion_agent_executable: PathBuf
-> Analysis Engine
```

Engine then resolves:

```text
request path
or
UTA_STUDIO_FUSION_AGENT_CLI_PATH
```

Runtime Manager currently has no `fusion_agent_adapter` implementation.

### Required target

Canonical resource:

```text
tool:fusion_agent_adapter
```

Runtime Manager owns:

- persistent configured external-tool path;
- executable validation;
- status/readiness;
- resolve;
- environment override/discovery if retained;
- stable tool identity/version metadata when available.

Studio owns only:

```text
fusion_mode = ai_judgment
```

plus explicit configure/clear actions sent to Runtime Manager.

Remove the raw executable path from Studio-owned analysis intent and from `AnalyzeRequest`.

### Main files

```text
runtime-manager/src/catalog/**
runtime-manager/src/store.rs
runtime-manager/src/resolver.rs
runtime-manager/src/cli.rs
app-core/src/config.rs
app-core/src/analysis_engine_adapter.rs
app-core/src/backend_cli/analysis_wire.rs
app-core/src/backend_cli/runtime_*.rs
desktop/src/studio/settings/models.rs
desktop/src/studio/actions_settings.rs
analysis-engine/src/execution/agent_client.rs
analysis-engine/src/engine.rs
```

### Acceptance

```text
Studio request JSON contains no raw fusion-agent path
Runtime Manager can configure/status/resolve/clear tool:fusion_agent_adapter
non-executable path is unusable
missing adapter blocks AI mode before execution
Algorithm mode does not require the tool
```

---

## A3. P1 — Planner does not model AI adapter as a required resource

### Current source problem

`Planner::requirements()` currently includes model resources and `tool:ffmpeg`, but does not add an AI adapter resource when `fusion_mode == AiJudgment`.

Therefore Preview cannot truthfully resolve/block the AI dependency.

### Required fix

When the validated workflow selects AI judgment, requirements must include:

```text
tool:fusion_agent_adapter
required = true
reason = fusion.candidate_graph / ai_judgment
```

`EnginePlan.resolved_resources` must contain the tool status.

Plan Preview must become blocked when the tool is missing/unusable.

### Main files

```text
analysis-engine/src/planner/plan.rs
analysis-engine/src/engine.rs
runtime-manager/src/resolver.rs
app-core/src/analysis_engine_adapter.rs
desktop/src/studio/analysis_preview.rs
```

---

## A4. P0 — General-purpose coding-agent CLIs are incorrectly treated as direct adapters

### Current source problem

`app-core/src/agent_discovery.rs` currently scans binaries such as:

```text
claude
codex
gemini
cursor-agent
aider
amp
```

Settings then offers those executable paths directly.

But Analysis Engine launches the configured executable with no provider-specific argv and writes Uta's custom JSON protocol directly to stdin:

```text
uta.fusion_agent_request / v1
```

It expects stdout to be exactly:

```text
uta.fusion_agent_response / v1
```

A normal Codex/Claude/Gemini/Aider CLI does not automatically implement this protocol.

### Required fix

The Engine-facing executable must be a **Uta Fusion Agent Adapter**.

Valid architecture:

```text
Analysis Engine
-> Uta Fusion Agent Adapter
-> Codex / Claude / Gemini / local provider
```

Do not mark a generic coding-agent executable usable solely because it exists on PATH.

Options:

- ship/provider-specific adapter executables such as `uta-fusion-agent-codex`;
- or require an adapter manifest/handshake proving Uta fusion protocol support;
- or retain manual executable selection but validate the Uta adapter protocol before reporting it usable.

`agent_discovery.rs` must stop presenting arbitrary general-purpose agent CLIs as directly compatible endpoints unless they are wrapped by a verified adapter.

### Acceptance

```text
plain codex/claude executable alone is not reported as a usable Fusion Agent Adapter
verified adapter is reported usable
AI Stage 4 enablement follows Runtime Manager tool usability, not Option<String>::is_some()
```

---

## A5. P1 — AI decision provenance and fingerprint semantics are incomplete

### Current source problem

`AnalysisProvenanceV1` currently records algorithm/resource versions such as:

```text
resources
calibration_version
fusion_version
hsmm_version
quantization_version
audio_quality_version
postprocess_version
```

AI judgment currently records no dedicated decision provenance.

Also:

- AI mode still reports `HSMM_VERSION` even when HSMM did not execute;
- current request fingerprint still contains the local raw fusion-agent path;
- no candidate-set digest, selected IDs, or response digest is preserved.

### Required fix

Add explicit final-decision provenance. At minimum AI mode must retain:

```text
decision_mode = ai_judgment
adapter_resource = tool:fusion_agent_adapter
adapter_protocol_version
resolved adapter identity/version when available
input candidate-set digest
selected candidate ids
adapter response digest
```

Algorithm mode should truthfully retain its HSMM/Viterbi algorithm identity.

Do not claim an HSMM decision when AI selected the path.

Do not store/request provider chain-of-thought.

Fingerprint stable identity, not local filesystem path.

Two different valid AI selections must never be silently conflated as one deterministic decision event.

### Main files

```text
analysis-engine/src/contract/result.rs
analysis-engine/src/fingerprint.rs
analysis-engine/src/engine.rs
analysis-engine/src/execution/agent_client.rs
app-core/src/backend_cli/analysis_wire.rs
app-core/src/analyzer/engine_run.rs
```

---

## A6. P1 — AI final-path validation is weaker than the Algorithm path

### Current source problem

The adapter currently verifies:

```text
selection is non-empty
selected object is structurally identical to one input candidate
```

Then canonical track construction catches overlap/invalid note structure.

However Algorithm decoding additionally reasons about hard-boundary constraints. The AI path does not currently apply the same final path constraint validation.

The prompt also currently says the selection should "cover the full song". That is not a correct singing-note invariant because intro/rest/instrumental/outro gaps are valid.

### Required fix

Introduce a shared final candidate-path validator used after either selector where appropriate.

Validate at least:

```text
candidate membership
ordered ascending timeline
no duplicate selected IDs unless explicitly legal (normally reject)
non-overlap
hard-boundary constraints
finite/valid candidate values
canonical singing-track validation
```

Change the AI instruction from "cover the full song" to something equivalent to:

```text
select an ordered, non-overlapping valid singing-note path from the supplied candidates and respect required hard boundaries
```

Do not require notes to cover silence/instrumental regions.

### Main files

```text
analysis-engine/src/execution/agent_client.rs
analysis-engine/src/fusion/hsmm.rs
analysis-engine/src/candidate_pipeline.rs
analysis-engine/src/fusion/canonical.rs
```

---

## A7. P2 — AI UX/readiness/network disclosure is incomplete

### Current source problem

Stage 4 currently enables AI judgment based on:

```text
config.fusion_agent_executable.is_some()
```

not Runtime Manager tool usability.

Current UI also describes a "locally installed AI coding agent CLI" but does not clearly state at the decision point that candidate metadata may be sent to an external network provider.

Plan Preview currently does not display exact AI decision mode + resolved adapter readiness.

### Required fix

- Settings shows Fusion Agent Adapter as an external **tool**, with usable/missing/unusable status.
- Processing Studio AI judgment is enabled only when Runtime Manager reports the adapter usable.
- Stage 4 displays concise external-provider/candidate-metadata disclosure when AI mode is selected.
- Preview shows exact decision mode and tool readiness but never contacts the provider.
- Add localized actionable errors for missing/unusable/protocol/timeout/cancel/provider failure.
- Keep EN / zh-CN / ja catalogs synchronized.

---

# Work package B — Audio preprocessing policy

## B1. P1 — Lead isolation is hard-coded into the default workflow as `Always`

### Current source problem

`app-core/src/workflow/default_definition.rs` currently creates:

```text
vocal_bgm_split   Always
lead_isolate      Always
vocal_cleanup_1   audio.denoise / Always
```

Therefore the default analysis route is effectively:

```text
OriginalMix
-> Vocal/BGM separation
-> Lead isolation
-> Denoise
-> analyzers
```

Lead isolation is technically disableable in Processing Studio and the compiler already supports transparent bypass back to `Vocal`, so the low-level dataflow capability already exists.

The product problem is that a new/default workflow executes Lead isolation automatically rather than treating it as an explicit preprocessing choice.

### Target behavior

For ordinary vocal analysis, Lead isolation should be an explicit optional preprocessing transformation, similar in product behavior to denoise/dereverb.

Recommended default:

```text
Lead isolation = Off
Denoise        = Off
Dereverb       = Off
```

`audio.extract_vocals` remains demand-driven/required when an OriginalMix must be converted to a vocal analysis source.

Do not equate `audio.extract_vocals` with `audio.lead_isolate`.

### Important exception

If the user explicitly requests a `LeadVocal` stem artifact, Lead isolation becomes required for that request.

```text
ordinary transcript/pitch/Candidate analysis:
    Lead isolation follows workflow/user preprocessing choice

explicit LeadVocal stem output:
    Lead isolation required
```

### Existing behavior to preserve

When Lead isolation is disabled:

```text
Vocal/BGM split.vocal
-> analyzers directly
```

Do not fail the analysis merely because lead/residual topology evidence is unavailable.

Current `estimate_vocal_topology(..., None, None)` returning truthful `Unknown` / no fake confidence is preferable to silently forcing Lead isolation.

### Main files

```text
app-core/src/workflow/default_definition.rs
app-core/src/workflow/validation.rs
app-core/src/workflow/definition.rs
analysis-engine/src/planner/plan.rs
analysis-engine/src/engine.rs
desktop/src/studio/processing_studio/**
desktop/src/studio/analysis_preview.rs
```

---

## B2. P1 — Default denoise is also incorrectly `Always`

### Current source problem

`vocal_cleanup_1` is created as:

```text
audio.denoise
ExecutionPolicy::Always
```

while dereverb is not inserted by default.

This makes default denoise effectively mandatory in the ordinary workflow unless the user manually disables/removes it.

### Required fix

Default preprocessing should not silently mutate the analysis input with denoise/dereverb.

Recommended default workflow:

```text
Vocal extraction: demand-driven baseline when source requires it
Lead isolation:   optional, default Off
Denoise:          optional, default Off
Dereverb:         optional, default Off
```

Processing Studio may add/enable these transformations explicitly.

If product-level Analysis Settings contains preprocessing defaults, those settings should determine the created/effective workflow intent without creating a second backend truth source.

---

## B3. P1 — Planner currently conflates “analysis needs vocal input” with “analysis needs isolated lead”

### Current source problem

Planner calculates:

```text
needs_analysis_lead = needs_transcript || needs_alignment || needs_pitch || needs_notes
```

and Lead isolation is then considered whenever `needs_analysis_lead` is true and the workflow selects it.

The name/logic encourages the assumption that ASR/pitch/GAME require `LeadVocal` specifically.

They can also consume the bypassed complete `Vocal` analysis stream when Lead isolation is disabled.

### Required fix

Separate these concepts:

```text
needs_vocal_analysis_input
requests_lead_stem
run_lead_isolate (workflow/user choice)
```

Do not make ordinary transcript/alignment/pitch/note analysis intrinsically require `LeadVocal` semantics.

A useful target decision is:

```text
if input is OriginalMix and vocal analysis is needed:
    audio.extract_vocals required

if workflow says Lead isolation ON:
    audio.lead_isolate runs and analyzers consume LeadVocal
else:
    analyzers consume Vocal

if LeadVocal stem explicitly requested:
    audio.lead_isolate required or request blocked
```

Keep exact analyzer attachment/bypass truth in the compiled workflow/plan.

---

## B4. P2 — Settings / Processing Studio need one coherent preprocessing model

### Required product behavior

The user should be able to understand and control the preprocessing chain without hidden defaults.

At minimum Processing Studio should clearly present:

```text
Vocal extraction      required when needed by the source/request
Lead isolation        On / Off
Denoise               On / Off
Dereverb              On / Off
```

If global Analysis Settings also exposes defaults, use them as **product defaults** that compile into workflow intent. Do not create separate runtime execution truth outside the workflow/Engine Plan.

Plan Preview must show the actual route, for example:

```text
Vocal -> RMVPE
```

or:

```text
Vocal -> Lead isolation -> Denoise -> RMVPE
```

The actual Engine plan remains authoritative.

---

# Work package C — Tests that are currently missing

Existing focused suites are green, but they do not cover the new cross-layer invariants above.

Add tests for at least:

## AI

```text
1. AI workflow -> real uta-analyze plan -> fusion_mode remains ai_judgment.
2. request/plan fusion_mode mismatch fails closed.
3. AI workflow requirements include tool:fusion_agent_adapter.
4. unusable adapter makes Preview blocked.
5. Algorithm workflow does not require adapter.
6. raw executable path is absent from Studio-owned AnalyzeRequest wire.
7. plain codex/claude binary is not considered a verified adapter.
8. AI manifest contains decision provenance.
9. AI result does not falsely claim HSMM selected the path.
10. different valid AI selections are not conflated as one deterministic decision event.
11. AI path violating hard boundaries is rejected.
12. Preview never starts the adapter/provider.
```

## Preprocessing

```text
1. new/default workflow does not force Lead isolation.
2. new/default workflow does not force denoise/dereverb.
3. Candidate analysis succeeds with Lead isolation disabled and analyzers bound to Vocal.
4. explicit LeadVocal stem request requires Lead isolation.
5. disabling Lead isolation never silently enables another separator.
6. topology without lead/residual evidence remains truthful Unknown/degraded/review as applicable.
7. exact Plan Preview displays the actual preprocessing route.
8. existing transparent bypass tests remain green.
```

---

# Recommended implementation order

Do these serially:

```text
1. A1 Exact fusion_mode in Plan/Preview
2. A2 + A3 Runtime Manager adapter resource + Planner requirement
3. A4 Adapter abstraction/discovery cleanup
4. A5 AI provenance/fingerprint
5. A6 AI shared path validation
6. B1 + B2 default preprocessing policy
7. B3 Planner semantic cleanup
8. B4 product UI/defaults/Preview
9. A7 final UX/i18n/error copy
10. focused regression matrix
```

Do not mix the Runtime Manager adapter ownership change with a general Engine execution rewrite.

---

# Non-goals

Do **not** do these as part of this work:

- rewrite the Analysis Engine into a universal dynamic DAG runtime;
- move Analysis Engine -> Runtime Manager calls to an `uta-runtime` subprocess;
- let AI generate new note/evidence values;
- silently fall back from AI judgment to Algorithm;
- silently run Lead isolation/denoise/dereverb when disabled;
- make `audio.lead_partition` part of this work;
- change Candidate/Authored ownership;
- weaken exact Preview -> exact queued request semantics;
- add automatic model/runtime fallback.

---

# Current verification baseline

At the time this gap list was created, focused current-source tests were green:

```text
uta-analysis-engine: 179 passed, 2 ignored
uta-analyze CLI tests: 4 passed
uta-studio-core: 386 passed, 1 ignored
uta-studio-desktop: 158 passed
```

These green tests did **not** prove the gaps above were closed; several then-current DTO defaults allowed missing AI-plan fields to pass silently.

Post-implementation focused verification completed on 2026-08-28:

```text
uta-analysis-engine library: 205 passed, 2 ignored
uta-analysis-engine CLI integration: 4 passed
uta-studio-core: 397 passed, 1 ignored
uta-runtime-manager: 67 library + 10 CLI passed
uta-studio-audio: 10 passed, 1 ignored
uta-studio-desktop: 175 passed
utz: 13 library + 3 conformance + 5 metadata passed
cargo fmt --all -- --check: passed
cargo xtask docs check: documentation outputs are current
process-boundary, source-line-limit, localization-parity, and unstaged diff checks: passed
```

After implementation, rerun at least:

```text
bash dev.sh -c cargo test -p uta-runtime-manager
bash dev.sh -c cargo test -p uta-analysis-engine
bash dev.sh -c cargo test -p uta-studio-core
bash dev.sh -c cargo test -p uta-studio-desktop
bash dev.sh -c cargo fmt --all -- --check
bash dev.sh -c cargo xtask docs check
```

Also run process-boundary scans and `git diff --check` without rewriting retained test evidence.

---

# Definition of done

This handoff is complete only when all of the following are true:

```text
[x] AI decision mode is exact in request, Engine Plan, Preview, queue and execution.
[x] Runtime Manager owns tool:fusion_agent_adapter path/status/resolve.
[x] AI adapter is a verified Uta protocol endpoint, not an arbitrary coding-agent binary.
[x] AI adapter is an explicit required Plan resource in AI mode.
[x] AI decision provenance/fingerprint/cache behavior is truthful and non-deterministic-safe.
[x] AI-selected paths receive full shared final-path/canonical validation.
[x] AI failures never execute Algorithm as fallback.
[x] Lead isolation is no longer silently forced for ordinary analysis.
[x] Denoise/dereverb are no longer silently forced by the default workflow.
[x] Explicit LeadVocal output still requires Lead isolation.
[x] Disabled Lead isolation transparently feeds Vocal to analyzers.
[x] Preview shows the exact preprocessing route and exact AI decision mode/readiness.
[x] EN / zh-CN / ja user-visible AI/preprocessing copy is synchronized.
[x] Focused test matrix and hygiene gates pass.
```
