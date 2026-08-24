# Uta — 分离式架构总设计 v1.0

**状态**：架构交接基线
**日期**：2026-08-22
**代码仓库**：`bintis/uta-studio`
**审计时参考分支**：`native-inference`
**审计时参考 commit**：`08e332f9ec7a5b943862953ade3febaad71a2a0f`

> 本文档定义 Uta 当前冻结的组件边界。音频算法细节以
> `docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md`
> 和 `docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_SEPARATION_PLAN_v1.1.md` 为权威来源。

---

# 1. 最终组件拆分

系统拆为四个长期独立组件：

```text
utz
uta-runtime-manager
uta-analysis-engine
uta-studio
```

核心原则：

> **UTZ 定义领域交换语义；Runtime Manager 管可运行资源；Analysis Engine 执行分析；Studio 管产品工作流。**

依赖关系：

```text
                     +----------------+
                     |      utz       |
                     | domain format  |
                     +-------^--------+
                             |
                             | candidate/export schemas
                             |
+------------------+         |         +----------------------+
| uta-runtime-     |<--------+-------->| uta-analysis-engine  |
| manager          | resolve / lease   | execution plane      |
+---------^--------+                   +----------^-----------+
          |                                       |
          | install/status                         | AnalyzeRequestV1
          |                                       | ResultManifestV1
          |                                       |
          +-------------------+-------------------+
                              |
                              v
                         +----------+
                         | uta-     |
                         | studio   |
                         | control  |
                         | plane    |
                         +----------+
```

禁止形成：

```text
Studio 自己一套模型真相
+
Engine 自己一套下载逻辑
+
CLI 再一套安装逻辑
```

---

# 2. `utz` 的职责

UTZ 是稳定的领域交换定义与参考实现。

拥有：

```text
UTZ package
manifest
VocalChart 0.3
PitchEvidence 0.3
validation / conformance
feature negotiation
AssetRef
representations
extensions
```

不拥有：

```text
AI inference
model lifecycle
runtime selection
Studio project state
analysis queue
editor state
```

UTZ 0.3 已固定，Analysis Engine 需求不再随意反向改格式。

---

# 3. UTZ 0.3 当前冻结基线

版本：

```text
UTZ package       0.3.x
VocalChart        0.3.x
PitchEvidence     0.3.x
```

Canonical time：

```text
1 second = 1,000,000 integer units
```

核心 feature：

```text
vocal-chart/0.3
pitch-evidence/0.3
```

推荐机器分析扩展：

```text
singing-analysis/0.3
```

音频标准角色：

```text
instrumental    REQUIRED for a valid UTZ package
guide_vocals
original
lead_vocal
backing_vocal
harmony_vocal
```

注意：

```text
clean_lead_vocal
vocal_residual
```

是 Analysis Engine 内部分析 artifact，不是默认 UTZ 标准音频角色。

Representations（MIDI/USTX/UltraStar 等）永远不高于 VocalChart 权威性。

---

# 4. Candidate 与 Authored

Analysis Engine 输出：

```text
Candidate VocalChart
```

Studio 管理：

```text
Candidate
    ↓ review/edit
Authored VocalChart
```

Engine 不拥有：

```text
作者锁定状态
编辑历史
Undo/Redo
游戏规则层的最终 scoring choice
```

默认候选规则保持中立：

```text
Pitched     -> pitch
Rap/Spoken  -> rhythm
Freestyle   -> none
bonus       -> normal
```

---

# 5. `uta-runtime-manager`

Runtime Manager 是唯一的 model/runtime lifecycle truth source。

拥有：

```text
resource catalog
model/runtime/tool/bundle identities
source/revision/license metadata
install/import
verify
repair
reinstall/remove
SHA-256 integrity
conversion recipes
immutable generations
backend validation
policy-aware readiness
resolve
resource lease
doctor/smoke lifecycle support
```

不拥有：

```text
audio algorithm planning
tensor preprocessing
ASR orchestration
Fusion/HSMM
VocalChart inference
Studio UI
Studio DB
```

---

# 6. Runtime resource 类型

统一资源：

```text
model:<id>
runtime:<id>
tool:<id>
bundle:<id>
```

当前主要模型：

```text
model:bs_roformer_vocals_ep317
model:melband_roformer_inst_v2
model:melband_roformer_harmony
model:melband_roformer_denoise_aufr33
model:melband_roformer_dereverb_anvuew

model:qwen3_asr_1_7b
model:qwen3_forced_aligner_0_6b
model:firered_asr2_aed

model:rmvpe
model:fcpe
model:game
model:basic_pitch
model:stars
```

未来候选：

```text
model:rosvot
```

---

# 7. Runtime state 不能压成一个 bool

必须分离：

```text
InstallState
ValidationState
Usability
```

典型：

```text
installed != validated != usable
```

Validation policy：

```text
production
    ProductionPinned only

benchmark
    ProductionPinned + BenchmarkCandidate

experimental
    + Experimental
```

`Unsupported` 永远不可 resolve。

---

# 8. Runtime installation invariant

生产资源是 immutable generation：

```text
Catalog recipe
    ↓
Plan
    ↓
Acquire / Import
    ↓
Source hash verify
    ↓
Convert
    ↓
Output hash verify
    ↓
Install manifest
    ↓
Atomic publish
    ↓
Immutable generation
```

reinstall：

```text
旧 generation 继续可用
    ↓
新 generation 完整构建与验证
    ↓
atomic current switch
```

运行中的 Engine 持有 lease，不能被 remove/reinstall 破坏。

---

# 9. Runtime Manager 不做“自动更新”

禁止：

```text
uta-runtime update -> 上网找最新模型
```

新模型采用显式 release lifecycle：

```text
new upstream checkpoint/model
    ↓
offline benchmark
    ↓
Gold Set regression
    ↓
ONNX/OpenVINO/GGUF/native conversion
    ↓
numerical + semantic parity
    ↓
hardware/runtime validation
    ↓
license audit
    ↓
new catalog recipe
    ↓
explicit user install/reinstall
```

Analysis Engine 和 Runtime Manager 都不做 self-training、pseudo-label 自动训练或后台微调。

---

# 10. `uta-analysis-engine`

Analysis Engine 是 UI-less / library-less / project-DB-less / no-hidden-download 的本地分析执行平面。

一句话定义：

> 接收显式语义标记的音频、歌词与结构约束，完成音频处理、模型推理、证据融合和候选结构生成，返回 UTZ-compatible candidate artifacts。

拥有：

```text
decode/resample/channel normalization
timeline mapping
separation / restoration
lead isolation
analysis cleanup
Qwen ASR
forced alignment
RMVPE
GAME
FCPE
Basic Pitch
FireRed optional expert
STARS optional native candidate
DSP technique evidence
calibration
correlation discounting
candidate graph
HSMM/Viterbi
rhythm quantization
Candidate VocalChart
PitchEvidence
SingingAnalysis
standalone export
worker supervision
cancellation
degraded-result policy
fingerprint
```

不拥有：

```text
model download
project DB
song library
Artifact DB
editor state
product queue
```

---

# 11. Engine stable capability IDs

Public/stable capability names必须是语义而非模型名：

```text
audio.decode
audio.extract_vocals
audio.extract_instrumental
audio.lead_isolate
audio.lead_partition
audio.denoise
audio.dereverb

speech.transcribe
speech.align

pitch.track
pitch.secondary

notes.game
notes.basic_pitch
notes.rosvot
notes.stars

technique.analyze
analysis.acoustic_dsp

fusion.transcript
fusion.alignment
fusion.singing
fusion.candidate_graph

finalize.vocal_chart
rhythm.quantize
```

模型替换不能迫使 Studio workflow 改 capability ID。

---

# 12. Analysis Engine 输入契约

协议：

```text
uta.analysis-engine.request
AnalyzeRequestV1
```

调用方负责：

```text
local file identity
SHA-256
explicit semantic role
primary flag
source_start / canonical timeline
lyrics mode
constraints
requested artifacts
quality/runtime policy
```

Engine 负责确认：

```text
decoded sample rate
channels
codec/container
frame count
duration
peak/decoded facts
```

规则：

```text
exactly one primary
local file only in v1
SHA-256 mandatory
no filename role inference
no hidden time-stretch
canonical external time = 1_000_000 units/s
```

---

# 13. 输入音频语义

允许：

```text
original_mix
vocal_stem
guide_vocals
lead_vocal
clean_lead_vocal
instrumental
backing_vocal
harmony_vocal
```

路由：

```text
original_mix
    -> full separation / lead isolation / cleanup

vocal_stem / guide_vocals
    -> lead isolation / cleanup

lead_vocal
    -> cleanup

clean_lead_vocal
    -> direct analysis

instrumental
    -> reference/secondary; not sole singing-analysis primary
```

---

# 14. Studio Product DAG 与 Engine Execution Plan

Studio DAG 表达用户意图：

```text
“我要 vocals”
“我要 transcript”
“我要 pitch”
“我要重新跑这个节点”
```

Engine plan 表达真实执行：

```text
decode
separator model A
lead isolate model B
Qwen worker
RMVPE worker
GAME worker
disagreement escalation
calibration
HSMM
```

规则：

> Studio 决定“做什么/什么时候做/哪个结果被采用”；Engine 决定“怎么执行”。

Studio 永远不准备模型 tensor。

---

# 15. Engine worker/runtime 拓扑

推荐：

```text
Uta Analysis Engine
├── RoFormer Runtime (C++/GGML/Vulkan)
├── Qwen ASR Worker
├── Qwen Align Worker
├── OpenVINO Worker
│   ├── RMVPE
│   ├── GAME
│   ├── FCPE
│   ├── Basic Pitch
│   ├── FireRed
│   └── STARS candidate
└── Native Fusion/DSP core
```

worker protocol：

```text
stdin  -> NDJSON
stdout -> NDJSON machine frames
stderr -> logs
```

无 HTTP / localhost REST。

---

# 16. Audio Analysis 是架构核心，不是附属功能

完整算法设计以 `docs/design/audio-analysis/` 下的当前 Framework、Separation Plan 与 Coverage Checklist 为权威。

关键链：

```text
original_mix
    |
    +----------------------------+
    |                            |
    v                            v
extract vocals              HQ instrumental
    |                            |
guide_vocals                instrumental
    |
lead isolation
    |
+---+----------------+
|                    |
lead_vocal       vocal_residual
|
optional cleanup
|
clean_lead_vocal
|
+--------+--------+---------+----------+
|        |        |         |          |
Qwen     Align    RMVPE     GAME       DSP
|        |        |         |          |
+--------+--------+---------+----------+
                 |
          Canonical Evidence
                 |
          Calibration/Fusion
                 |
          Candidate Graph
                 |
              HSMM
                 |
     +-----------+------------+
     |                        |
Candidate VocalChart     PitchEvidence
     |
SingingAnalysis evidence
```

---

# 17. Audio separation 的两个目标

必须独立优化：

```text
Karaoke / UTZ:
    high-quality instrumental

Analysis:
    clean, semantically usable foreground lead
```

禁止默认：

```text
instrumental = mix - clean_lead_vocal
```

当前 reference recipes：

```text
audio.extract_vocals
    -> bs_roformer_vocals_ep317

audio.extract_instrumental
    -> melband_roformer_inst_v2

audio.lead_isolate
    -> melband_roformer_harmony
       (Lead / Back separation implementation candidate)

audio.denoise
    -> melband_roformer_denoise_aufr33

audio.dereverb
    -> melband_roformer_dereverb_anvuew
```

---

# 18. Separation semantic artifacts

```text
instrumental
guide_vocals
lead_vocal
clean_lead_vocal
vocal_residual
```

其中：

`guide_vocals`
可能包含 lead/harmony/backing/doubles/adlibs/multiple singers。

`lead_vocal`
是可听、可交换的 foreground musical stem。

`clean_lead_vocal`
是 internal analysis working stem，不默认 export。

`vocal_residual`
不能自动标记为 backing/harmony。

---

# 19. Separation quality gates

至少：

```text
timeline
finite
clipping
silence
energy
lead purity
vocal leakage
musical damage
cleanup consistency
```

Balanced/Maximum 应保留 raw `lead_vocal` 与 `clean_lead_vocal` 比较。

若 cleanup 改坏 onset/voicing/pitch contour：

```text
cleanup_damage_suspected
```

并对受影响区间回退较少处理版本。

---

# 20. Duet / multi-singer

VocalChart 已能表达多 track / 多 singer。

语义：

```text
part = singer/player assignment
role = musical role
```

交替 duet：

```text
shared foreground lead
+ alignment
+ part/time constraints
```

可在 v1 分析。

同时 duet：

```text
monophonic F0 assumption may fail
```

必须检测、降级并保留不确定性。

能力分离：

```text
audio.lead_isolate
    foreground vs support

audio.lead_partition
    multiple simultaneous foreground singers
```

`lead_partition` 是 future/optional，不阻塞 v1。

---

# 21. Baseline experts

Fast baseline：

```text
Qwen3-ASR-1.7B
Qwen3 ForcedAligner 0.6B
RMVPE
GAME
DSP
required separation
Fusion/HSMM
```

Balanced：

```text
Fast
+ lead purity
+ vocal topology
+ cleanup consistency
+ FCPE disagreement
+ Basic Pitch disagreement
+ optional FireRed challenger
```

Maximum：

```text
Balanced
+ consistency reruns
+ secondary separation
+ STARS/ROSVOT optional experts
+ future lead_partition
```

Maximum 不等于整首歌跑所有模型。

---

# 22. GAME / STARS / ROSVOT 当前路线

GAME：

```text
官方 ONNX release assets
-> hash pinning
-> OpenVINO conversion
-> native worker
-> primary note/boundary expert
```

GAME 是 P0。

STARS：

```text
official CKPT
-> inference-only export wrapper
-> ONNX subgraphs
-> parity
-> OpenVINO
-> optional multi-task expert
```

动态 boundary regulation、`.item()` 驱动 shape、DP/Viterbi、TextGrid/MIDI 等放 Rust/C++ host，不强塞整套 Python program 进 ONNX。

ROSVOT：

```text
official checkpoint exists
-> ONNX/native feasibility
-> parity
-> license/runtime validation
```

属于后续 expert，不阻塞 baseline。

---

# 23. Continuous pitch 与 target notes 必须分离

RMVPE：

```text
continuous F0 sensor
```

输出：

```text
f0_hz
voicing
confidence
```

禁止：

```text
每帧 nearest MIDI
-> 直接成为 note
```

连续 MIDI：

```text
m = 69 + 12 * log2(f0 / 440)
```

但 VocalChart note pitch 是语义目标；PitchEvidence 是物理连续证据。

---

# 24. Note inference / fusion

Candidate boundary sources：

```text
GAME
Basic Pitch onset
Alignment
F0 discontinuity
voicing transition
DSP articulation
hard constraints
future ROSVOT/STARS
```

最终流程：

```text
candidate boundaries
    ↓
segment pitch alternatives
    ↓
calibrated evidence
    ↓
correlation discount
    ↓
context-aware weights
    ↓
candidate graph
    ↓
segment-level HSMM/Viterbi
    ↓
semantic notes
    ↓
rhythm quantization
```

量化必须在 semantic note 后。

---

# 25. Expressive singing rules

Vibrato：

```text
one note + periodic pitch modulation
```

不能因跨 semitone 就碎成多个音符。

Glissando：

中间经过的 semitone 默认是 pitch movement，不是新 note，除非有 onset/plateau/boundary/duration 等证据。

Melisma：

```text
one lyric token -> multiple notes
```

必须保留 continuation semantics。

Octave correction：

全局/上下文决定，不做 blanket ×2 / ÷2。

---

# 26. Confidence / evidence

不同模型 raw score 不能直接比较。

必须：

```text
temperature scaling
isotonic / Platt-style calibration
reliability evaluation
versioned calibrator
```

相关 expert 不能重复投票：

```text
correlation group
dependencies
```

Disagreement window 是主要成本控制：

```text
baseline
-> detect conflict
-> selectively run secondary experts
```

---

# 27. Engine 输出

核心：

```text
AnalysisResultManifestV1
Candidate VocalChart 0.3
PitchEvidence 0.3
SingingAnalysis/0.3
Transcript
Alignment
requested stems
diagnostics
provenance
fingerprint
degraded_reasons
```

状态：

```text
ok
ok_degraded
failed
cancelled
```

机器 evidence 引用 VocalChart stable IDs，不复制第二套权威 note geometry。

---

# 28. Engine output ownership

Engine 只写授权 run-temp：

```text
Engine
    -> run-temp
```

Studio：

```text
validate
hash
semantic check
atomic Artifact DB commit
```

Engine 不直接写 Studio Artifact DB。

---

# 29. Analysis fingerprint

必须覆盖：

```text
input SHA
request contract/version
audio role/timeline
lyrics/constraint hash
profile

model IDs
immutable model generations/content digests
runtime generations
runtime recipe digest
backend/device
validation policy

separation/cleanup recipes
calibration version
fusion version
HSMM version
postprocess/quantization version
```

---

# 30. `uta-studio`

Studio 是产品 control plane。

拥有：

```text
library
source authorization
Processing Studio outer DAG
queue
retry/cancel
Artifact DB
artifact revision/history
cache
freeze/bypass
Candidate/Authored
editor
undo/redo
model/runtime UI frontend
final export UX
```

它不拥有：

```text
model tensor preparation
inference model lifecycle truth
worker-specific neural preprocessing
```

---

# 31. Studio API 原则

尽量保留产品语义 API：

```text
list_audio_models
get_audio_model_status
install_audio_model
reinstall_audio_model
remove_audio_model

analysis_runtime_status
validate_audio_processing_profile
preview_effective_audio_params

run_analysis_plan
run_analysis_node
run_analysis_node_downstream
run_analysis_request

reanalyze_transcript
reanalyze_full
reanalyze_pitch
realign
reanalyze_force_transcribe
```

不要因为实现变成 Qwen/GAME/RMVPE 就新增一堆 public `qwen_*` / `game_*` API。

三套 registry 分开：

```text
Studio app API
Engine protocol/capabilities
Native worker ABI/runtime recipes
```

---

# 32. Standalone Analysis Engine

Engine 必须脱离 Studio 工作。

例如：

```text
lead_vocal
    ↓
Analysis Engine
    ↓
Candidate VocalChart
PitchEvidence
MIDI / USTX
```

但是 UTZ 0.3 package 要求 instrumental。

如果只给 lead：

```text
不能伪造静音 instrumental
不能声称输出有效 UTZ package
```

必须由调用方提供 instrumental，或从 original_mix 生成。

---

# 33. Native-only / offline production

正式执行链：

```text
native binaries
local files
pinned models
no network
no Python runtime
```

Python/PyTorch 只允许在：

```text
model export
conversion research
parity benchmark
offline validation
```

不进入 production Engine。

---

# 34. 不做 self-evolving model

删除：

```text
online self-training
pseudo-label promotion
production weight mutation
background fine-tuning
automatic teacher/student evolution
```

保留为测试的变换：

```text
pitch-shift equivariance
time-stretch consistency
gain/EQ/noise/codec robustness
```

这些只产生 regression signal，不触发训练。

---

# 35. 集成顺序

固定顺序：

```text
1. Runtime Manager
   lifecycle/status/resolve/lease 测试

2. Analysis Engine standalone
   separation
   Qwen
   RMVPE
   GAME
   Fusion/HSMM
   real-song tests

3. Engine contract freeze
   AnalyzeRequestV1
   ResultManifestV1
   capability/error/fingerprint

4. Studio reintegration
   Runtime Manager frontend
   Engine client
   Studio DAG adapter
   Artifact commit
   Candidate/Authored workflow

5. Product E2E
   import
   explicit model install
   analyze
   inspect
   edit
   export
```

关键 gate：

> **独立 Analysis Engine 没通过真实音频闭环前，不进行深度 Studio reintegration。**

---

# 36. 第一版 Analysis Engine 完整闭环门槛

必须：

```text
[ ] Runtime Manager truthful status/resolve

[ ] original_mix -> guide_vocals
[ ] guide_vocals -> lead_vocal
[ ] original_mix -> production instrumental independently

[ ] Qwen ASR real
[ ] ForcedAligner real
[ ] RMVPE real
[ ] GAME real

[ ] Candidate boundaries
[ ] Fusion
[ ] HSMM
[ ] Candidate VocalChart 0.3
[ ] PitchEvidence 0.3

[ ] duet/polyphony uncertainty
[ ] cleanup damage handling
[ ] cancellation
[ ] long-song stability
[ ] fingerprint/provenance
[ ] output confinement
```

---

# 37. 最终不可破坏的边界

```text
UTZ
    = what the data means

Runtime Manager
    = what verified resources can run

Analysis Engine
    = how this audio is analyzed

Studio
    = what the user wants to do with the result
```

实现过程中任何代码迁移都必须能回答：

```text
这个职责属于哪一层？
```

如果答案是两层同时拥有，通常说明边界正在重新耦合。
