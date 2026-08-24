# 21 — Final-v1 Design Parity Audit

**State:** `READY`
**Audit revision:** 2
**Precondition:** cards 15–19, 20A, and 20 are terminal; card 20A and card 20 should be `READY` for a green implementation result
**Task class:** static/audit closure; no model inference
**Owner:** active implementation/review agent

## Read

```text
AGENTS.md
docs/engineering-constraints.md
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
docs/KEY_CONCLUSIONS.md
```

Use current source/tests plus `tasks/remaining-models/STATE.md` for prior card outcomes; deleted completion logs are not audit inputs.

## Mission

Answer one question from current source, not historical claims:

> After model closure and feature closure, is every final-v1 product capability either implemented and evidenced, or explicitly outside the final design / retained as an accepted blocker?

This is not the expensive release/Nix pass. It is the final design-completeness gate before the later explicit release pass reserved by `AGENTS.md`.

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
Vocal/BGM/Lead/VocalResidual executable lanes, with Backing/Harmony retained only as chart roles/future audio identities
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

The actual final package build is deferred to the later explicit release pass reserved by `AGENTS.md`.

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

Do not run model inference or `nix build` in this static audit; accelerator authorization remains governed by `AGENTS.md` for any separately justified dynamic follow-up.

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

## Current result


**State:** `READY`

Audit revision 2 reran the current-source design/parity review after 21A–21C. All revision-1 implementation gaps are closed: reachable product analysis execution uses exact `EngineQueueIntent -> AnalysisCliClient -> uta-analyze`; every advertised audio quality gate has a typed Plan-bound result and fail-closed/degraded behavior; and canonical **Uta! Studio** display identity, i18n, generated docs and active audit links are current.

Revision 2 also enforced the current repository rule that hash verification is not required. Analysis Engine, app-core, Runtime Manager, native workers, conversion/install utilities and UTZ no longer recompute or compare hashes to accept/reject content. Existing hash fields remain compatibility metadata, content IDs, fingerprints or provenance. Safe-path, regular-file, declared-file-set, byte-size, schema, semantic identity, correlation, completeness and atomic-publication checks remain in force.

The affected Card-20 lanes remain green. Focused non-inference suites passed: Analysis Engine `132 passed, 2 ignored`; app-core `467 passed`; Runtime Manager `61 passed`; OpenVINO Worker `53 passed`; GGML Worker `4 passed`; Qwen Worker `15 passed`; UTZ `14 passed`; and Desktop i18n `5 passed`. The product identity gate, docs-current check, process-boundary/dependency/import scans, missing-active-link scan, source-line limit, hash-rejection scan, Rust/Python/shell syntax checks, formatting and `git diff --check` passed. Packaging definitions, worker/notices coverage and canonical CLI environment variables remain statically present. No model inference, download, accelerator context or Nix build was run.

### Parity matrix

| design area | implementation | evidence | decoupling status | remaining blocker |
| --- | --- | --- | --- | --- |
| Repository/process architecture | packaged CLI protocols only; native Production execution; source media remains read-only | exact-intent and real CLI tests; dependency/import/Desktop-spawn scans | clean | final Nix/package execution is deferred to release acceptance |
| Workflow / Processing Studio | compiled typed DAG, legal reorder/duplicates, exact bindings, five conditions, Preview/queue identity | Workflow/CLI/app-core suites and Card-20 bubble rerun | clean | none |
| Analysis / experts / Fusion | retained capability registry is wired; conditional experts, correlated evidence, quality gates, Candidate/finalization and quantization are typed | Analysis Engine `132 passed, 2 ignored`; app-core `467 passed` | backend-owned behind `uta-analyze` | no coding gap; retained model-quality/license promotion blockers remain |
| Audio semantics | truthful Original/Vocal/Guide/Lead/CleanLead/Instrumental roles; VocalResidual is not Backing/Harmony; no implicit fallback | semantic, quality-gate and native worker suites | clean | `audio.lead_partition` remains explicitly future/optional |
| Editor / Evidence Workbench | Candidate/Authored, compare/merge, review, undoable suggestions, read-only evidence and selected-artifact audition are implemented | app-core/UI action coverage and Card-20 diagnostics | app-core local domain only | none |
| Export | Studio owns atomic UTZ/UltraStar export with file-set, byte-size, schema, semantic and decode validation | app-core/UTZ/export bubble coverage | correctly outside Engine | none |
| Contracts / packaging readiness | API catalogue, EN/zh-CN/ja parity, docs, runtime metadata, CLIs/workers/notices and Wayland wrapper are present | identity/i18n/docs/line-limit/static packaging gates | packaged boundary defined | actual Nix build/repository acceptance deferred |
| Model/Production claims | integration and Production readiness remain separate per resource | `STATE.md` and Runtime Manager policy facts | Runtime Manager-owned | 11 explicit external quality/provenance/license blockers remain `production_ready=no` |

Card 21 is green for design completeness. Card 20 remains `READY`. The next phase is the separately reserved final repository/Nix release acceptance; it must not reinterpret retained model blockers as completed Production promotion.
