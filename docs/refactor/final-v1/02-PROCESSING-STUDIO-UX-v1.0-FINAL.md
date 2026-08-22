# Uta Studio Processing Studio 用户交互设计 — 最终定稿

**文档版本：** v1.0  
**状态：** FINAL / Approved  
**文档类型：** 用户工作流编辑器与 DAG 投影设计  
**配套文档：** `01-AUDIO-PROCESSING-ARCHITECTURE-v1.0-FINAL.md`  
**适用范围：** 音频分离、后处理、歌词转写、对齐、音高/音符分析、证据融合与最终 Canonical Singing Track 生成

---

## 1. 文档目的

Uta Studio 内部已经采用 DAG 表达音频分析依赖、Artifact 流转、缓存、重试和执行关系。

DAG 适合作为：

- 系统内部执行真相；
- 调试与诊断视图；
- Artifact lineage / provenance 展示；
- 缓存命中、失败传播和重跑依据。

但是，直接让普通用户操作完整 DAG 会暴露过多工程概念，例如：

- edge；
- artifact kind；
- cache lineage；
- freeze / bypass；
- executor；
- runtime；
- topology；
- dependency propagation。

这些概念对开发、诊断有价值，但不适合作为主要工作界面。

因此 Uta Studio 引入新的主工作页面：

# **Processing Studio**

Processing Studio 不取代 DAG，而是作为用户编辑音频处理 Workflow 的主界面。

用户在 Processing Studio 中表达的是：

> “我要对哪一版声音做什么处理、用什么模型、处理到什么程度、哪些分析器观察哪一个 Artifact，以及哪些步骤优先执行。”

系统再把这些用户意图编译为合法的 Analysis DAG。

---

# 2. 核心设计原则

## 2.1 DAG 保留，但不再要求用户直接操作固定 DAG

旧思路：

```text
Fixed Analysis DAG
        ↓
User operates DAG directly
        ↓
Execution
```

新思路：

```text
Node Capability Registry
        +
User Workflow Definition
        ↓
Workflow Compiler
        ↓
Compiled Analysis DAG
        ↓
Execution Planner
        ↓
Scheduler / Runtime
```

因此系统拥有的不是一张永久固定的业务 DAG，而是：

1. 一套稳定的 Node Capability；
2. 一套稳定的 Artifact Type；
3. 用户定义的 Workflow；
4. 编译得到的合法 DAG。

---

## 2.2 用户编辑的是 Workflow，不是底层执行器

用户可以决定：

- Vocal 用哪个模型分离；
- BGM 用哪个模型分离；
- 是否需要 Harmony Separation；
- Denoise / Dereverb / Harmony 谁先谁后；
- Vocal 是否后处理；
- BGM 是否后处理；
- 后处理几次；
- 哪一版 Vocal 提供给 ASR；
- 哪一版 Vocal 提供给 Pitch；
- 哪些高成本 Expert 只在 disagreement windows 执行；
- Fast / Balanced / Maximum 质量模式。

用户默认不需要决定：

- OpenVINO 还是 Vulkan；
- worker executable；
- GGML revision；
- cache key；
- tensor format；
- backend device id。

Runtime 由系统按照 Runtime Policy 自动解析。

---

# 3. Processing Studio 的用户心智模型

页面不应该让用户思考：

> “我正在编辑一个有 37 个 edge 的 DAG。”

页面应该让用户思考：

> “我正在搭建一条声音处理流水线。”

主要对象只有四类：

```text
Audio
Processing Node
Analyzer
Final Output
```

推荐视觉结构：

```text
SOURCE
  │
  ▼
AUDIO TRANSFORMATION
  │
  ├─ Vocal Lane
  ├─ BGM Lane
  └─ Harmony / Other Stem Lane
  │
  ▼
ANALYSIS ATTACHMENTS
  │
  ▼
FUSION / FINALIZATION
```

---

# 4. 页面信息架构

Processing Studio 分为三个逻辑区域。

## 4.1 Audio Workflow

自由度最高。

负责所有会产生新音频 Artifact 的处理：

- Vocal / BGM Separation
- Lead / Back Vocal Separation
- Denoise
- Dereverb
- Stem Refinement
- Enhancement
- Ensemble
- Audio Normalization
- 未来其它音频 Transformation

这些 Node 的顺序会真正改变数据流。

---

## 4.2 Analysis

中等自由度。

分析器不一定产生新音频，而是“观察”某一个 Audio Artifact：

- FireRedASR2
- Qwen3-ASR
- Qwen3 Forced Aligner
- RMVPE
- FCPE
- GAME
- STARS
- Basic Pitch
- VocalParse
- Acoustic DSP
- Technique Expert

关键交互不是“它在列表里的第几个”，而是：

> **它分析的是哪一个 Artifact？**

---

## 4.3 Fusion / Finalization

自由度最低。

负责把不同 Evidence 汇总成系统语义：

- Transcript Fusion
- Canonical Lyrics
- Forced Alignment / Boundary Fusion
- Evidence Calibration
- Candidate Graph
- HSMM / Viterbi
- Canonical Singing Track

这一层存在较强的语义依赖，不允许用户任意破坏。

---

# 5. Audio Workflow：真正可拖动的数据流

这是 Processing Studio 最重要的区域。

例如一个用户可以建立：

```text
Original Mix
     ↓
Vocal / BGM Separation
     ├───────────────┐
     ▼               ▼
   Vocal           Instrumental
     │               │
     ▼               ▼
 Dereverb          Denoise
     │               │
     ▼               ▼
Harmony Split     Final BGM
  ┌────┴────┐
  ▼         ▼
Lead       Back
  │
  ▼
Denoise
```

另一个用户可以选择：

```text
Vocal
  ↓
Denoise
  ↓
Harmony Split
  ├─ Lead
  └─ Back
```

也可以：

```text
Vocal
  ↓
Harmony Split
  ├─ Lead → Dereverb → Denoise
  └─ Back → Denoise
```

系统不应在业务代码中枚举这些排列组合。

系统只应验证：

```text
previous.output_type
        compatible with
next.input_type
```

---

# 6. Node Capability Contract

每个 Processing Node 只声明：

- 它是什么 Operation；
- 它接受哪些 Artifact Role；
- 它产生哪些 Artifact Role；
- 是否保持输入 Role；
- 是否允许重复实例；
- 支持哪些模型；
- 模型支持哪些 Runtime；
- 是否存在 hard dependency；
- 是否允许被 Analyzer 消费。

例如：

```text
Denoise
accepts:
  Mix
  Vocal
  LeadVocal
  BackVocal
  Instrumental

produces:
  same semantic role
```

```text
HarmonySplit
accepts:
  Vocal

produces:
  LeadVocal
  BackVocal
```

```text
VocalBgmSeparation
accepts:
  SourceMix

produces:
  Vocal
  Instrumental
```

因此，Processing Studio 的自由来自类型系统，而不是来自无限的 `if/else`。

---

# 7. Node Instance 与 Node Type 必须分离

一个 Node Type 可以出现多次。

例如：

```text
Vocal
  ↓
Denoise #1
  ↓
Harmony Split
  ↓
Lead
  ↓
Denoise #2
```

因此禁止把：

```text
AnalysisNodeId::Denoise
```

理解成全 Workflow 唯一节点。

推荐内部数据模型：

```rust
struct WorkflowNode {
    instance_id: WorkflowNodeId,
    node_type: NodeType,
    model_id: ModelId,
    params: NodeParams,
    execution_policy: ExecutionPolicy,
}
```

Node Type 决定能力。

Node Instance 决定这一次具体 Workflow 中的配置。

---

# 8. Node 的视觉设计

默认 Node 使用横向长方形卡片。

示例：

```text
┌────────────────────────────────────────────────────┐
│ ≡  Continuous Pitch                     ● Ready    │
│                                                    │
│ RMVPE                                              │
│ OpenVINO · Intel GPU                              │
│                                                    │
│ Lead Vocal → Continuous F0                        │
│                                                    │
│ [ Model ▼ ]                        [ Advanced ]    │
└────────────────────────────────────────────────────┘
```

默认层只展示：

- 操作名称；
- 当前模型；
- 当前输入；
- 输出类型；
- Ready / Running / Cached / Failed；
- Runtime 解析结果；
- 必要的模型状态。

不要默认展示：

- tensor 参数；
- cache signature；
- worker path；
- GGML commit；
- OpenVINO properties；
- debug flags。

这些放入 Advanced / Developer Mode。

---

# 9. Model Selector

模型选择器是 Processing Studio 的核心交互之一。

示例：

```text
Vocal Separation

Model
┌──────────────────────────────────┐
│ BS RoFormer 124-band      Best   │
│ BS RoFormer EP317         Stable │
│ MelBand Inst V2           Legacy │
└──────────────────────────────────┘
```

模型项建议展示：

- Model Name
- Purpose
- Quality Tier
- Benchmark status
- Production validation status
- Downloaded / Missing
- Model size
- Runtime availability

状态语义必须严格区分：

```text
BenchmarkCandidate
ProductionPinned
Experimental
Legacy
Unavailable
```

排行榜冠军不能因为分数高就自动显示成 Production Ready。

---

# 10. Runtime UI

普通用户不直接选择 Runtime。

系统按照正式政策：

```text
OpenVINO
   ↓ unavailable / parity failed
Vulkan
   ↓ unavailable / unvalidated
Fail Closed
```

Node 默认只显示解析结果：

```text
RMVPE
OpenVINO · Intel Arc
```

或：

```text
Qwen3-ASR-1.7B
Vulkan · Intel Arc
```

Advanced / Developer Mode 才允许：

- Pin Runtime
- Pin Device
- Force diagnostic CPU
- 查看 runtime validation evidence

CPU 不是普通生产 fallback。

Python 永远不是 production fallback。

---

# 11. Vocal Lane 与 BGM Lane

Vocal / BGM 分离之后必须明确产生独立 Lane。

```text
                    SOURCE
                      │
              Vocal / BGM Split
                      │
        ┌─────────────┴─────────────┐
        │                           │
    VOCAL LANE                  BGM LANE
        │                           │
   processing                   processing
        │                           │
        ▼                           ▼
 Clean Vocal                   Final BGM
```

原因：

- Vocal 与 BGM 的后处理需求不同；
- 两边可选择不同模型；
- 两边可以拥有不同数量的后处理节点；
- Vocal 可能继续拆 Lead / Harmony；
- BGM 可以单独进行 instrumental refinement；
- 缓存与 provenance 必须独立。

---

# 12. Harmony / Lead / Back Vocal

Harmony Separation 不是简单“清理步骤”，而是可能改变后续 Pitch 分析输入的重要节点。

示例：

```text
Vocal
  ↓
Lead / Back Separation
  ├─ Lead Vocal
  └─ Back Vocal
```

默认情况下：

```text
Lead Vocal
→ ASR
→ F0
→ Note Analysis
```

Back Vocal 不应立即丢弃。

它应保留为独立 Artifact，以支持未来：

- duet；
- backing harmony；
- octave double；
- choir region；
- harmony notes；
- 多声部 Karaoke。

---

# 13. Analyzer Attachment

Analyzer 不应被强制放在音频 Lane 的末尾。

用户应该能够指定：

> “我要分析哪一版声音。”

例如：

```text
Vocal
  ↓
Denoise
  ├────────────→ FireRed ASR
  │
  ↓
Dereverb
  ↓
Harmony Split
  ↓
Lead
  ├────────────→ RMVPE
  ├────────────→ GAME
  └────────────→ DSP
```

这意味着：

- ASR 最佳输入不一定与 F0 最佳输入相同；
- 用户可以保留多个 Vocal Artifact；
- Analyzer 与 Audio Transformation 是 attachment，而不是简单串行 Stage。

推荐 UI：

```text
┌────────────────────────────────────┐
│ Clean Lead Vocal                   │
│                                    │
│ Consumers                          │
│ ├ RMVPE                            │
│ ├ GAME                             │
│ ├ Acoustic DSP                     │
│ └ STARS                            │
│                                    │
│ [+ Add analysis]                   │
└────────────────────────────────────┘
```

---

# 14. 四种关系必须在 UI 和代码中分开

## 14.1 Hard Dependency

必须满足，用户不能拖坏。

例如：

```text
Canonical Lyrics
      ↓
Qwen Forced Alignment
```

没有 Canonical Lyrics，Forced Alignment 不允许执行。

---

## 14.2 Dataflow Dependency

由用户决定。

例如：

```text
Denoise
Dereverb
Harmony Split
```

只要输入/输出类型兼容，允许用户改变顺序。

---

## 14.3 Priority

两个已经 Ready 的独立节点谁先获得资源。

例如：

```text
RMVPE
GAME
FireRed
```

用户拖动 priority 不应创造新的 dependency edge。

---

## 14.4 Conditional Execution

节点仅在条件满足时运行。

例如：

```text
Basic Pitch
Execution:
  Always
  Only on disagreement
  Disabled
```

或：

```text
STARS
Maximum Quality only
```

---

# 15. Drag & Drop 语义

拖动不是单一行为。

系统必须根据节点所在区域决定拖动含义。

### Audio Transformation 区域

拖动可能改变真实数据流。

```text
Vocal → Denoise → Dereverb
```

拖成：

```text
Vocal → Dereverb → Denoise
```

应重新编译 Workflow DAG。

---

### Parallel Analysis 区域

拖动主要改变 Priority。

例如：

```text
RMVPE
GAME
FireRed
DSP
```

改变显示顺序不应隐式创造：

```text
RMVPE → GAME → FireRed
```

---

### Hard Dependency 区域

非法拖动必须阻止。

例如把 Forced Aligner 放到 Transcript Fusion 之前。

反馈：

```text
Qwen Forced Alignment requires Canonical Lyrics.
```

然后 Node 回到最近合法位置。

---

# 16. Workflow Compiler

Processing Studio 不直接把 UI 坐标交给 Scheduler。

保存的是语义 Workflow。

编译流程：

```text
User Workflow
     ↓
Resolve Node Instances
     ↓
Validate Artifact Types
     ↓
Validate Hard Dependencies
     ↓
Cycle Detection
     ↓
Resolve Conditional Nodes
     ↓
Build Artifact Edges
     ↓
Compile Analysis DAG
     ↓
Resolve Runtime
     ↓
Build Execution Plan
```

如果编译失败，用户必须得到可理解的错误。

例如：

```text
Harmony Split requires Vocal input.
Current input is Instrumental.
```

而不是：

```text
ArtifactKind mismatch at edge 27.
```

---

# 17. Workflow 数据模型建议

```rust
struct WorkflowDefinition {
    workflow_id: WorkflowId,
    version: u32,
    nodes: Vec<WorkflowNode>,
    edges: Vec<WorkflowEdge>,
    analyzer_bindings: Vec<AnalyzerBinding>,
    quality_mode: QualityMode,
}

struct WorkflowNode {
    instance_id: WorkflowNodeId,
    node_type: NodeType,
    model_id: ModelId,
    params: NodeParams,
    execution_policy: ExecutionPolicy,
    priority: i32,
}

struct WorkflowEdge {
    from: WorkflowPort,
    to: WorkflowPort,
}

struct AnalyzerBinding {
    analyzer_node: WorkflowNodeId,
    source_artifact: WorkflowPort,
}
```

UI position可以单独保存：

```rust
struct WorkflowLayout {
    positions: HashMap<WorkflowNodeId, NodePosition>,
}
```

但是：

> **WorkflowLayout 永远不能成为 execution truth。**

---

# 18. Scheduler 语义

最终 Scheduler 使用：

```text
Compiled DAG dependencies
        +
Workflow priority
        +
Artifact cache
        +
Execution policy
        +
Runtime availability
        +
Resource budget
        ↓
Ready Set
        ↓
Dispatch
```

例如：

```text
READY
RMVPE      priority 100   OpenVINO
GAME       priority 90    OpenVINO
FireRed    priority 80    OpenVINO
Qwen ASR   priority 70    Vulkan
```

Scheduler 根据资源决定真实 dispatch。

Priority 不等于串行约束。

---

# 19. Conditional Expert UX

Processing Studio 要直接表达 Fast / Balanced / Maximum。

例如：

```text
Basic Pitch

Execution
○ Always
● Only on disagreement
○ Disabled
```

```text
VocalParse

Execution
● Maximum only
```

```text
STARS

Execution
● Disagreement windows
```

用户无需理解 `conditional DAG expansion`。

---

# 20. Preset 与自由编辑

完全自由的 Workflow 对高级用户有价值，但新用户需要起点。

因此建议提供：

- Fast
- Balanced
- Maximum
- Custom

Preset 创建的是一个 WorkflowDefinition。

选择 Preset 后仍然可以编辑。

一旦编辑：

```text
Balanced
→ Balanced · Modified
```

用户可以：

```text
Save as Preset
```

但系统必须保留官方推荐 Preset，以便：

- support；
- bug reproduction；
- benchmark；
- regression testing。

---

# 21. 默认 Workflow

默认 Workflow 应体现高质量但不过度复杂。

示意：

```text
Source
  ↓
Vocal/BGM Separation
  ├────────────────────────────┐
  │                            │
Vocal                        BGM
  │                            │
Lead/Harmony                  │
  ├─ Lead                     │
  └─ Back                     │
  │                            │
Lead optional cleanup       optional cleanup
  │                            │
  ├─ FireRed / Qwen ASR       └─ Final BGM
  ├─ RMVPE
  ├─ GAME
  └─ DSP
       ↓
Transcript Fusion
       ↓
Forced Alignment
       ↓
Evidence Fusion
       ↓
HSMM / Viterbi
       ↓
Canonical Singing Track
```

该 Workflow 只是默认，不是永久固定逻辑。

---

# 22. Validation UX

Workflow 必须实时显示合法性。

推荐状态：

```text
Valid
Warning
Invalid
Missing Model
Runtime Unavailable
Waiting for Input
```

### Invalid

禁止运行。

例如：

```text
Harmony Split has no Vocal input.
```

### Warning

允许运行。

例如：

```text
Denoise is applied twice to the same lineage.
This may remove high-frequency vocal detail.
```

### Missing Model

Node 保留，但不能 Ready。

提供：

```text
Download model
Choose another model
Disable node
```

---

# 23. 允许重复后处理，但提供语义警告

架构不禁止：

```text
Denoise → Denoise
Dereverb → Dereverb
```

因为用户可能有合理需求。

但是可以给软提示：

```text
Repeated denoise may remove consonants or breath detail.
```

这是 warning，不是 hard error。

原则：

> 类型系统负责阻止不可能的流程；UX warning 负责提醒可能不理想的流程。

---

# 24. Cache 与 Re-run

Node 应显示：

```text
Cached
Needs Run
Running
Completed
Failed
Stale
```

用户可以对单 Node：

- Run from here
- Re-run this node
- Re-run downstream
- Use cached result
- Freeze output

但这些高级操作默认放在 Node menu / Advanced。

UI 不需要让用户理解 cache signature。

---

# 25. Artifact Version Picker

当同一语义存在多个 Artifact 时，应该允许选择。

例如：

```text
Lead Vocal
├─ raw
├─ dereverb
└─ dereverb + denoise
```

Analyzer source 可以选择：

```text
RMVPE Source
[ Lead Vocal · dereverb + denoise ▼ ]
```

这比强制使用 Lane 最末端更加灵活。

---

# 26. Advanced DAG 页面继续保留

现有 Analysis Graph 页面不删除。

它的新定位是：

# **Advanced Graph / Diagnostics**

主要用途：

- 查看真实 Compiled DAG；
- 查看 Artifact lineage；
- 查看 cache edges；
- 查看 runtime；
- 查看 attempts；
- 查看 failure propagation；
- 查看 execution timeline；
- developer diagnostics。

普通用户的日常工作入口变为 Processing Studio。

---

# 27. Processing Studio 与 Advanced Graph 的双向关系

```text
Processing Studio
      ↓ compile
Analysis DAG
      ↓ execute
Run State
      ↓ project back
Processing Studio
```

Processing Studio 必须实时反映：

- Ready；
- Running；
- Cached；
- Failed；
- Disabled；
- Conditional skipped。

Advanced Graph 则显示真实 edge。

---

# 28. 用户保存的不是 DAG implementation detail

用户保存的是：

```text
Workflow Intent
```

例如：

```text
Use model X for vocal separation
Run harmony split after dereverb
Analyze this lead-vocal artifact with RMVPE
Use Basic Pitch only on disagreement
```

不要把：

- worker binary path；
- OpenVINO device string；
- graph internal id；
- cache directory；

写进用户 Workflow。

这些属于 Runtime Resolution。

---

# 29. 模型更新不应破坏 Workflow

Workflow 引用：

```text
capability + model_id
```

模型升级后：

- 如果 model_id 仍兼容，则 Workflow 不变；
- 如果模型被 deprecated，显示 migration；
- 不允许偷偷换模型后仍称结果完全相同。

ProductionPinned 与 BenchmarkCandidate 的状态更新不应改变 Workflow 拓扑。

---

# 30. 用户交互原则总结

Processing Studio 最终遵守以下原则：

1. **DAG 是系统真相，不是普通用户的工作语言。**
2. **用户可以自由设计 Audio Transformation。**
3. **自由来自 Artifact 类型系统，不来自业务代码枚举排列组合。**
4. **Vocal / BGM / Harmony 使用独立 Lane。**
5. **Analyzer 绑定到 Artifact，而不是强制绑定到流程尾部。**
6. **Audio 节点拖动可以改变真实数据流。**
7. **Parallel Analyzer 拖动主要改变优先级。**
8. **Hard Dependency 永远不能被拖动破坏。**
9. **Node Type 可以拥有多个 Node Instance。**
10. **Runtime 默认自动解析：OpenVINO → Vulkan → Fail Closed。**
11. **CPU 只用于 reference / diagnostics；Python 不作为生产 fallback。**
12. **用户选择模型，不管理底层 AI runtime。**
13. **Conditional Expert 是一等公民。**
14. **Workflow Layout 与 Execution Semantics 分离。**
15. **Processing Studio 是主工作台，Advanced DAG 是诊断工具。**

---

# 31. 最终目标

Processing Studio 应该让高级用户拥有足够自由，例如：

```text
“我希望 Vocal 先 Dereverb 再 Harmony。”
```

或者：

```text
“BGM 不去噪，但 Lead Vocal 做两次不同强度的处理。”
```

或者：

```text
“ASR 看 denoise 后的 Vocal，
Pitch 看 Harmony Split 后的 Lead。”
```

或者：

```text
“Basic Pitch 和 STARS 只处理冲突窗口。”
```

同时系统仍然保证：

- 类型安全；
- 无环；
- hard dependency 正确；
- Artifact provenance 完整；
- cache 可追踪；
- runtime 可验证；
- Scheduler 可优化；
- Canonical Singing Track 的语义稳定。

Processing Studio 的核心价值不是“隐藏 DAG”。

而是：

> **允许用户用符合音频制作直觉的方式构造 Workflow，同时由系统把这种自由编译成可验证、可缓存、可重放、可诊断的 DAG。**

---

# 32. Editor 是与 Processing Studio 并列的一等工作区

代码审计确认当前 Editor 已经拥有成熟的：

- 多轨；
- note / lyric authoring；
- waveform；
- analyzer pitch guide；
- beat grid；
- audition；
- tap-to-time；
- track role；
- problems；
- undo / redo；
- Candidate/Authored revision 工作流。

因此 Processing Studio 上线后不得把 Editor 改成“结果预览器”。

Song-level 顶部工作区建议：

```text
Processing | Graph | Editor | Results
```

用户心智：

```text
Processing = 机器怎么做
Editor     = 我怎么定稿
Results    = 最后怎么用
Graph      = 系统实际上做了什么
```

---

# 33. Processing Studio 到 Editor 的主入口

Finalization 区域必须有明显的：

```text
Canonical Singing Track
└─ Candidate ready
   [Open in Editor]
```

如果已经保存：

```text
Canonical Singing Track
└─ Authored
   [Continue Editing]
```

如果 Workflow 变化产生新 candidate：

```text
Canonical Singing Track
└─ Authored · New candidate available
   [Compare] [Keep authored]
```

---

# 34. Editor Source 不再固定为 Original / Vocals / Instrumental

Processing Studio 会产生很多 Audio Artifact。

Editor 顶部 Audio Source Picker 应列出当前 Workflow lineage：

```text
Playback
[ Final BGM ▼ ]

Waveform
[ Lead Vocal · clean ▼ ]

Reference
[ Original Mix ▼ ]
```

可选项示例：

```text
Original Mix
Vocal · raw
Vocal · denoise
Vocal · dereverb
Lead Vocal
Lead Vocal · clean
Back Vocal
Final BGM
```

选项显示 producer/model/processing chain 的简短 provenance。

---

# 35. Editor 增加 Evidence 面板

Processing Studio 的 Analysis Attachments 产生证据。

Editor 不重新跑模型，只读取已存在 Evidence Artifact。

建议 Toolbar：

```text
Evidence [▾]
Review   [7 issues]
```

Evidence 菜单：

```text
✓ Fused confidence
✓ RMVPE F0
  FCPE F0
✓ GAME boundary
  Basic Pitch onset
  Qwen word boundaries
  STARS techniques
✓ Disagreement regions
```

Timeline 中：

- F0 仍使用细线；
- boundary 用竖线/marker；
- disagreement 用浅色时间区间；
- confidence 用低干扰 heat band；
- technique 用 note 下方小 badge/underline。

不能让 Evidence 遮挡 authored notes。

---

# 36. Editor Review Queue

用户不应必须从 0:00 手工检查整首歌。

Processing Studio/Fusion 应输出 Review Items：

```text
7 unresolved
├─ 00:42.31 Possible octave error
├─ 01:15.08 GAME / onset disagreement
├─ 02:04.20 Word boundary low confidence
└─ ...
```

Editor：

```text
[Previous] [Next] [Mark reviewed]
```

点击直接 zoom 到对应时间窗。

---

# 37. Editor 建议操作必须可撤销

在 Note Inspector 中可以出现：

```text
Suggestion
GAME + RMVPE prefer A4
confidence 0.91

[Accept A4]
[Keep authored A#4]
[Inspect evidence]
```

Acceptance 必须通过现有 Editor Action / undo history。

模型不能直接修改 authored document。

---

# 38. Harmony / Backing 与现有 Track Strip

现有 Editor 已有：

```text
Lead
Harmony
Backing
Adlib
```

因此 Harmony Separation 的结果直接进入候选 track。

Processing Studio 可以决定：

```text
Generate harmony candidate track: On/Off
```

Editor Track Strip 显示：

```text
LEAD      Singer A      scored
HARMONY   Auto          reference
BACKING   Auto          reference
```

用户可以：

- change role；
- enable scoring；
- name singer；
- move selection to track；
- delete/merge candidate material。

不新增独立“和声编辑器”。

---

# 39. Workflow Node 与 Editor 的联动入口

音频 Processing Node 菜单增加：

```text
Preview artifact
Open artifact lineage
Use as Editor playback source
Use as Editor waveform source
Attach analyzer
Run downstream
```

Analysis Node 菜单增加：

```text
Inspect evidence in Editor
Open disagreement regions
Re-run this analyzer
Compare revision
```

Finalization Node 菜单增加：

```text
Open candidate in Editor
Compare with authored
Promote candidate
View provenance
```

---

# 40. Editor 不应被 Workflow 拖动语义污染

Processing Studio 的 node position 可以改变 dataflow。

Editor 的 note/lyric timeline 是内容编辑。

两者状态必须严格分开：

```text
WorkflowLayout
≠
EditorDocument
```

改变 Workflow 不能移动任何 authored note。

改变 Editor note 不能自动改变 Workflow。

两者只通过：

```text
ArtifactRevision
Evidence
Candidate/Authored relation
```

连接。

---

# 41. 用户看到的完整闭环

```text
1. Processing
   选择模型 / 拖动音频处理 / 绑定分析器

2. Run
   系统 compile + execute

3. Review
   Candidate ready
   7 uncertain regions

4. Editor
   人工只重点检查冲突区域
   同时可以完整编辑歌词、音符、多轨

5. Save
   AuthoredChart revision

6. Results
   Export UTZ / UltraStar / audio assets
```

这比“DAG → Run → Export”更符合真正的创作软件心智。

