# Uta Studio API Change Ledger — v1.0 FINAL

**用途：** 本次 `native-inference` 重构的 API 新增/扩展审计记录。  
**规则：** 在写任何新的 app-owned command/public UI-domain API 前先登记本表。

---

# 1. 决策规则

优先级：

```text
Reuse existing API
    ↓ 不足
Extend existing API with backward-compatible fields/params
    ↓ 语义仍无法表达
Add new API
```

禁止：

- 只因为新页面用了不同名词就复制 API；
- 创建旧 API 的 thin alias；
- 用新 API 绕过现有 ArtifactRef / AnalysisRequest / UiCommand safety boundary；
- 未登记就把 command 加进 `API_CAPABILITIES`。

---

# 2. 本重构应优先复用的现有 API

| Existing command | Expected reuse |
|---|---|
| `get_analysis_graph` | Advanced Graph / compiled graph reading; may extend return semantics instead of aliasing |
| `preview_analysis_plan` | Preview an already-compiled analysis request |
| `preview_full_analysis_plan` | Default/full plan preview |
| `run_analysis_plan` | Generic plan execution |
| `run_analysis_node` | Run one node + upstream closure |
| `run_analysis_node_downstream` | Run downstream |
| `run_analysis_request` | Preferred final execution entry for compiled Workflow |
| `inspect_analysis_node_io` | Processing/Graph inspector reuse |
| `load_analysis_artifacts` | Artifact inventory |
| `load_artifact_revisions` | Revision picker |
| `inspect_artifact` | Workflow artifact inspector |
| `preview_artifact` | Bounded preview |
| `artifact_lineage` | Lineage |
| `preview_artifact_downstream_impact` | Impact |
| `preview_node_downstream_impact` | Node impact |
| `compare_artifacts_typed` | Revision compare |
| `merge_chart_revisions` | Candidate/Authored merge |
| `analysis_runtime_status` | Extend for native runtime status |
| `list_audio_models` | Model picker/catalog |
| `get_audio_model_status` | Model install/backend/license state |
| `install_audio_model` | Explicit install |
| `editor_actions` | Editor action registry |
| `dispatch_ui_interaction` | Existing typed shell interaction boundary |

---

# 3. Expected candidate APIs — do NOT create without final audit

| Candidate | Access | Existing alternatives to audit | Why it may be genuinely new | Decision |
|---|---|---|---|---|
| `list_workflow_capabilities` | read | `get_analysis_graph`, `list_audio_models` | Capability registry is not compiled graph or model catalog | Added — API-001 |
| `load_song_workflow` | read | `get_song_analysis_profile`, `load_config` | Workflow topology is not a parameter profile | Added — API-002 |
| `save_song_workflow` | mutation | `set_song_analysis_profile`, `save_config` | Per-song topology/preset selection has different semantics | Added — API-003 |
| `preview_workflow_compile` | read | `preview_analysis_plan` | Editing-time typed-port/cycle validation happens before AnalysisRequest exists | Added — API-004 |

Likely **not** needed:

```text
run_workflow
open_workflow_artifact
editor_open_candidate
processing_studio_model_status
```

Use existing execution/artifact/editor/model APIs unless an actual gap is proven.

---

# 4. Change record template

Copy one block per API decision.

```md
## API-XXX — <command or proposed capability>

- Date:
- Phase:
- Area:
- Requested behavior:
- Access class:
- Existing APIs inspected:
  - ...
- Existing implementation functions inspected:
  - ...
- Decision: Reuse | Extend | Add | Reject
- Why reuse is sufficient / insufficient:
- Backward compatibility:
- Artifact/source-media safety:
- UI command mapping:
- `API_CAPABILITIES` change:
- `ui_interaction_capabilities` change:
- i18n/error path:
- Final tests:
- Files touched:
- Notes:
```

---

# 5. Ledger

No API additions are pre-authorized merely by being listed in the final design. Agents must append records here as implementation proceeds.

## API-001 — `list_workflow_capabilities`

- Date: 2026-08-22
- Phase: 1/2
- Area: workflow
- Requested behavior: Read the typed Node Capability registry used by Processing Studio before a compiled graph exists.
- Access class: `read`
- Existing APIs inspected: `get_analysis_graph`, `list_audio_models`
- Existing implementation functions inspected: `baseline_graph_spec`, `list_audio_models`
- Decision: Add
- Why reuse is insufficient: A compiled graph contains instances and execution edges; the model catalog contains installable model files. Neither describes editable typed ports, duplicate-instance policy, hard dependencies, or capability classes.
- Backward compatibility: Additive.
- Artifact/source-media safety: Pure in-memory read.
- UI command mapping: Processing Studio reads it through the local typed workflow command surface.
- `API_CAPABILITIES` change: Add one unique `read` entry.
- `ui_interaction_capabilities` change: Add Processing Studio capability/model-picker actions with the page implementation.
- i18n/error path: Unknown capabilities compile to user-facing validation issues.
- Final tests: Registry identity/port validation and API uniqueness.
- Files touched: `app-core/src/workflow/*`, `app-core/src/api.rs`.

## API-002 — `load_song_workflow`

- Date: 2026-08-22
- Phase: 2
- Area: workflow
- Requested behavior: Load versioned per-song workflow topology and layout, migrating legacy audio settings when absent.
- Access class: `read`
- Existing APIs inspected: `get_song_analysis_profile`, `load_config`, `get_analysis_graph`
- Existing implementation functions inspected: `song_analysis_profile_get`, `AudioProcessingPlanSnapshot::from_settings`
- Decision: Add
- Why reuse is insufficient: Analysis profiles are parameter overrides and must not become topology storage; a compiled graph cannot preserve user intent or layout.
- Backward compatibility: Missing rows produce a deterministic migrated default without writing it.
- Artifact/source-media safety: Reads SQLite/config only.
- UI command mapping: Typed Processing Studio load/route action.
- `API_CAPABILITIES` change: Add one unique `read` entry.
- `ui_interaction_capabilities` change: Covered by Processing Studio route.
- i18n/error path: Invalid stored JSON returns a visible workflow-load error.
- Final tests: Legacy default load, schema migration, layout excluded from execution digest.
- Files touched: `app-core/src/workflow/*`, `app-core/src/library_db/*`, `app-core/src/api.rs`.

## API-003 — `save_song_workflow`

- Date: 2026-08-22
- Phase: 2
- Area: workflow
- Requested behavior: Validate and persist per-song workflow intent and UI layout.
- Access class: `mutation`
- Existing APIs inspected: `set_song_analysis_profile`, `save_config`
- Existing implementation functions inspected: `song_analysis_profile_set`, `AppConfig::save`
- Decision: Add
- Why reuse is insufficient: Topology revision/layout is distinct from global config and analysis parameter inheritance.
- Backward compatibility: Versioned JSON; save increments revision and preserves unknown future data only through explicit schema migration.
- Artifact/source-media safety: Writes only the app SQLite database; never source media, model directories, or artifacts.
- UI command mapping: `WorkflowCommand::Save` through `UiCommand`.
- `API_CAPABILITIES` change: Add one unique `mutation` entry.
- `ui_interaction_capabilities` change: Add typed save command.
- i18n/error path: Validation messages are returned rather than debug enums.
- Final tests: Invalid type/cycle rejected; isolated SQLite roundtrip.
- Files touched: `app-core/src/workflow/*`, `app-core/src/library_db/*`, `app-core/src/api.rs`.

## API-004 — `preview_workflow_compile`

- Date: 2026-08-22
- Phase: 2
- Area: workflow
- Requested behavior: Validate editing-time typed ports/cycles and project valid intent into the exact compiled DAG/snapshot.
- Access class: `read`
- Existing APIs inspected: `preview_analysis_plan`, `get_analysis_graph`
- Existing implementation functions inspected: `build_plan`, `AnalysisGraphSpec::validate`
- Decision: Add
- Why reuse is insufficient: `preview_analysis_plan` requires an existing `AnalysisRequest`; editing validation occurs before that request and must report typed-port errors.
- Backward compatibility: Additive; compiled graph continues to use `AnalysisGraphSpec`.
- Artifact/source-media safety: Pure computation.
- UI command mapping: Typed Processing Studio compile-preview action.
- `API_CAPABILITIES` change: Add one unique `read` entry.
- `ui_interaction_capabilities` change: Add preview action.
- i18n/error path: Returns structured user-facing issues.
- Final tests: Duplicate instances, invalid ports/types, cycles, hard dependencies, deterministic digest.
- Files touched: `app-core/src/workflow/*`, `app-core/src/api.rs`.
