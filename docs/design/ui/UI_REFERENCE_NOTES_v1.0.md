# Uta Studio UI Reference Notes — v1.0 FINAL

本目录中的 PNG 是 Processing Studio 的**信息架构/视觉方向参考**，不是当前程序截图，也不是像素级实现规范。

真正的产品行为以：

1. `docs/design/architecture/UTA_SEPARATED_ARCHITECTURE_DESIGN_v1.0.md`
2. `docs/design/integration/UTA_STUDIO_ANALYSIS_SETTINGS_MODEL_SELECTION_EXECUTION_UX_DESIGN_v1.0.md`
3. `docs/design/editor/UTA_STUDIO_EDITOR_INTEGRATION_DESIGN_v1.0.md`
4. 当前 `desktop/src/studio/**` 行为

为准。

---

# 1. 三张图如何使用

## `processing-studio-dark.png`

适合参考：

- Processing Studio 主 canvas；
- Vocal/BGM lane；
- analyzer attachment；
- node inspector；
- Processing / Advanced Graph / Results 顶部结构。

需要按最终设计补：

```text
Processing | Graph | Editor | Results
```

Editor 必须成为一等入口。

## `processing-studio-light.png`

适合参考：

- Audio Workflow / Analysis / Fusion 三段式用户心智；
- Model Picker；
- Fast/Balanced/Maximum；
- Valid/Warning/Cached/Missing 模型状态；
- Analyzer attach 到特定 artifact。

不要照搬图里的旧/示例模型名；模型列表以 final architecture + catalog 为准。

## `workflow-lanes-dark.png`

最适合参考：

- drag node；
- Vocal/BGM branch；
- Lead/Harmony split；
- optional expert；
- inspector；
- compiled workflow status。

但最终 Graph 不是靠 canvas 坐标执行，仍由 Workflow Compiler 生成。

---

# 2. Editor 的 UI 方向

现有 Editor 不重画成 Processing Studio node canvas。

保留当前：

```text
Track strip
Transport / waveform
Piano-roll timeline
Lyrics
Notes
Inspector
Problems
Undo/redo
Audition
```

增强区域：

```text
┌──────────────────────────────────────────────────────────────┐
│ Processing | Graph | Editor | Results                       │
├──────────────────────────────────────────────────────────────┤
│ Track strip: LEAD | HARMONY | BACKING | ADLIB               │
├──────────────────────────────────────────────────────────────┤
│ Playback [Final BGM ▼]  Waveform [Clean Lead ▼]  A/B [..]   │
│ Evidence [Fused F0, GAME, Qwen...]   Review [7 issues]       │
├──────────────────────────────────────────────────────────────┤
│ AUDIO WAVEFORM                                               │
│ ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~ │
│                                                              │
│ F0 / boundary evidence                                      │
│ ───── fused F0 ─────────── │ GAME boundary                  │
│                                                              │
│ PIANO ROLL                                                   │
│ C5 |             ███████                                    │
│ B4 |       █████                 █████                       │
│ A4 | █████                                                  │
│                                                              │
│ Lyrics      こ  の  う  た ...                               │
│                                                              │
│ Review region    [ low confidence / octave risk ]            │
├──────────────────────────────────────────────────────────────┤
│ Inspector: authored note + suggestion + evidence             │
└──────────────────────────────────────────────────────────────┘
```

Visual priority：

```text
Authored notes
> lyrics
> accepted candidate/suggestion
> evidence
> grid/waveform
```

Evidence 永远只读，不覆盖用户内容。

---

# 3. 不要照图实现的东西

- 不要让普通用户选择 GGML commit。
- 不要加 CPU 自动 fallback UI。
- 不要把 Advanced Graph 变成第二个可拖 workflow。
- 不要把 Editor 缩成 Processing Studio 的小 panel。
- 不要复制一个新的 Model Manager。
- 不要使用图中的示例模型名替代最终 model registry。
- 不要把 runtime badge 当用户配置控件；普通用户只看解析结果。
