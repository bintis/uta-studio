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
| `list_workflow_capabilities` | read | `get_analysis_graph`, `list_audio_models` | Capability registry is not compiled graph or model catalog | Pending audit |
| `load_song_workflow` | read | `get_song_analysis_profile`, `load_config` | Workflow topology is not a parameter profile | Pending audit |
| `save_song_workflow` | mutation | `set_song_analysis_profile`, `save_config` | Per-song topology/preset selection has different semantics | Pending audit |
| `preview_workflow_compile` | read | `preview_analysis_plan` | Editing-time typed-port/cycle validation happens before AnalysisRequest exists | Pending audit |

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
