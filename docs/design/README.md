# Uta! Studio architecture and design

This directory contains the **current durable architecture** for Uta! Studio. It is organized by long-lived responsibility, not by refactor phase. Repository rules live in `AGENTS.md`, and current closure state lives in `tasks/remaining-models/STATE.md`.

## Authority order

When design documents disagree, use this order:

1. `docs/design/architecture/UTA_SEPARATED_ARCHITECTURE_DESIGN_v1.0.md` — frozen system/component boundaries.
2. `docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md` — authoritative audio-analysis architecture and evidence/fusion contracts.
3. `docs/design/audio-analysis/UTA_EXPERT_FUSION_POLICY_AND_REPAIR_v1.0.md` — authoritative stage-3/stage-4 enablement, typed fusion-policy, fallback, and candidate-selection repair.
4. `docs/design/audio-analysis/UTA_AI_JUDGMENT_FUSION_MODE_v1.0.md` — authoritative contract for the explicit non-default AI judgment decision mode, Runtime Manager tool ownership, network/privacy boundary, provenance, and hard-fail semantics.
5. `docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_SEPARATION_PLAN_v1.1.md` — authoritative separation/preprocessing architecture.
6. `docs/design/audio-analysis/UTA_AUDIO_ANALYSIS_COVERAGE_CHECKLIST_v1.0.md` — coverage and acceptance matrix for the analysis stack.
7. Supporting domain specifications below, only where they do not contradict items 1–6.

The separated-architecture handoff supersedes earlier monolithic Singing/Audio design documents. Superseded reference documents are intentionally not retained in this tree.

## Frozen architecture decisions

- `utz` owns domain exchange semantics.
- Runtime Manager is the sole model/runtime lifecycle truth.
- Analysis Engine executes analysis and does not download models.
- Studio owns product workflow/control-plane behavior and does not prepare model tensors.
- Studio communicates with backend components through packaged CLI machine protocols; it does not import backend implementation crates.
- Production resolution is fail-closed: installed, validated, and usable are distinct states.
- Every model's inference is Rust-owned graph construction over the pinned upstream GGML shared-library ABI. Vulkan is the default; CPU is explicit experimental mode, and GPU/integrated-GPU failures never fall back to it.
- Normal Studio analysis sends `RuntimePolicy::Production`; missing installation, runtime, model implementation, or structural requirements fail closed.
- Runtime generations are immutable and atomically published/leased.
- Candidate analysis never overwrites Authored chart truth.
- The current singing chain is optional vocal extraction/lead cleanup -> transcription and forced alignment -> RMVPE + Maximum-only FCPE + Acoustic DSP -> GAME note evidence with optional challengers -> Fusion/Candidate -> Stage-4 decision. Algorithm/HSMM is the deterministic default; explicit AI judgment is the constrained external alternative.
- Generated transcription and forced alignment run on Rust/upstream-GGML Qwen3 providers, with FireRed as an optional never-substituting transcript challenger; caller-provided lyrics remain independent input.
- Leap XE90 owns one dual-output invocation for vocals and the instrumental residual. PolarFormer is an explicit experimental strategy, not an independently scheduled default instrumental branch.
- `audio.lead_partition` is future capability work, not a v1 baseline prerequisite.
- Stage 4 defaults to deterministic Algorithm fusion; explicit AI judgment is allowed in normal Production analysis, may use a networked provider, and may only select from real Engine candidates.
- Runtime Manager owns the external `tool:fusion_agent_adapter` executable/readiness; Studio selects decision mode but does not send a raw adapter path to Analysis Engine.
- AI-judgment failure never silently falls back to Algorithm, and fresh AI decisions are not assumed deterministic-cache reusable.

Older linked specifications may contain historical model examples. The current executable inventory and schema-7 behavior in `tasks/remaining-models/STATE.md`, `docs/AUDIO_MODELS.md`, and `docs/KEY_CONCLUSIONS.md` override those examples.

## Supporting specifications

### Architecture / process boundary

- `docs/design/architecture/UTA_STUDIO_CLI_PROCESS_BOUNDARY_v1.0.md` — packaged CLI boundary and dependency direction.
- `docs/design/architecture/DAG_LAYOUT_ENGINE.md` — Advanced Graph projection, authoritative snapshot selection, generic DAG layout, and routing rules.

### Product integration / UX

- `docs/design/integration/UTA_STUDIO_ANALYSIS_RUNTIME_INTEGRATION_DESIGN_v1.0.md` — Studio reintegration seam and runtime/analysis integration details.
- `docs/design/integration/UTA_STUDIO_ANALYSIS_SETTINGS_MODEL_SELECTION_EXECUTION_UX_DESIGN_v1.0.md` — Analysis settings, model selection, plan preview, execution UX, and Processing Studio behavior.
- `docs/design/audio-analysis/UTA_AI_JUDGMENT_FUSION_MODE_v1.0.md` — explicit Algorithm/AI-judgment Stage 4 mode, external adapter ownership, privacy, provenance, and failure semantics.

### Editor

- `docs/design/editor/UTA_STUDIO_EDITOR_INTEGRATION_DESIGN_v1.0.md` — Editor ownership, Candidate/Authored interaction, and authoring boundaries.
- `docs/design/editor/UTA_STUDIO_EDITOR_OPENUTAU_ENRICHMENT_DESIGN_v1.0.md` — implemented Editor enrichment for evidence suggestions, lyric readings, technique detail, and fast lyric entry.
- `docs/design/editor/UTA_STUDIO_EDITOR_OPENUTAU_ENRICHMENT_TODO_v1.0.md` — completed implementation checklist; manual running-UI interaction review remains recommended before release handoff.

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
- `AGENTS.md` — active repository and execution rules.

Do not recreate `refactor/`, `final-v1/`, or `90-REFERENCE-SUPERSEDED/` architecture folders. A future architecture change should create a new versioned design document, not a phase-named directory.
