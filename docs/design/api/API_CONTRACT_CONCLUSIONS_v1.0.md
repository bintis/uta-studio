# Uta! Studio API Contract Conclusions — v1.0 FINAL

This file contains durable API conclusions only. It is not a change log or implementation journal.

## Decision rule

Prefer, in order:

```text
Reuse existing API
  -> backward-compatible extension when required
  -> add a new API only when existing semantics cannot express the behavior
```

Do not create thin aliases, bypass ArtifactRef / AnalysisRequest / UiCommand safety boundaries, or add app-owned commands merely because a new page uses different terminology.

## Existing APIs to reuse

The final design should preferentially reuse these established boundaries:

- analysis graph/plan: `get_analysis_graph`, `preview_analysis_plan`, `preview_full_analysis_plan`, `run_analysis_plan`, `run_analysis_node`, `run_analysis_node_downstream`, `run_analysis_request`;
- artifact inspection/revision: `inspect_analysis_node_io`, `load_analysis_artifacts`, `load_artifact_revisions`, `inspect_artifact`, `preview_artifact`, `artifact_lineage`, downstream-impact preview, typed artifact compare and chart-revision merge;
- runtime/model state: `analysis_runtime_status`, `list_audio_models`, `get_audio_model_status`, `install_audio_model`;
- UI/editor command boundary: `editor_actions`, `dispatch_ui_interaction`.

Studio/app-core must keep local wire/domain DTOs and must not import backend implementation crates to reuse these APIs.

## APIs that require an actual gap before addition

These concepts may justify a dedicated API only after current source is audited and existing boundaries are proven insufficient:

- workflow capability registry;
- per-song workflow load/save;
- editing-time workflow compile/validation preview before an `AnalysisRequest` exists.

No API is pre-authorized by appearing in a design document.

## APIs that should normally not be added

Avoid duplicate surfaces such as:

```text
run_workflow
open_workflow_artifact
editor_open_candidate
processing_studio_model_status
```

Use the established execution, artifact, editor and model/runtime APIs unless a concrete semantic gap is demonstrated.

## Durable acceptance rule

`API_CAPABILITIES`, UI interaction capabilities, error handling, access classification and i18n must reflect the APIs actually exposed by current source. When the contract changes, update this conclusion document to the new final state; do not append dated change records or create an API log.