# 21 — Final-v1 Design Parity Audit

**Precondition:** cards 15–19, 20A, and 20 are terminal; card 20A and card 20 should be `READY` for a green implementation result
**Task class:** static/audit closure; no model inference
**Owner:** active implementation/review agent

## Read

```text
AGENTS.md
docs/agent-tasks/MODEL_GPU_WORK_POLICY.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
tasks/final-features/21_FINAL_DESIGN_PARITY_AUDIT.md
docs/design/architecture/UTA_SEPARATED_ARCHITECTURE_DESIGN_v1.0.md
docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md
docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_SEPARATION_PLAN_v1.1.md
docs/design/audio-analysis/UTA_AUDIO_ANALYSIS_COVERAGE_CHECKLIST_v1.0.md
docs/design/architecture/UTA_STUDIO_CLI_PROCESS_BOUNDARY_v1.0.md
docs/design/integration/UTA_STUDIO_ANALYSIS_RUNTIME_INTEGRATION_DESIGN_v1.0.md
docs/design/integration/UTA_STUDIO_ANALYSIS_SETTINGS_MODEL_SELECTION_EXECUTION_UX_DESIGN_v1.0.md
docs/agent-tasks/FINAL_V1_ACCEPTANCE_CHECKLIST.md
docs/KEY_CONCLUSIONS.md
```

Use current source/tests plus `tasks/remaining-models/STATE.md` for prior card outcomes; deleted completion logs are not audit inputs.

## Mission

Answer one question from current source, not historical claims:

> After model closure and feature closure, is every final-v1 product capability either implemented and evidenced, or explicitly outside the final design / retained as an accepted blocker?

This is not the expensive release/Nix pass. It is the final design-completeness gate before `docs/agent-tasks/FINAL_REPOSITORY_ACCEPTANCE.md`.

## Audit categories

### Repository/process architecture

Verify:

```text
Studio/backend implementation crates remain decoupled
Studio uses AnalysisCliClient / RuntimeCliClient only
Desktop -> app-core only
no HTTP inference/control service
no production Python/PyTorch/Transformers/uv/venv path
source media read-only
explicit model download/install policy
```

### Workflow / Processing Studio

Verify real implementation/evidence for:

```text
dynamic audio transformation reorder
type-invalid drop blocked
cycle blocked
duplicate processing nodes
Vocal/BGM/Lead/Backing/Harmony lane semantics as supported by final contract
analyzer binds exact artifact
priority is not dependency
Always/OnDisagreement/MaximumOnly/Disabled execution
compiled DAG executes, not only previews
Advanced Graph matches compiled execution snapshot
Preview == queued execution snapshot/digest
```

### Analysis / experts / Fusion

Verify current capability registry against real call sites and result artifacts:

```text
transcript/alignment/pitch/note/acoustic evidence
optional experts
STARS/ROSVOT dependency-aware evidence
technique evidence
fusion.transcript
fusion.alignment
fusion.singing
fusion.candidate_graph
finalize.vocal_chart
rhythm.quantize contract, if retained
```

No `implementation_exists=true` without a real execution path.

### Audio semantics

Verify:

```text
Original/Vocal/Guide/Lead/CleanLead/Instrumental/Backing/Harmony semantics are not relabeled guesses
lossless/lossy encoding policy matches payload
source timeline preserved
no implicit CPU/Vulkan fallback
```

### Editor / Evidence Workbench

Verify:

```text
existing notes/lyrics editing preserved
Lead/Harmony/Backing/Adlib chart tracks preserved
Candidate opens
Authored saves
re-analysis never silently overwrites Authored
Candidate/Authored compare/merge
Evidence layers read-only
Review Queue navigation
Suggestion accept undoable
Artifact source/waveform/playback selection
A/B audition
technique evidence presentation
```

### Export

Verify Studio/app-core owns user export and real code exists for:

```text
UTZ
UltraStar
exported audio validation
staging cleanup
```

Do not flag an unimplemented backend `AnalysisEngine::export()` placeholder as a product gap when Studio-owned export is the final architecture.

### Contracts / packaging readiness

Verify static state for:

```text
API capability catalogue completeness/uniqueness
i18n EN/zh-CN/ja parity
source file <= 2000 lines
runtime-lock identity/recipes/notices
packaging definitions include required CLIs/workers/notices
canonical env vars for analysis/runtime CLIs
```

The actual final package build is deferred to `docs/agent-tasks/FINAL_REPOSITORY_ACCEPTANCE.md`.

## Dynamic follow-up rule

If this audit finds a concrete implementation gap, do not hide it and do not patch an unrelated subsystem inside the audit.

Finish:

```text
NEEDS_REVIEW
```

and provide a minimal proposed follow-up card name/scope.

Record a focused follow-up Markdown card under, when useful:

```text
tasks/final-features/followups/
```

The same active agent implements the follow-up, verifies it, then reruns the affected focused bubble and card 21 with a new audit revision/completion record.

Do not proceed to final repository acceptance while card 21 is `NEEDS_REVIEW`.

## Static checks

Allowed and expected:

```text
focused cargo tests/checks for changed packages if useful
capability registry scans
Cargo dependency scans
implementation namespace scans
project-name/source-size/i18n/docs static gates where cheap
process tree inspection
git diff --check
```

Do not run model inference or `nix build` in this static audit; accelerator authorization remains governed by `docs/agent-tasks/MODEL_GPU_WORK_POLICY.md` for any separately justified dynamic follow-up.

## Green result

Card 21 is `READY` only when:

```text
cards 01–13 are terminal, with each model completion distinguishing integration_ready from production_ready
cards 15–19 are READY
card 20A Studio/backend/UI parity closure is READY
card 20 product E2E feature bubble is READY
no known final-v1 coding capability is still a placeholder/fail-closed stub
process-boundary decoupling gates are clean
no false capability/model Production claim exists
```

If a model/license limitation is genuinely outside code control, report it explicitly rather than calling the implementation incomplete; release acceptance still decides whether such a retained blocker is acceptable.

## Durable completion update

Set card 21's current state/result in `tasks/remaining-models/STATE.md`. Update `docs/KEY_CONCLUSIONS.md` with any final durable design/parity conclusion. Do not create a completion log under `docs/`.

Include a concise parity matrix:

```text
design area | implementation | evidence | decoupling status | remaining blocker
```

Stop after this audit.
