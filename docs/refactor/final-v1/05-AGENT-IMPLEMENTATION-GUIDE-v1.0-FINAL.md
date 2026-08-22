# Uta Studio `native-inference` 全量重构 Agent 落地指导 — v1.0 FINAL

**状态：FINAL / Implementation Contract**  
**仓库：** `bintis/uta-studio`  
**目标分支：** `native-inference`  
**代码审计基线：** `56fdbec50444939360caf2832a7b1d958941fe6b` (`Refactoring in progress`)  
**最终定稿日期：** 2026-08-22  

本文件是本次大重构的**主执行文档**。Coding Agent 应按 checkbox 顺序推进，不要把历史 draft 当成新的执行计划。

---

# 0. 权威顺序

遇到冲突时按以下顺序处理：

1. 本文件；
2. `01-AUDIO-PROCESSING-ARCHITECTURE-v1.0-FINAL.md`；
3. `04-NATIVE-RUNTIME-LOCK-v1.0-FINAL.json`；
4. `02-PROCESSING-STUDIO-UX-v1.0-FINAL.md`；
5. `03-EDITOR-INTEGRATION-v1.0-FINAL.md`；
6. 仓库当前 `AGENTS.md` 与 `docs/engineering-constraints.md` 中未被本最终设计明确替代的安全/产品规则；
7. 当前代码；
8. 旧设计、旧验证记录和历史文档。

特别说明：

- 当前 `AGENTS.md` / `engineering-constraints.md` 仍描述 Python/uv 时代 runtime；**这些部分最终必须更新为 native-only**。
- 但其中 source-media read-only、显式下载、API catalogue、Editor 交互、Wayland-only、UTZ/UltraStar、一文件 2000 行等规则在整个重构期间仍然有效。
- 历史验证中的失败记录不得删除；最终支持矩阵只能追加新的验证结论。

---

# 1. 最终任务

本次重构完成后，Uta Studio 必须满足：

- [ ] 产品运行与仓库均无 Python / PyTorch / Transformers / uv / venv。
- [ ] Rust 继续作为 control plane：queue / run / retry / cancel / progress / cache / artifact / DB / API。
- [ ] 模型执行使用独立 native worker，不使用 Python TCP server。
- [ ] 用户可在 Processing Studio 构造动态音频 Workflow。
- [ ] Workflow 编译为内部 DAG；Advanced Graph 继续存在并显示真实 compiled DAG。
- [ ] Audio transformation 顺序可由用户改变，类型/依赖非法时禁止连接。
- [ ] Vocal / BGM / Lead / Back/Harmony 支持独立 lane。
- [ ] Analyzer 绑定到具体 Audio Artifact，而不是固定取“最后一个 vocal”。
- [ ] Canonical Lyrics / alignment / F0 / note / technique 由多 Expert + Fusion 产生。
- [ ] Editor 完整保留，并增强为 Candidate → Human Authored 的 Evidence Workbench。
- [ ] AuthoredChart 永远不被重新分析静默覆盖。
- [ ] Runtime/model 下载只能由用户在 Models & runtime 明确触发。
- [ ] 最终 `git ls-files '*.py' '*.pyi'` 为空。
- [ ] 最终所有 app-owned source file ≤ 2000 行。
- [ ] 最终一次性执行完整 compile/test/package/hardware verification。

---

# 2. 本次特殊工作模式：重构优先，最终才编译

## 2.1 Phase 0–14 禁止反复编译

这次范围足够大，**不要每完成一个 phase 就 cargo check/test/build**。这样会不断被中间态编译错误打断，导致为了暂时编译而写兼容层、重复 adapter 或过早收敛设计。

Phase 0–14：

**不要运行：**

```text
cargo check
cargo test
cargo clippy
cargo build
nix build
cmake --build
ctest
完整 GUI build
```

也不要为了“临时能编译”恢复旧 Python fallback。

允许且推荐的轻量检查：

```sh
git status --short
git diff --check
git diff --stat
git grep ...
wc -l ...
jq ...
rg ...
```

如果某一步需要确认某个接口签名，直接读当前源码和上游 pinned source；不要通过不断编译来探索接口。

## 2.2 Phase 15 才进入 Build/Fix Loop

所有结构改造完成、Python 删除、UI/API/i18n 收口后，再进入 Phase 15：

```text
compile → fix → compile → test → fix → package → smoke
```

Phase 15 内允许多轮修复，直到完整门槛通过。

---

# 3. 硬性工程规则

## 3.1 单文件不得超过 2000 行

适用于 app-owned：

```text
.rs
.cpp/.cc/.c
.h/.hpp
以及重构期间尚未删除的 .py
```

规则：

- [ ] 每次准备修改一个大文件，先 `wc -l`。
- [ ] >1600 行且本次还要明显增加逻辑时，优先拆模块。
- [ ] 绝对禁止在 handoff 时 >2000 行。
- [ ] `third_party/` 原样 vendor 文件可以豁免；不要为了规则手改第三方单头文件。
- [ ] 新增 app-owned 文件一开始就按职责拆分，不建立 3000 行的 `workflow.rs` 再“以后拆”。

最终 gate：

```sh
git ls-files \
  | grep -E '\.(rs|c|cc|cpp|h|hpp)$' \
  | grep -vE '(^|/)(third_party|target|result|node_modules)/' \
  | while read -r f; do
      n=$(wc -l < "$f")
      [ "$n" -le 2000 ] || printf '%6d %s\n' "$n" "$f"
    done
```

输出必须为空。对项目自有 `vendor/utz` 仍按 2000 行约束；只有真正 upstream third-party 可豁免。

## 3.2 API：复用优先，新建必须登记

每次想新增 app-owned API/command 前：

- [ ] 搜 `app-core/src/api.rs::API_CAPABILITIES`。
- [ ] 搜 `app-core/src/lib.rs` 已有公开 domain operation。
- [ ] 搜 `desktop/src/studio/commands.rs`。
- [ ] Editor 功能再搜 `app-core/src/editor/actions.rs` 与 `desktop/src/studio/editor/*`。
- [ ] 搜 `desktop/src/studio/ui_api.rs` 的现有 UI contract。
- [ ] 在 `06-API-CHANGE-LEDGER-v1.0-FINAL.md` 先写审计记录，再写代码。

决策规则：

### A. 老 API 完全满足
直接复用。**禁止新增别名 API。**

### B. 老 API 语义正确，只差很小参数/返回字段
优先向后兼容地扩展老 API。

例如：

```text
已有 inspect_artifact
只差 workflow producer metadata
→ 扩展返回结构
→ 不新建 inspect_workflow_artifact
```

### C. 老 API 只能通过扭曲语义才能复用
允许新增，但 ledger 必须写明：

- 调研过哪些旧 API；
- 为什么旧语义不能表达目标；
- 新 API 的 access class；
- UI command 映射；
- test/diagnostics 计划；
- i18n/error path。

禁止因为“名字更好看”就新建 API。

## 3.3 新 API 完成条件

每个新 command：

- [ ] 登记到 `API_CAPABILITIES`。
- [ ] access 必须是 `read | mutation | destructive | external | temporary`。
- [ ] Desktop UI 若使用，必须走 typed `UiCommand`/`UiAction` 或既有 local command boundary。
- [ ] `ui_interaction_capabilities` / API coverage 保持同步。
- [ ] destructive 操作必须明确确认。
- [ ] Artifact 操作使用 `ArtifactRef`/受控 identity，不接受任意用户 PathBuf 作为数据真相。
- [ ] 最终有 contract test。
- [ ] 错误在 UI 可见。

## 3.4 数据安全

- [ ] Source media 永远 read-only。
- [ ] 不移动/覆盖/删除用户原歌。
- [ ] Existing model directories 是用户数据；迁移不自动删除。
- [ ] Cache destructive action 必须显式确认。
- [ ] Artifact revision 保持 content-addressed immutable。
- [ ] Candidate 可再生；AuthoredChart 是 human-owned。
- [ ] Re-analysis 不能自动替换 AuthoredChart。

## 3.5 Runtime / 下载

- [ ] 启动、页面 render、diagnostics 不下载 runtime/model。
- [ ] 下载只由 Models & runtime explicit action 发起。
- [ ] 不启动 HTTP inference/control server。
- [ ] Linux desktop 保持 Wayland-only。
- [ ] CPU 不是生产自动 fallback。
- [ ] Python 绝不是 fallback。

## 3.6 UI / i18n

新用户文案同步：

```text
desktop/assets/i18n/en.json
desktop/assets/i18n/zh-CN.json
desktop/assets/i18n/ja.json
```

- [ ] 三语言 key 集合一致。
- [ ] Processing Studio 不复制 Settings 的模型安装职责。
- [ ] Advanced Graph 不重新成为主编辑入口。
- [ ] Editor 不因为 Workflow UI 新增而降级。

---

# 4. 当前代码：必须优先复用的基础

不要重写以下已经成熟的能力。

## 4.1 DAG / Plan

### `app-core/src/analysis_graph.rs`

现有：

- `AnalysisNodeId(String)`：历史兼容很好，保留。
- `AnalysisGraphSpec`
- `AnalysisNodeSpec`
- `AnalysisEdge`
- `validate()`
- `topo_order()`
- `dependencies_of()`
- `dependents_of()`

**目标：** 从“静态业务 DAG 定义”转为“compiled DAG 表示”。校验/拓扑算法继续复用。

### `app-core/src/analysis_plan.rs`

继续承担：

- target closure；
- disabled/freeze/bypass；
- cache decision；
- run state grouping。

不要另写一个平行 scheduler-plan engine。

## 4.2 Audio plan snapshot

### `app-core/src/audio_processing.rs`

已有：

- `AudioProcessingSettings`
- `AudioProcessingStep`
- `AudioInputReference::SourceMedia | StepOutput`
- `AudioOutputBinding`
- `AudioProcessingPlanSnapshot`

这是现有“简单 workflow compiler”的雏形。

**目标：** 将它迁移/泛化到 `WorkflowDefinition -> WorkflowExecutionSnapshot`，不要从零重新发明 immutable run snapshot。

## 4.3 Artifact / lineage

### `app-core/src/analysis_artifact.rs`

已有：

- `ArtifactRevision`
- producer node
- input revisions
- config hash
- algorithm version
- content hash
- immutable `ArtifactStore`

### `app-core/src/artifact_workbench/*`

已有：

- inspect
- typed preview/diff
- edit draft
- impact
- capture
- revision selection
- lineage

动态 Workflow 必须建立在这套 revision 真相上。

## 4.4 Analysis orchestration

### `app-core/src/analyzer/{queue,run,control,reanalyze}.rs`

继续复用：

- queue；
- per-run workdir；
- retry；
- cancel/stop；
- history；
- per-node attempts；
- logs；
- artifact capture；
- final DB updates。

只替换 execution plane，不重写 control plane。

## 4.5 Existing app API

重点复用现有：

```text
get_analysis_graph
preview_analysis_plan
preview_full_analysis_plan
run_analysis_plan
run_analysis_node
run_analysis_node_downstream
run_analysis_request
inspect_analysis_node_io
load_analysis_artifacts
load_artifact_revisions
inspect_artifact
preview_artifact
artifact_lineage
preview_artifact_downstream_impact
preview_node_downstream_impact
compare_artifacts_typed
merge_chart_revisions
analysis_runtime_status
list_audio_models
get_audio_model_status
install_audio_model
editor_actions
dispatch_ui_interaction
```

特别注意：

**不要新增 `run_workflow()` 只为了名字更符合新 UI。**  
如果 compiled workflow 可以转成已有 `AnalysisRequest` / `run_analysis_request`，就复用现有执行 API。

## 4.6 Desktop command system

### `desktop/src/studio/commands.rs`
### `desktop/src/studio/ui_api.rs`

已有 typed：

```text
AppCommand
LibraryCommand
SettingsCommand
AnalysisCommand
EditorCommand
UiCommand
UiAction
```

Processing Studio 新交互应进入这一体系，不另建 event-bus API。

## 4.7 Editor

### Core

```text
app-core/src/editor/*
```

已有 `EditorDocument` / actions / problems / lyrics / notes。

### Desktop

```text
desktop/src/studio/editor/*
```

已有：

- timeline；
- piano gutter；
- waveform；
- beat grid；
- analyzer pitch contour；
- multi-track；
- Lead/Harmony/Backing/Adlib；
- duet；
- tap-to-time；
- bind/unbind；
- audition；
- problems；
- undo/redo；
- Candidate/Authored revision loading/merge。

**禁止重写 Editor。** 新工作只做 bridge/evidence/review/source enhancements。

## 4.8 RoFormer native code

```text
native-inference/roformer/
```

现有 direct GGML/Vulkan helper、graph/runtime、progress/cancel、diagnostics、durable Vulkan logging。

继续 productionize，不重写为另一套 RoFormer runtime。

---

# 5. 目标目录边界

推荐新增：

```text
app-core/src/workflow/
├─ mod.rs
├─ types.rs
├─ capability.rs
├─ definition.rs
├─ compiler.rs
├─ validation.rs
├─ migration.rs
└─ snapshot.rs

app-core/src/native_runtime/
├─ mod.rs
├─ protocol.rs
├─ supervisor.rs
├─ registry.rs
├─ router.rs
├─ runtime_lock.rs
└─ worker.rs

app-core/src/singing/
├─ mod.rs
├─ evidence.rs
├─ transcript_fusion.rs
├─ alignment_fusion.rs
├─ pitch_fusion.rs
├─ review.rs
├─ hsmm.rs
└─ canonical.rs

desktop/src/studio/processing_studio/
├─ mod.rs
├─ state.rs
├─ page.rs
├─ canvas.rs
├─ lanes.rs
├─ node_card.rs
├─ inspector.rs
├─ drag.rs
├─ validation.rs
└─ actions.rs

desktop/src/studio/editor/
├─ ...existing files...
├─ evidence.rs
├─ suggestions.rs
├─ review_queue.rs
└─ artifact_sources.rs
```

Native runtime：

```text
native-inference/roformer/             existing
native-inference/openvino-worker/      generic OpenVINO host if appropriate
native-inference/qwen-asr/             transcribe.cpp integration/adapter
native-inference/qwen-align/           predict-woo integration/adapter
```

是否使用一个 generic OpenVINO worker 还是多个小 worker，优先看现有模型 loader/生命周期复用；不要为了“目录整齐”强行把互相冲突的 runtime 放进同进程。

---

# 6. Workflow Domain 的目标结构

建议：

```rust
WorkflowDefinition
WorkflowNodeId
CapabilityId
WorkflowNodeInstance
WorkflowPort
WorkflowEdge
AnalyzerBinding
ExecutionPolicy
WorkflowLayout
WorkflowExecutionSnapshot
```

关键规则：

- `WorkflowNodeId` 是 instance identity。
- `CapabilityId` 是“这个节点会做什么”。
- 同一 capability 可以出现多次。
- Layout 坐标与执行语义完全分离。
- Audio transform edge 会改变真正 dataflow。
- Analyzer attachment 代表消费某个 Artifact。
- Priority 只影响 ready-node dispatch，不创造 dependency。
- Conditional node 是一等语义。
- Hard dependency 不允许用户拖坏。

---

# 7. API 新增审计：本重构预计真正可能需要的最小集合

以下不是“直接去创建”的指令，而是**先审计旧 API 后最可能被证明必要**的 command：

```text
list_workflow_capabilities    read
load_song_workflow            read
save_song_workflow            mutation
preview_workflow_compile      read
```

原因：

- `get_song_analysis_profile` 是参数 override，不应被扭曲成拓扑存储。
- `get_analysis_graph` 返回 compiled/static graph，不等同 Node Capability registry。
- `preview_analysis_plan` 输入已经是 AnalysisRequest 语义，不能负责编辑期的 typed-port/cycle validation。

但在真正新增前仍必须：

- [ ] 搜当前 HEAD。
- [ ] 写 API ledger。
- [ ] 确认没有近期 commit 已经加入等价功能。
- [ ] 如果可以用已有 API 小扩展解决，则取消新增。

**不建议新增：**

```text
run_workflow
run_processing_studio
open_workflow_artifact
editor_open_candidate
```

优先复用：

```text
run_analysis_request
inspect_artifact
OpenArtifactCompatibleEditor / existing revision load
merge_chart_revisions
```

---

# 8. Phase 0 — 最终基线与防护栏

> 不编译。

- [ ] 确认当前 branch 是 `native-inference`。
- [ ] `git status --short`，记录现有未提交改动；不得覆盖未知用户改动。
- [ ] `git rev-parse HEAD`；若不等于 `56fdbec...`，先阅读差异再调整以下路径假设，**不要 reset**。
- [ ] 阅读 `AGENTS.md`。
- [ ] 阅读 `docs/engineering-constraints.md`。
- [ ] 阅读 `docs/analysis-dag-redesign.md`。
- [ ] 阅读 `docs/native-inference-rewrite-plan.md`，仅作为历史，不把旧模型矩阵覆盖 FINAL docs。
- [ ] 把本最终设计包放入仓库建议路径 `docs/refactor/final-v1/`。
- [ ] 新建/拷贝 `API_CHANGE_LOG.md`，后续每个 API 变更必须实时登记。
- [ ] 记录当前 `API_CAPABILITIES` command 列表，作为 before snapshot。
- [ ] 记录当前所有 tracked Python 文件列表，作为 deletion checklist。
- [ ] 记录 `flake.nix`、CI、AGENTS、engineering constraints 中 Python/uv/venv 引用。
- [ ] 对本次预计触碰的 source file 做行数清单。
- [ ] 标出 >1600 行热点，不允许继续向热点文件堆职责。
- [ ] 检查用户 model/cache/source 路径规则，确保迁移不会做破坏操作。
- [ ] Phase 0 diff review 完成。
- [ ] **不要编译。**

---

# 9. Phase 1 — 建立 Workflow Domain，不碰 UI

> 先把“用户可编辑流程”变成稳定 domain，再开始改页面。

## 9.1 类型

- [ ] 新建 `app-core/src/workflow/` 模块。
- [ ] 定义 `WorkflowNodeId`，与 `AnalysisNodeId` 分离。
- [ ] 定义 `CapabilityId`。
- [ ] 定义 `WorkflowNodeInstance`。
- [ ] 定义 typed input/output port。
- [ ] 定义 `WorkflowEdge`。
- [ ] 定义 `AnalyzerBinding`。
- [ ] 定义 `ExecutionPolicy::{Always,Conditional,Disabled}` 或等价稳定语义。
- [ ] 定义 `priority`，明确不表示 dependency。
- [ ] 定义 `WorkflowLayout`，明确只用于 UI。
- [ ] 给所有持久结构加 schema version。
- [ ] 新字段用 `serde(default)` / migration 保持旧数据可读。

## 9.2 Capability Registry

- [ ] 建立 Node Capability Registry。
- [ ] Audio capability 至少表达 `accepts roles` / `produces roles`。
- [ ] Denoise/Dereverb 能保持 semantic role。
- [ ] Vocal/BGM separation 输出 Vocal + Instrumental。
- [ ] Lead/Harmony separation 输出 LeadVocal + BackVocal。
- [ ] Analyzer capability 表达所需 input artifact/evidence，而不是列表位置。
- [ ] Fusion capability 表达 hard dependencies。
- [ ] 不把 model id 写死在 capability；model 是 node instance configuration。

## 9.3 Validator

复用 `AnalysisGraphSpec` 的图算法思想：

- [ ] duplicate instance 检查。
- [ ] unknown port 检查。
- [ ] type compatibility。
- [ ] hard dependency。
- [ ] cycle detection。
- [ ] analyzer source validity。
- [ ] terminal/final output validity。
- [ ] conditional dependency validity。
- [ ] 生成 user-facing validation message，不直接显示内部 enum debug string。

## 9.4 阶段收口

- [ ] 检查没有创建重复的 DAG topo algorithm。
- [ ] 检查每个新 app-owned API 是否登记；若全是内部 domain，不新增 API。
- [ ] `git diff --check`。
- [ ] source line-count audit。
- [ ] **不要编译。**

---

# 10. Phase 2 — Workflow Compiler + 旧配置迁移

## 10.1 复用 `AudioProcessingPlanSnapshot`

- [ ] 写 `AudioProcessingSettings -> WorkflowDefinition` migration。
- [ ] 保证旧 `vocal_cleanup_chain` 顺序被准确迁移。
- [ ] 保证旧 `accompaniment_cleanup_chain` 顺序被准确迁移。
- [ ] 旧 `karaoke_model_id`/multistem side path 不丢。
- [ ] 不改变已保存 song 的现有结果，仅影响下一次 re-analysis。

## 10.2 Compiler

目标：

```text
WorkflowDefinition
→ validation
→ Compiled AnalysisGraphSpec
→ WorkflowExecutionSnapshot
→ existing AnalysisRequest/AnalysisPlan
```

- [ ] 编译时给每个 node instance 产生稳定 compiled `AnalysisNodeId`。
- [ ] `AnalysisGraphSpec::validate()` 继续作为 compiled graph final guard。
- [ ] 保留 `AnalysisGraphSpec` 给历史 run snapshot/Advanced Graph。
- [ ] 不再把 `baseline_graph_spec()` 当未来业务唯一真相。
- [ ] baseline graph 保留为 legacy/default migration source。
- [ ] `analysis_plan` 改为接收 compiled graph/snapshot，而不是强依赖 baseline static graph。
- [ ] Freeze/Disable/Bypass 语义继续有效。
- [ ] Cache decision 继续走现有 plan。
- [ ] 将 Workflow revision/id 记录到 run snapshot。

## 10.3 Workflow 持久化

- [ ] 优先评估 per-song workflow 与 global preset 的现有存储位置。
- [ ] 不把 Workflow JSON 硬塞进 `analysis_profile` 字段。
- [ ] 若新增 DB table，必须 versioned migration 且旧数据库可打开。
- [ ] Preset 与 song override 分离。
- [ ] UI layout 可持久化，但不参与 execution hash。
- [ ] 若需要新 API，按第 7 节最小集合 + ledger 流程执行。

- [ ] Phase 2 静态 review。
- [ ] **不要编译。**

---

# 11. Phase 3 — Artifact / Cache 泛化到动态 Workflow

当前 stage-specific kind 如 `DenoisedVocalStem`/`DereverbedVocalStem` 不足以表达任意顺序和重复处理。

## 11.1 兼容优先

- [ ] 不删除旧 `ArtifactKind` variant，历史 DB/JSON 必须继续读。
- [ ] 引入 generic workflow audio artifact/descriptor，或用等价 sidecar metadata。
- [ ] `AudioRole` 至少支持 SourceMix/Vocal/LeadVocal/BackVocal/Instrumental。
- [ ] processing chain 从 lineage 得出，不创造 `DereverbedThenDenoisedLeadStem` 这类枚举爆炸。
- [ ] 每个 intermediate audio 可被 Artifact Store capture。
- [ ] producer identity 对应具体 workflow node instance。
- [ ] input revision 精确绑定。
- [ ] runtime recipe digest 纳入影响输出的 cache identity。
- [ ] model digest 继续纳入。
- [ ] Artifact path 永远在授权 cache/store root。

## 11.2 Editor/Workbench 兼容

- [ ] 旧 Artifact Workbench API 继续工作。
- [ ] typed preview 对 generic audio 返回 metadata。
- [ ] lineage 能显示 workflow node instance。
- [ ] impact preview 基于 compiled graph。
- [ ] Set Active / Pin / Invalidate / Delete 语义保持。
- [ ] AuthoredChart 不因 intermediate artifact 新结构被重写。

- [ ] Phase 3 diff/API/line audit。
- [ ] **不要编译。**

---

# 12. Phase 4 — Native Worker Protocol 与 Runtime Router

## 12.1 替换 Python TCP 架构

当前 `app-core/src/analyzer/server.rs` 启动 Python server + loopback TCP。

目标：

```text
Rust supervisor
   └─ child process
       stdin  NDJSON commands
       stdout NDJSON machine events only
       stderr logs
```

- [ ] 定义 versioned `ready / run / progress / output / done / error` frame。
- [ ] stdout JSON mode 不混普通日志。
- [ ] stderr/durable log 承担诊断。
- [ ] cancel 优先通过协议，超时可 kill child。
- [ ] crash/exit code 映射回现有 node attempt/error system。
- [ ] Rust 负责 overall DAG progress；worker 只报本任务 local progress。
- [ ] worker 只能写 run temp dir，Rust 校验后 commit Artifact。
- [ ] 禁止 HTTP server。
- [ ] 禁止 Python fallback。

## 12.2 Runtime Router

- [ ] 读取 final runtime lock。
- [ ] Generic model capability：OpenVINO / Vulkan。
- [ ] 只选 `ProductionPinned`/已验证 backend。
- [ ] backend unavailable → fail closed。
- [ ] CPU 只在 diagnostics/reference explicit mode。
- [ ] runtime selection 写入 attempt record。
- [ ] runtime recipe digest 写入 cache/artifact provenance。

## 12.3 两个 Qwen 固定例外

### Qwen3-ASR-1.7B

- [ ] runtime repo = `handy-computer/transcribe.cpp`
- [ ] runtime commit = `ea077b87590bcfb090d7c38c03ab36cd1c7005d3`
- [ ] GGML = `8c63e70982c95ceb862e3a1073a2c1beef75d60a`
- [ ] source model revision = `7278e1e70fe206f11671096ffdd38061171dd6e5`
- [ ] GGUF = `Qwen3-ASR-1.7B-Q4_K_M.gguf`
- [ ] SHA-256 = `b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e`
- [ ] backend 固定 Vulkan。

### Qwen3-ForcedAligner-0.6B

- [ ] runtime repo = `predict-woo/qwen3-asr.cpp`
- [ ] runtime commit = `6dcc586e5073fd6e85ee5728e75f0903d6c70c6c`
- [ ] model revision = `c07281df297b9905d24a508279258cccf987a064`
- [ ] CPU/reference GGML pin = `9be313313c8ecb9488911bd64550190e3ed80f38`
- [ ] production Vulkan GGML override = `8c63e70982c95ceb862e3a1073a2c1beef75d60a`
- [ ] 任何 compatibility patch 必须 vendor + hash。
- [ ] backend 固定 Vulkan。

不要把这两个 worker 假装成同一个 implementation。

- [ ] Phase 4 static review。
- [ ] **不要编译。**

---

# 13. Phase 5 — Separation / Restoration / Harmony Native 化

## 13.1 RoFormer

复用：

```text
native-inference/roformer
```

- [ ] 把现有 diagnostics-only CLI 收口为稳定 worker contract。
- [ ] 保留 progress/cancel callback。
- [ ] 保留 durable Vulkan logging。
- [ ] batch/async/coopmat 等默认值必须来自已验证 profile，不凭感觉开启。
- [ ] 不在 runtime 自动下载 GGML/model。
- [ ] model registry 记录 license/revision/hash/backend validation。
- [ ] benchmark winner 与 ProductionPinned 分开。

## 13.2 Workflow 能力

实现/登记 capability：

- [ ] Vocal/BGM separation。
- [ ] BGM extraction/refinement。
- [ ] Denoise。
- [ ] Dereverb。
- [ ] Lead/Back-Harmony separation。
- [ ] optional multistem 若最终仍需。
- [ ] 允许 Denoise/Dereverb 重复 node instance。
- [ ] 允许先 Harmony 再 cleanup，或先 cleanup 再 Harmony，只要 type 合法。
- [ ] Vocal/BGM 两 lane 独立。

## 13.3 模型选择

最终设计中的“最高分候选”只作为 BenchmarkCandidate，直到：

- checkpoint 可获得；
- license 可接受；
- hash 可固定；
- native graph 能加载；
- target GPU 完整验证。

不能为达到文档名称而假造权重/hash。

- [ ] 现有已经验证的模型可暂时 ProductionPinned。
- [ ] 新冠军模型通过完整验收后再替换默认。
- [ ] 用户已有旧模型文件不自动删除。

- [ ] Phase 5 review。
- [ ] **不要编译。**

---

# 14. Phase 6 — ASR / Transcript Fusion / Alignment

## 14.1 Chinese route

- [ ] FireRedASR2-AED：中文/方言/中文唱声 primary。
- [ ] 首选 OpenVINO；Vulkan 只有在有已验证实现时才启用。
- [ ] 保留 model confidence。
- [ ] 保留 word/character timestamp 作为 Alignment Evidence。
- [ ] Maximum 可加入 FireRedASR2-LLM challenger，但不要让它成为硬依赖。

## 14.2 Multilingual/Qwen route

- [ ] Qwen3-ASR-1.7B 使用 pinned `transcribe.cpp` Vulkan。
- [ ] Qwen transcript-only 事实保持；不要伪造 timestamp。
- [ ] 语言路由与 auto detect 结果记录 provenance。
- [ ] 中文也可把 Qwen 当 secondary independent evidence。

## 14.3 Transcript Fusion

- [ ] Canonical Lyrics 不直接等于任意一个 ASR output。
- [ ] token/segment evidence 保留来源与 confidence。
- [ ] CJK normalize/segmentation 语义迁移到 Rust/native。
- [ ] hallucination filtering 的行为用 golden fixture 固化。
- [ ] known lyrics 路线与 generated ASR 路线分开。
- [ ] source-time offset 规则保持。

## 14.4 Forced Alignment

- [ ] Qwen Forced Aligner 使用 pinned predict-woo Vulkan recipe。
- [ ] Canonical Lyrics 是 hard dependency。
- [ ] FireRed timestamp 是辅助 evidence，不冒充 forced aligner。
- [ ] 对齐输出保留 word/character boundaries + confidence。
- [ ] 长音频 chunk/merge 规则 deterministic。
- [ ] 任何 fallback 明确，不能静默 CPU/Python。

- [ ] Phase 6 review。
- [ ] **不要编译。**

---

# 15. Phase 7 — Pitch / Note / Technique / Fusion

实现最终 Expert 架构，不退化成 `RMVPE -> round MIDI`。

## 15.1 F0

- [ ] RMVPE = primary continuous singing F0。
- [ ] 保留原 16k mono / 10ms hop 等兼容语义，除非 final design 明确更新。
- [ ] RMVPE 优先 OpenVINO。
- [ ] FCPE = secondary independent F0 expert。
- [ ] voiced probability/confidence 做 calibration。
- [ ] octave disagreement 可被 review。

## 15.2 Boundary / onset

- [ ] GAME = primary note-boundary expert。
- [ ] STARS = secondary boundary + technique expert。
- [ ] Basic Pitch 降为 auxiliary onset/activation/contour。
- [ ] 不让 Basic Pitch 成为最终 F0 truth。
- [ ] VocalParse 只做 Maximum/high-level symbolic prior，不做 10ms boundary。

## 15.3 Technique / DSP

- [ ] Rust DSP 实现 acoustic onset/energy/periodicity/spectral features。
- [ ] vibrato/glissando/ornament 是 technique/evidence，不自动制造多个 MIDI note。
- [ ] STARS 如果依赖 RMVPE，Fusion 做 correlation discount。
- [ ] provenance 中记录 expert dependency。

## 15.4 Fusion

- [ ] 引入 confidence calibration。
- [ ] dynamic expert weighting。
- [ ] disagreement windows。
- [ ] Candidate Graph。
- [ ] duration-aware HSMM/Viterbi。
- [ ] Canonical Singing Track 输出 notes/F0/lyrics/boundaries/techniques/confidence/provenance。
- [ ] 生成 `ReviewRegion[]`。
- [ ] Fast/Balanced/Maximum 是 profile，不改 Artifact 语义。
- [ ] Conditional expert 只在条件区域执行。

- [ ] Phase 7 review。
- [ ] **不要编译。**

---

# 16. Phase 8 — Processing Studio 主工作区

**不要把它塞进现有大型 analysis UI 文件。**

新建 `desktop/src/studio/processing_studio/`。

## 16.1 Route / State

- [ ] 新增一等 route/state，而不是把现有 `AnalysisInspect` 改名硬塞。
- [ ] Song-level navigation 最终为 `Processing | Graph | Editor | Results` 或等价一等入口。
- [ ] Processing 状态与 Analysis Graph 状态分离。
- [ ] WorkflowLayout 与 WorkflowDefinition 分离。
- [ ] drag position 不参与 execution hash。

## 16.2 Audio Workflow Canvas

- [ ] 长方形 draggable node。
- [ ] 清晰 drag handle。
- [ ] Vocal/BGM lane。
- [ ] Harmony split 后 Lead/Back branch。
- [ ] drag Audio Transformation 会真实改变 dataflow。
- [ ] 非法 type drop 显示可理解错误并回弹。
- [ ] cycle drop 禁止。
- [ ] 同 capability 可重复。
- [ ] node model selector。
- [ ] model badge：BenchmarkCandidate/ProductionPinned/Experimental/Legacy。
- [ ] runtime 只显示解析结果，普通用户不选 GGML commit/backend internals。

## 16.3 Analyzer attachment

- [ ] Analyzer 绑定具体 Artifact。
- [ ] ASR source 与 Pitch source 可不同。
- [ ] attachment drag 不改变 audio chain。
- [ ] parallel analyzer reorder 主要改变 priority。
- [ ] Conditional policy：Always / disagreement / Maximum / Disabled。
- [ ] reuse existing model panel/status API，不再做一套 Model Manager。

## 16.4 Inspector

复用现有 UI patterns：

- [ ] Model。
- [ ] Input Artifact。
- [ ] Outputs。
- [ ] Execution policy。
- [ ] resolved runtime。
- [ ] cache/revision。
- [ ] Run this / Run downstream（复用已有 Analysis commands/API）。
- [ ] Open lineage/impact（复用 Artifact Workbench）。
- [ ] Advanced 里才显示 runtime lock/evidence。

## 16.5 API discipline

- [ ] 优先复用 `run_analysis_request` 等现有 API。
- [ ] Workflow persistence/compile 若必须新增 API，先 ledger。
- [ ] 所有按钮进入 typed UiCommand。
- [ ] `ui_api` coverage 更新。
- [ ] 三语言 i18n。

- [ ] Phase 8 review。
- [ ] **不要编译。**

---

# 17. Phase 9 — Advanced Graph 适配，不删除

现有 Graph/Artifact Workbench 投入很大，继续保留。

- [ ] Graph 数据源改为 selected Workflow/Run 的 compiled DAG。
- [ ] 默认不允许直接拖图改变 Workflow。
- [ ] MINI/lineage/edge binding 继续工作。
- [ ] node attempt/log/IO inspector 继续工作。
- [ ] selected historical run 仍解析当时 revision，不 fallback 当前 Active。
- [ ] static baseline graph 只用于 legacy/default，不能覆盖 compiled graph。
- [ ] Processing node 可跳 Graph 对应 compiled node。
- [ ] Graph Artifact/Candidate 可跳 Editor。
- [ ] 现有 Artifact context menu 能复用就复用。

- [ ] Phase 9 review。
- [ ] **不要编译。**

---

# 18. Phase 10 — Editor Bridge 与功能增强

**Editor 是保留项，不重做。**

## 18.1 不允许回退的现有能力

确认保持：

- [ ] multi-track Lead/Harmony/Backing/Adlib。
- [ ] duet part。
- [ ] piano timeline。
- [ ] waveform。
- [ ] beat grid。
- [ ] pitch contour。
- [ ] lyric/note editing。
- [ ] split/merge/quantize/copy/paste。
- [ ] bind/unbind。
- [ ] tap-to-time。
- [ ] Audio/Pitch/Mixed audition。
- [ ] lock mode。
- [ ] problems。
- [ ] undo/redo。
- [ ] whole-song lyrics editor。
- [ ] UTZ/UltraStar editor exports。

## 18.2 Artifact audio source

当前 Original/Vocals/Instrumental picker 泛化为 Workflow Artifact revisions：

- [ ] Playback source。
- [ ] Waveform source。
- [ ] optional A/B source。
- [ ] 保持 playhead。
- [ ] source metadata 显示 role/producer/model 简短 provenance。
- [ ] 不修改 EditorDocument。

## 18.3 Evidence Workbench

新增只读 layer：

- [ ] Fused F0。
- [ ] RMVPE。
- [ ] FCPE。
- [ ] GAME boundary。
- [ ] Basic Pitch onset。
- [ ] Qwen/FireRed word boundaries。
- [ ] STARS technique。
- [ ] Fusion confidence。
- [ ] disagreement regions。

默认层要克制，Authored notes 永远视觉最强。

## 18.4 Review Queue

- [ ] `ReviewRegion` next/previous。
- [ ] low confidence。
- [ ] octave risk。
- [ ] boundary disagreement。
- [ ] word-note mismatch。
- [ ] voicing conflict。
- [ ] lead/harmony contamination。
- [ ] reviewed state。

## 18.5 Suggestion

- [ ] Suggestion 不直接修改 document。
- [ ] Accept 通过现有 EditorAction/undo 系统。
- [ ] Ignore 不修改 chart revision。
- [ ] “Inspect evidence” 只改变 UI view。
- [ ] 不把 model suggestion 混成 blocking chart error。

## 18.6 Candidate/Authored

复用现有 revision load/merge：

- [ ] Candidate ready → Open in Editor。
- [ ] Save → Authored revision。
- [ ] upstream rerun → Authored 保留，提示 New candidate。
- [ ] Compare。
- [ ] Merge。
- [ ] Keep Authored。
- [ ] 不自动 replace。

## 18.7 Harmony

- [ ] Lead candidate → Lead track。
- [ ] Harmony → Harmony track。
- [ ] Backing → Backing track。
- [ ] Adlib → Adlib track。
- [ ] 默认非 Lead reference/non-scored，用户可改。
- [ ] 不建“独立和声编辑器”。

## 18.8 文件规模

不要继续把 evidence/review 全塞进：

```text
editor/actions.rs
editor/panels.rs
editor/view/timeline.rs
```

新模块独立，现有文件只接 wiring。

- [ ] Phase 10 review。
- [ ] **不要编译。**

---

# 19. Phase 11 — Models & Runtime / Vendor / Status Native 化

当前：

```text
app-core/src/vendor/setup.rs
app-core/src/vendor/status.rs
app-core/src/vendor/types.rs
app-core/src/vendor_scripts.rs
```

仍是 Python-centric。

## 19.1 Runtime status

改为 native component：

```text
roformer_runtime
openvino_runtime
qwen_asr_runtime
qwen_align_runtime
vulkan_device
openvino_gpu
installed models
runtime-lock integrity
missing components
```

- [ ] 删除 uv/python/venv readiness 语义。
- [ ] runtime lock digest 可诊断。
- [ ] Qwen 两 runtime 分开报告。
- [ ] generic model 显示 validated OpenVINO/Vulkan capability。
- [ ] unsupported 明确，不 silent CPU。

## 19.2 Setup

- [ ] explicit install only。
- [ ] 不下载 tool/model on launch。
- [ ] 支持 native helper/model install/reinstall/remove。
- [ ] 用户旧 model dir 不自动清理。
- [ ] model license/size/hash/status 在 UI 可见。
- [ ] runtime binary 包装进 Nix/package，或使用明确受控安装路径。
- [ ] 不在 setup 中装 Python。

## 19.3 Config

- [ ] old config 可读 migration。
- [ ] new save 不再写 torch/python/whisperx/demucs obsolete 字段。
- [ ] runtime preference 保存“模型/质量/策略”，不是随意 backend toggle。
- [ ] Qwen pin 不暴露给普通用户更改。

- [ ] Phase 11 review。
- [ ] **不要编译。**

---

# 20. Phase 12 — 删除 Python 与旧 Runtime

只有 native domain/UI/worker 都已接好后执行。

## 20.1 删除

- [ ] `app-core/analyzer/**/*.py`
- [ ] analyzer Python tests。
- [ ] `server.py`。
- [ ] Python audio model catalog/runtime code。
- [ ] `vendor_scripts.rs` 中 embedded Python。
- [ ] Python/uv/venv setup/status helpers。
- [ ] `scripts/build-user-guide.py`；使用 canonical `cargo xtask docs`。
- [ ] `tools/import_uvr_audio_catalog.py`；需要功能则 port 到 xtask，否则删除。
- [ ] flake Python/uv inputs/wrappers。
- [ ] CI Python compileall。
- [ ] docs 中 active Python setup instructions。
- [ ] obsolete models/runtime paths：HTDemucs/MDX/Parakeet/WhisperX/Wav2Vec2 等按 final design 清理 support。
- [ ] 不删除用户磁盘上的旧模型数据。

## 20.2 Zero Python gate

```sh
test -z "$(git ls-files '*.py' '*.pyi')"
```

必须成功。

再扫 active code：

```sh
rg -n \
  'UTA_STUDIO_PYTHON_PATH|UTA_STUDIO_UV_PATH|python_path\(|configured_python_path\(|uv_path\(|venv|server\.py|app-core/analyzer' \
  --glob '!docs/validation/**'
```

active executable/config code 必须零命中；历史 validation 文档可明确保留旧事实。

- [ ] Phase 12 review。
- [ ] **不要编译。**

---

# 21. Phase 13 — API / i18n / Docs / AGENTS 最终同步

## 21.1 API

- [ ] API ledger 所有 entry 状态变为 Reused/Extended/Added/Rejected。
- [ ] 每个 Added API 已进 `API_CAPABILITIES`。
- [ ] 没有未登记的 app-owned public API。
- [ ] 旧 API 若替换，保留兼容或明确 migration。
- [ ] command uniqueness。
- [ ] access class 正确。
- [ ] diagnostics 不调用 destructive/mutating workflow。

## 21.2 UI API

- [ ] 每个 Processing Studio button/menu 有 typed UiAction。
- [ ] Editor 新 action 纳入现有 Editor action registry，能复用就不新增。
- [ ] keyboard/pointer behavior 不回退。
- [ ] pointer capture cleanup 保留。

## 21.3 i18n

- [ ] en/zh-CN/ja keys 同步。
- [ ] Processing/Graph/Editor/Results 文案完成。
- [ ] validation error 用户可理解。
- [ ] runtime error 不暴露无意义内部 exception。

## 21.4 仓库规则文档

更新 `AGENTS.md` 与 `docs/engineering-constraints.md`：

- [ ] 删除 Python/uv/venv runtime 指示。
- [ ] 写入 generic OpenVINO→Vulkan + Qwen pinned exception。
- [ ] 保留 explicit download。
- [ ] 保留 API rules。
- [ ] 保留 2000 line。
- [ ] 保留 Wayland-only。
- [ ] 保留 Editor/product safety。
- [ ] Definition of done 更新为 native final gates。

## 21.5 用户文档

- [ ] `docs/user-guide/en.md`
- [ ] `docs/user-guide/zh-CN.md`
- [ ] `docs/user-guide/ja.md`
- [ ] Processing Studio。
- [ ] Graph。
- [ ] Editor Evidence/Review。
- [ ] Models & runtime。
- [ ] zero-Python 安装说明。
- [ ] final native backend status。

- [ ] Phase 13 review。
- [ ] **不要编译。**

---

# 22. Phase 14 — 最终编译前静态审计

这是最后一个“不编译”阶段。

## 22.1 Diff

- [ ] `git status --short`
- [ ] `git diff --check`
- [ ] 检查没有意外修改 source media / generated user data。
- [ ] 检查没有把模型大文件误提交。
- [ ] 检查 third-party attribution/license。

## 22.2 规模

- [ ] 全 app-owned source ≤2000 行。
- [ ] 热点文件职责清晰。
- [ ] `processing_studio` 已拆模块。
- [ ] Editor evidence/review 已拆模块。
- [ ] `api.rs` 若逼近 2000 行，内部拆 area catalogue，但保留统一 `api_capabilities()` public contract。

## 22.3 API

- [ ] API ledger 无 “TODO audit”。
- [ ] 搜所有新增 `pub fn`/command，确认是否需要 API entry。
- [ ] 没有 duplicate command。
- [ ] 没有为了 UI rename 的冗余 API。

## 22.4 Python

- [ ] tracked Python = 0。
- [ ] Python/uv/venv active refs = 0。
- [ ] Nix/CI 无 Python compile。
- [ ] package runtime 不探测 Python。

## 22.5 Runtime pins

- [ ] Qwen ASR commit/hash 完全匹配 runtime lock。
- [ ] Qwen align runtime/GGML override 完全匹配 runtime lock。
- [ ] compatibility patch digest 记录。
- [ ] Generic backend validation status 不造假。
- [ ] no silent CPU fallback。

## 22.6 产品语义

- [ ] Workflow compile 不依赖 UI position。
- [ ] Advanced Graph 使用 compiled graph。
- [ ] AuthoredChart 保护。
- [ ] Candidate/Authoring provenance。
- [ ] Source read-only。
- [ ] downloads explicit。
- [ ] no HTTP server。

- [ ] **到这里为止仍不要编译。**

---

# 23. Phase 15 — 最终一次性 Build / Test / Fix / Package

现在才开始编译。

## 23.1 基础格式与 Rust

按顺序：

```sh
nix develop path:. -c cargo fmt --all -- --check

nix develop path:. -c cargo check \
  --workspace --all-targets --locked

nix develop path:. -c cargo test \
  --workspace --all-targets --locked

nix develop path:. -c cargo clippy \
  --workspace --all-targets --locked -- -D warnings

nix develop path:. -c cargo xtask docs check
```

- [ ] fmt pass。
- [ ] check pass。
- [ ] test pass。
- [ ] clippy pass。
- [ ] docs check pass。

若失败，在 Phase 15 内修复并重新执行相关 gate。

## 23.2 Native C++ runtimes

分别 configure/build/test：

- [ ] RoFormer runtime。
- [ ] OpenVINO worker/runtime。
- [ ] Qwen ASR transcribe.cpp integration。
- [ ] Qwen Forced Aligner predict-woo integration。
- [ ] 其它 native helper。

要求：

- [ ] 不依赖 Python runtime。
- [ ] Vulkan explicit。
- [ ] OpenVINO explicit。
- [ ] runtime lock 打印/诊断正确。
- [ ] stdout NDJSON 无日志污染。

## 23.3 Nix package

```sh
nix build path:.#uta-studio --print-build-logs
```

- [ ] package pass。
- [ ] wrapped executable smoke launch。
- [ ] Wayland-only。
- [ ] package 不包含/启动 Python。
- [ ] package 包含需要的 native runtime components / notices。

## 23.4 API / line / zero-python 再跑

- [ ] API catalogue contract test。
- [ ] UI API coverage。
- [ ] 2000-line gate。
- [ ] zero-Python gate。
- [ ] project-name scan。
- [ ] i18n parity。

## 23.5 Native model hardware smoke

使用真实 Intel Arc/self-hosted/manual evidence，普通 hosted CI 不冒充 GPU 验证。

至少：

### RoFormer
- [ ] short real audio。
- [ ] full song。
- [ ] repeated runs。
- [ ] cancel。
- [ ] clean process exit。
- [ ] no driver reset/black screen。

### FireRed
- [ ] OpenVINO real Chinese speech/song。
- [ ] timestamp/confidence。
- [ ] repeated full-song route。
- [ ] cancellation。

### Qwen ASR
- [ ] pinned Q4_K_M hash。
- [ ] Vulkan。
- [ ] transcript parity fixture。
- [ ] full song。
- [ ] repeated runs。
- [ ] cancel/restart。

### Qwen Forced Aligner
- [ ] predict-woo runtime commit。
- [ ] Vulkan GGML override。
- [ ] model revision。
- [ ] known lyrics alignment。
- [ ] full song。
- [ ] repeated runs。
- [ ] cancel/restart。

### RMVPE
- [ ] OpenVINO。
- [ ] F0 golden。
- [ ] voiced/unvoiced。
- [ ] full song。
- [ ] repeated runs。

### Other experts
- [ ] backend-specific smoke。
- [ ] output contract。

## 23.6 Resource contention

- [ ] RoFormer Vulkan + OpenVINO task sequencing。
- [ ] Qwen Vulkan + OpenVINO task sequencing。
- [ ] no unvalidated simultaneous GPU contention。
- [ ] Scheduler obeys priority but does not invent dependencies。
- [ ] memory pressure error cleanly reported。

## 23.7 Full E2E

跑至少一首中文歌和一首非中文歌：

- [ ] import/scan。
- [ ] Processing Studio default workflow。
- [ ] change audio order。
- [ ] separate Vocal/BGM。
- [ ] Harmony split。
- [ ] ASR。
- [ ] transcript fusion。
- [ ] forced alignment。
- [ ] RMVPE/FCPE/GAME/etc evidence。
- [ ] HSMM/Viterbi。
- [ ] CandidateChart。
- [ ] ReviewRegion。
- [ ] Open Editor。
- [ ] evidence layers。
- [ ] next disagreement。
- [ ] edit note/lyric。
- [ ] undo/redo。
- [ ] Harmony track。
- [ ] A/B audio source。
- [ ] save AuthoredChart。
- [ ] rerun upstream。
- [ ] verify AuthoredChart preserved。
- [ ] compare new candidate。
- [ ] UTZ export。
- [ ] UltraStar export。
- [ ] decode exported audio。
- [ ] no temp leak。

## 23.8 Editor playback

- [ ] sustained real-chart audition。
- [ ] audio stream running/unmuted。
- [ ] no xrun/quantum failure。
- [ ] playhead interpolation stable。
- [ ] manual scroll wins temporarily。
- [ ] no concurrent high-parallel build during playback validation。

## 23.9 Final zero-Python

最后再确认：

```sh
test -z "$(git ls-files '*.py' '*.pyi')"
```

并在运行 Uta Studio 完整分析时确认 process tree 中无 Python。

- [ ] zero tracked Python。
- [ ] zero Python process。
- [ ] zero uv/venv setup。
- [ ] zero Python fallback。

---

# 24. Agent 每个 Phase 的提交前自检格式

Phase 0–14 每次结束只做下面这些，不编译：

```text
[ ] 本 phase checklist 完成
[ ] git diff --check
[ ] touched file line counts OK
[ ] API ledger updated
[ ] new public command/API justified
[ ] no source-media destructive behavior
[ ] no silent fallback introduced
[ ] no unrelated user changes overwritten
[ ] no compile/test/build run
```

Phase 15 才填写 build/test evidence。

---

# 25. 禁止做的事情

- [ ] 禁止为了中间态编译通过保留第二套旧 pipeline。
- [ ] 禁止创建“临时 Python fallback”然后忘记删除。
- [ ] 禁止复制现有 Artifact/Analysis APIs 仅换名字。
- [ ] 禁止把 Workflow drag 坐标当执行顺序。
- [ ] 禁止把 priority 当 dependency。
- [ ] 禁止让 Graph 页面成为第二个 Workflow editor。
- [ ] 禁止重写 Editor。
- [ ] 禁止模型 Evidence 自动修改 AuthoredChart。
- [ ] 禁止模型下载随启动触发。
- [ ] 禁止静默 CPU fallback。
- [ ] 禁止把 BenchmarkCandidate 标成 ProductionPinned。
- [ ] 禁止删除用户旧 model/cache/source 数据以“清理迁移”。
- [ ] 禁止 app-owned source file >2000 行。
- [ ] 禁止未登记新 API。
- [ ] 禁止无认证 HTTP control/inference server。
- [ ] 禁止 Linux X11/XWayland fallback。
- [ ] 禁止最终包含 Python runtime。

---

# 26. 完成定义

只有以下全部成立才可以宣称本次重构完成：

- [ ] Dynamic Processing Studio Workflow 已成为用户主工作方式。
- [ ] Compiled DAG / Advanced Graph 继续完整可诊断。
- [ ] Native runtimes 覆盖所有生产分析节点。
- [ ] 两个 Qwen runtime pins 完全一致。
- [ ] Generic OpenVINO/Vulkan router 只使用验证 backend。
- [ ] Canonical Singing Track 多 Expert Fusion 完整。
- [ ] Editor 保留且增强。
- [ ] Candidate/Authored lifecycle 安全。
- [ ] Artifact lineage/cache/revision 精确。
- [ ] Models & runtime 全 native。
- [ ] API catalogue 无漂移。
- [ ] 所有源文件符合 2000 行。
- [ ] repository zero Python。
- [ ] full Rust/native/Nix gates pass。
- [ ] real Intel GPU smoke pass。
- [ ] end-to-end song + editor + exports pass。
- [ ] final docs/i18n/AGENTS 与代码一致。
