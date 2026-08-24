# Uta Studio architecture and design

This directory contains the **current durable architecture** for Uta Studio. It is organized by long-lived responsibility, not by refactor phase. Agent execution instructions live in `docs/agent-tasks/`.

## Authority order

When design documents disagree, use this order:

1. `docs/design/architecture/UTA_SEPARATED_ARCHITECTURE_DESIGN_v1.0.md` — frozen system/component boundaries.
2. `docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md` — authoritative audio-analysis architecture and evidence/fusion contracts.
3. `docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_SEPARATION_PLAN_v1.1.md` — authoritative separation/preprocessing architecture.
4. `docs/design/audio-analysis/UTA_AUDIO_ANALYSIS_COVERAGE_CHECKLIST_v1.0.md` — coverage and acceptance matrix for the analysis stack.
5. Supporting domain specifications below, only where they do not contradict items 1–4.

The separated-architecture handoff supersedes earlier monolithic Singing/Audio design documents. Superseded reference documents are intentionally not retained in this tree.

## Frozen architecture decisions

- `utz` owns domain exchange semantics.
- Runtime Manager is the sole model/runtime lifecycle truth.
- Analysis Engine executes analysis and does not download models.
- Studio owns product workflow/control-plane behavior and does not prepare model tensors.
- Studio communicates with backend components through packaged CLI machine protocols; it does not import backend implementation crates.
- Production resolution is fail-closed: installed, validated, and usable are distinct states.
- Runtime generations are immutable and atomically published/leased.
- Candidate analysis never overwrites Authored chart truth.
- The baseline singing chain is vocal extraction -> lead isolation -> Qwen ASR/Aligner + RMVPE + GAME + DSP -> Fusion/Candidate/HSMM.
- Instrumental extraction is an independent branch from the original mix.
- `audio.lead_partition` is future capability work, not a v1 baseline prerequisite.

## Supporting specifications

### Architecture / process boundary

- `docs/design/architecture/UTA_STUDIO_CLI_PROCESS_BOUNDARY_v1.0.md` — packaged CLI boundary and dependency direction.

### Product integration / UX

- `docs/design/integration/UTA_STUDIO_ANALYSIS_RUNTIME_INTEGRATION_DESIGN_v1.0.md` — Studio reintegration seam and runtime/analysis integration details.
- `docs/design/integration/UTA_STUDIO_ANALYSIS_SETTINGS_MODEL_SELECTION_EXECUTION_UX_DESIGN_v1.0.md` — Analysis settings, model selection, plan preview, execution UX, and Processing Studio behavior.

### Editor

- `docs/design/editor/UTA_STUDIO_EDITOR_INTEGRATION_DESIGN_v1.0.md` — Editor ownership, Candidate/Authored interaction, and authoring boundaries.

### Runtime and API contracts

- `docs/design/runtime/NATIVE_RUNTIME_LOCK_SPEC_v1.0.json` — native runtime identity/lock specification.
- `docs/design/api/API_CONTRACT_CONCLUSIONS_v1.0.md` — durable API reuse/extension conclusions.

### UI references

- `docs/design/ui/UI_REFERENCE_NOTES_v1.0.md` — UI reference notes.
- `docs/design/ui/analysis-settings/README.md` — Analysis settings/model-resolution/plan-preview diagrams.
- `docs/design/ui/reference/` — Processing Studio reference images.

## Related current state

- `docs/KEY_CONCLUSIONS.md` — current technical conclusions; source/tests override stale prose.
- `tasks/remaining-models/STATE.md` — current model/resource readiness.
- `docs/engineering-constraints.md` — repository engineering and verification constraints.
- `docs/agent-tasks/README.md` — operational task/runbook index.

Do not recreate `refactor/`, `final-v1/`, or `90-REFERENCE-SUPERSEDED/` architecture folders. A future architecture change should create a new versioned design document, not a phase-named directory.
