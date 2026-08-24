# Uta! Studio Editor Integration — 最终定稿

**文档版本：** v1.0
**日期：** 2026-08-22
**状态：** Supporting design / Approved under current architecture
**代码审计基线：** `bintis/uta-studio@native-inference`
**架构权威：** `docs/design/architecture/UTA_SEPARATED_ARCHITECTURE_DESIGN_v1.0.md`
**音频分析权威：** `docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md`
**产品集成补充：** `docs/design/integration/UTA_STUDIO_ANALYSIS_RUNTIME_INTEGRATION_DESIGN_v1.0.md`

---

# 1. 结论

现有 Editor 必须保留。

它不只是一个结果查看器，而已经是 Uta! Studio 最成熟的人工 authoring 子系统之一。

未来架构中：

```text
Processing Studio
    = machine workflow authoring

Editor
    = human chart authoring
```

两者通过 Artifact Revision、Evidence 与 Candidate/Authored revision 关系连接。

不得把两者合并成一个巨大页面，也不得让 Processing Studio 取代 Editor。

---

# 2. 当前代码审计

## 2.1 Core 与 UI 已经正确分层

当前：

```text
app-core/src/editor/
    UI-agnostic domain

desktop/src/studio/editor/
    Bevy UI / input / rendering / audition
```

`desktop/src/studio/editor/mod.rs` 明确把桌面 Editor 定义为建立在 `app_core::EditorDocument` 上的 UI-facing layer。

这层边界应该长期保留。

---

## 2.2 EditorDocument 是 authored truth

当前 `EditorDocument`：

- owns `VocalChartV1`；
- 提供 flattened note/lyric view；
- 所有 chart mutation 经过 document；
- drag 中允许暂时非法 overlap；
- Problems 最终阻止非法保存。

这是一套成熟的 document-editor 模式。

禁止未来直接把模型 Evidence 数据结构变成 EditorDocument。

---

# 3. 当前 Editor 已有能力清单

## 3.1 Track

已经支持：

```text
Lead
Harmony
Backing
Adlib
```

并支持：

- singer；
- scoring enable；
- multiple lead → duet parts；
- coverage bar；
- move selection between tracks。

## 3.2 Notes

已经支持：

- create；
- delete；
- move；
- resize；
- semitone / octave transpose；
- split；
- merge；
- quantize；
- duplicate；
- copy/cut/paste；
- Normal/Golden/Freestyle/Rap/GoldenRap。

## 3.3 Lyrics

已经支持：

- inline lyric edit；
- all-song lyrics edit；
- add/delete；
- split/merge；
- syllabize；
- timing shift；
- boundary shift；
- phrase split/merge；
- bind/unbind note；
- held lyric continuation。

## 3.4 Transport / Audition

已经支持：

```text
Audio
Pitch
Mixed
```

以及：

- play selection；
- play into selection；
- play out of selection；
- play visible range；
- play note pitch；
- play note vocal。

## 3.5 References

已经支持：

- waveform；
- waveform source；
- beat grid；
- model-derived pitch contour；
- other tracks as ghost notes。

## 3.6 Quality / Safety

已经支持：

- problems report；
- lock mode；
- undo/redo；
- history；
- clipboard；
- minimum note duration；
- snap；
- global timing shift。

这些能力全部属于“必须保留”的 product surface。

---

# 4. Editor 在新架构中的输入

新 Editor open contract：

```rust
struct EditorOpenRequest {
    song_id: String,
    chart_revision: ArtifactRef,
    workflow_revision: Option<WorkflowRevisionRef>,
    evidence_bundle: Option<ArtifactRef>,
    preferred_audio_sources: EditorAudioSources,
}
```

其中：

```text
chart_revision
  CandidateChart | AuthoredChart
```

`workflow_revision` 用于 provenance 和 stale detection。

`evidence_bundle` 用于只读模型证据。

`preferred_audio_sources` 只是初始 UI 偏好，不影响 chart 内容。

---

# 5. EditorSourceContext

当前 NativeEditor 只有一个 `artifact_source` 已经能追踪 Candidate/Authored revision。

目标扩展：

```rust
struct EditorSourceContext {
    opened_chart: ArtifactRef,
    workflow_revision: Option<String>,
    run_id: Option<i64>,
    evidence_bundle: Option<ArtifactRef>,
    audio_artifacts: Vec<EditorAudioArtifact>,
    newer_candidate: Option<ArtifactRef>,
}
```

这应成为 Editor 与 Processing Studio 的桥。

---

# 6. Audio Artifact Picker

当前播放/波形 source 主要使用：

```text
original
vocals
instrumental
```

目标改为 ArtifactRevision 选择。

每个选项：

```rust
struct EditorAudioArtifact {
    revision: ArtifactRef,
    role: AudioRole,
    label: String,
    producer: WorkflowNodeId,
    model_id: Option<ModelId>,
}
```

UI：

```text
Playback     [ Final BGM               ▼ ]
Waveform     [ Clean Lead Vocal        ▼ ]
Compare B    [ Lead Vocal before clean ▼ ]
```

需要支持：

- A/B；
- solo；
- temporary switch；
- preserve playhead。

---

# 7. Evidence Bundle

Canonical analysis 不应往 EditorState 塞几十个平行数组。

建议统一：

```rust
struct SingingEvidenceBundle {
    timeline_step_ms: u32,
    f0_sources: Vec<F0EvidenceTrack>,
    boundary_sources: Vec<BoundaryEvidenceTrack>,
    lyric_boundaries: Vec<BoundaryEvidenceTrack>,
    technique_tracks: Vec<TechniqueEvidenceTrack>,
    acoustic_tracks: AcousticEvidence,
    fused_confidence: Vec<ConfidenceFrame>,
    disagreement_regions: Vec<ReviewRegion>,
}
```

该 Bundle 只读。

EditorDocument 不引用模型内部 score。

---

# 8. Evidence Rendering

## 默认开启

```text
Fused confidence
Fused F0
Disagreement regions
Canonical word boundaries
```

## 默认关闭

```text
RMVPE raw
FCPE raw
GAME raw
Basic Pitch onset
STARS raw
FireRed timestamp
Qwen timestamp
DSP raw
```

高级用户可以逐项开启。

视觉优先级：

```text
Authored notes           strongest
Lyrics                   strong
Candidate/suggestions    medium
Evidence                 weak
Grid/waveform            background
```

模型证据不能盖过用户内容。

---

# 9. Review Region

```rust
struct ReviewRegion {
    id: String,
    start: f64,
    end: f64,
    severity: ReviewSeverity,
    reasons: Vec<ReviewReason>,
    confidence: f32,
    evidence_refs: Vec<EvidenceRef>,
    reviewed: bool,
}
```

Reason 示例：

```text
PitchDisagreement
BoundaryDisagreement
OctaveRisk
LyricBoundaryLowConfidence
WordNoteMismatch
VoicingConflict
LeadHarmonyLeak
TechniqueAmbiguous
```

Editor 提供 Next/Previous。

---

# 10. Suggestion Layer

模型 Suggestions 与 Problems 不应混成一类。

Problems：

> chart 自身可能非法或不适合导出。

Suggestions：

> 模型认为用户可能想改这里。

```rust
enum EditorSuggestionKind {
    ChangePitch,
    MoveBoundary,
    SplitNote,
    MergeNotes,
    BindLyric,
    MoveLyricBoundary,
    ChangeTrackRole,
    MarkTechnique,
}
```

Suggestion 可接受/忽略。

接受后调用现有 action system，进入 undo history。

---

# 11. Problems 分层

目标 UI：

```text
Checks
├─ Chart Errors
├─ Chart Warnings
└─ Analysis Review
```

不要把：

```text
overlapping notes
```

和：

```text
RMVPE / GAME disagreement
```

都称作 Error。

前者可能阻止保存。

后者不阻止保存，只建议人工检查。

---

# 12. Multi-track 与 Harmony

新分析链可以直接生成：

```text
Lead candidate
Harmony candidate
Backing candidate
Adlib candidate
```

这些映射到现有 TrackRole。

建议默认：

| candidate | role | scoring |
|---|---|---|
| lead | Lead | on |
| duet second singer | Lead | on |
| harmony | Harmony | off |
| backing | Backing | off |
| adlib | Adlib | off |

用户可以更改。

---

# 13. Technique 在 Editor 中的表现

Technique 不应该改变 NoteKind 的现有 UltraStar 语义。

Technique 是独立 annotation：

```text
vibrato
glissando
falsetto
breathy
ornament
```

可以显示在 note 下方或 Inspector 中。

不要把：

```text
vibrato
```

错误映射成：

```text
Golden note
```

两个概念完全独立。

---

# 14. Candidate 与 Authored 绝不能混淆

状态机：

```text
Candidate
   │ open
   ▼
Working Copy
   │ save
   ▼
Authored
```

重新分析：

```text
Authored A1   +   Candidate C2
```

不自动合并。

用户明确选择：

```text
Compare
Merge
Replace working copy
Ignore
```

---

# 15. Existing Revision Merge 应继续利用

当前桌面 Editor 已有：

```text
start_editor_revision_load_job
start_editor_merge_load_job
```

所以未来不需要另建一套 merge UI backend。

应该扩展现有 merge path，使它能带上：

- Workflow provenance；
- EvidenceBundle；
- suggestion diff；
- review-region mapping。

---

# 16. HumanCorrection Event

每次重要的人工修正可产生结构化 event。

重点不是记录鼠标每个像素移动，而是记录语义差异。

例如：

```text
Candidate:
A#4 12.31–13.40

Human:
A4 12.31–13.40
Reason:
vibrato_not_pitch_change
```

或：

```text
Candidate:
one note

Human:
four-note melisma
```

这些是未来训练 FusionMetaModel 最有价值的数据。

---

# 17. Save 行为

Save：

1. Validate EditorDocument；
2. 写 AuthoredChart；
3. capture immutable ArtifactRevision；
4. 保存 parent Candidate/Authored provenance；
5. optionally compute semantic HumanCorrection diff；
6. 更新 Processing Studio final node status；
7. 不触发自动重新分析。

---

# 18. Upstream Workflow Change

如果 Workflow 修改导致 candidate stale：

Editor 不关闭，不丢内容。

显示：

```text
Analysis changed upstream.
Your authored chart is safe.

[See new candidate]
```

并允许：

```text
Compare
```

---

# 19. Processing Studio → Editor Actions

Processing Studio：

### Audio Artifact
- Preview
- Use as editor playback
- Use as waveform
- A/B compare

### Analyzer Evidence
- Inspect in Editor
- Jump to conflicts
- Show layer

### CandidateChart
- Open in Editor
- Compare with authored
- Open immutable revision

---

# 20. Editor → Processing Studio Actions

Editor 可以提供：

```text
View source workflow
View artifact lineage
Re-run analyzer for this region
Re-run workflow downstream
```

“Re-run this region”如果 backend 不支持局部执行，可以降级为提示/全节点 rerun。

任何 rerun 都不能自动套用新结果到 authored notes。

---

# 21. Advanced Graph → Editor

Advanced Graph 选择：

```text
CandidateChart
AuthoredChart
EvidenceBundle
AudioStem
```

可直接：

```text
Open / Inspect in Editor
```

这样 Graph 继续是诊断入口，而不是 dead-end 技术页。

---

# 22. 推荐模块边界

## app-core

```text
workflow/
analysis_graph/
analysis_artifact/
editor/
    document/
    actions
    evidence
    suggestions
    corrections
```

## desktop

```text
studio/
    processing_studio/
    analysis/
    editor/
        view/
        evidence
        suggestions
        review_queue
        artifact_sources
```

Editor 不依赖 native inference runtime。

Editor 只读 Artifact/Evidence。

---

# 23. 测试要求

必须增加：

- Candidate revision opens in Editor；
- Authored revision opens without analyzer rerun；
- Workflow change never overwrites authored；
- merge keeps human edits according to merge mode；
- Evidence layer toggle never mutates document revision；
- suggestion accept is undoable；
- suggestion ignore does not mutate chart；
- A/B audio source preserves chart；
- Harmony candidate imports to Harmony role；
- second Lead becomes duet part；
- low-confidence review queue jumps to correct time；
- human correction diff is deterministic；
- old charts without EvidenceBundle still open；
- old `original/vocals/instrumental` sources remain migration-compatible。

---

# 24. 最终原则

Editor 长期维护以下边界：

> Machine evidence can advise.  
> Candidate data can be regenerated.  
> Human-authored chart is never silently overwritten.

中文表达：

> **模型负责提出最好的解释；编辑器负责让用户拥有最后决定权。**
