# Uta Analysis Engine — 音频分析框架 v2.1 RC

**状态**：Architecture Baseline / Release Candidate Design
**日期**：2026-08-22
**范围**：音频拆分、歌词转写、强制对齐、连续音高、音符转写、演唱技法、多专家融合、候选唱谱生成、Standalone Engine
**目标平台**：Windows / Linux；Intel Arc / Intel Xe 优先，同时保留 AMD / NVIDIA 可移植路径
**生产原则**：Native-only；Python / PyTorch / Transformers 只允许用于模型转换、ONNX 导出、离线 benchmark/parity 验证，不进入正式运行链路；Engine 不承担训练或自我进化
**格式标准源**：`https://github.com/bintis/utz`
**产品集成目标**：`bintis/uta-studio@native-inference`

---

# 0. 本文档取代什么

本文是对此前以下设计的统一重构与合并：

```text
UTA_STUDIO_AUDIO_ANALYSIS_FUSION_DESIGN_v1.0_FINAL
UTA_STUDIO_SINGING_ANALYSIS_DETAILED_DESIGN_v1.0_FINAL
UTA_SINGING_ENGINE_AUDIO_ARCHITECTURE_INPUT_CONTRACT_v1.0_FINAL
UTA_SINGING_ENGINE_AUDIO_SEPARATION_PLAN_v1
```

旧文档中的以下内容继续保留：

- RMVPE 作为主要连续 F0 传感器；
- GAME 作为主要唱声 Note Boundary / Region Expert；
- Basic Pitch 作为独立 onset / note / contour 证据；
- ROSVOT 作为第二唱声专用专家；
- DSP + STARS native candidate 路线；
- Qwen3-ASR + Qwen3 ForcedAligner；
- calibrated confidence；
- context-aware dynamic weighting；
- disagreement-window escalation；
- HSMM / Viterbi 全局解码；
- vibrato / glissando / melisma 特殊处理；
- Gold Validation Set 与模型升级回归门禁；
- Native Runtime 与本地 worker。

本版重新定义或修正：

- Studio / Analysis Engine / Runtime Manager / UTZ 的系统边界；
- UTZ 0.3 的最新冻结契约；
- Audio Separation Plan v1；
- duet / multi-singer 的真实能力边界；
- `guide_vocals`、`lead_vocal`、`clean_lead_vocal` 的区别；
- Engine Input Contract 与 UTZ Package Contract 的区别；
- Candidate / Authored 的所有权；
- Standalone Engine 的模型与运行时行为；
- `representations` 与 `extensions` 的职责；
- Engine outer/inner DAG 关系；
- 失败、降级、fingerprint、provenance、quality gate。

---

# 1. 最终系统分层

整个系统固定为四个主要组件：

```text
                    ┌─────────────────────┐
                    │       UTZ 0.3       │
                    │ format / schema /   │
                    │ interoperability    │
                    └─────────┬───────────┘
                              │
             ┌────────────────┼─────────────────┐
             │                │                 │
             ▼                ▼                 ▼
   Runtime Manager     Analysis Engine       Uta! Studio
      models +             execution           product
      runtimes               plane            control
```

准确的依赖关系：

```text
UTZ
 ↑                  ↑
 │                  │
Analysis Engine    Uta! Studio

Runtime Manager
        ↑             ↑
        │             │
        └──── shared ─┘
```

Runtime Manager 不需要拥有 UTZ 语义。

---

# 2. 四个组件分别拥有什么

## 2.1 UTZ

UTZ 是 Uta 生态的领域交换标准。

它拥有：

- ZIP package；
- manifest；
- VocalChart；
- PitchEvidence；
- audio / visuals / representations / extensions；
- schema；
- validator；
- conformance fixtures；
- deterministic writer；
- integrity rules；
- version rules。

UTZ 不拥有：

- AI 模型；
- 推理算法；
- Studio Artifact DB；
- 模型下载器；
- UI；
- Candidate 生成策略；
- 游戏评分算法实现。

---

## 2.2 Runtime Manager

Runtime Manager 是模型与运行时生命周期的唯一真相源。

负责：

```text
ModelCatalog
RuntimeCatalog
RuntimeRecipe
install
remove
repair
verify
SHA-256
license/source metadata
backend compatibility
device compatibility
readiness
resolve
```

Runtime Manager 不执行唱声分析。

它回答：

> 某个 Engine capability 所需要的模型与 runtime 是否已经准备好？路径、hash、recipe digest 是什么？

---

## 2.3 Uta Analysis Engine

Analysis Engine 是 execution plane。

定义：

> 给 Engine 明确的音频身份、语义、时间线和可选歌词约束，Engine 负责所有模型相关预处理、推理、证据融合与候选唱谱生成。

Engine 负责：

- decode；
- source verification；
- timeline mapping；
- separation；
- lead isolation；
- denoise / dereverb；
- ASR；
- Forced Alignment；
- F0；
- note transcription；
- technique evidence；
- expert calibration；
- candidate graph；
- HSMM / Viterbi；
- rhythm quantization；
- confidence / uncertainty；
- Candidate VocalChart；
- PitchEvidence；
- SingingAnalysis extension；
- requested stems；
- Standalone USTX/MIDI/etc export。

Engine 不负责：

- 曲库；
- 项目数据库；
-用户编辑；
- Undo / Redo；
- Artifact lifecycle ownership；
- Candidate → Authored 决策；
- UI；
- 模型自动下载；
- HTTP 服务。

---

## 2.4 Uta! Studio

Studio 是 control plane + product layer。

负责：

- Library；
- Source authorization；
- Processing Studio；
- outer DAG；
- queue；
- retry/cancel；
- Artifact DB；
- revisions；
- lineage；
- cache；
- freeze / bypass；
- Candidate / Authored；
- editor；
-播放；
- export UX；
- Models & Runtime GUI；
- explicit model installation。

核心规则：

```text
Studio 决定：
装什么
跑什么
什么时候跑
哪些结果复用
最终采用什么

Engine 决定：
具体怎么跑
用什么 recipe
怎么预处理
怎么推理
怎么融合
```

---

# 3. Engine 的一句话定义

> **Uta Analysis Engine turns explicitly identified audio and optional lyric constraints into analysis-ready stems and UTZ-compatible candidate singing structure. It owns model-dependent preprocessing, inference and fusion; it does not own libraries, authoring state, model acquisition, or product workflow.**

中文：

> **给 Engine 音频、明确的音频语义、可选歌词与结构约束；Engine 输出可编辑的唱声结构、连续音高证据、分析证据与请求的音频 stem。**

---

# 4. Engine 独立使用的定位

Standalone Engine 类似：

```text
ffmpeg
whisper.cpp
Vocal2Midi
```

而不是第二个 Uta! Studio。

典型场景：

```text
single-file transcription
batch dataset generation
known-lyrics alignment
vocal-stem analysis
benchmark
offline benchmark / regression dataset generation
DAW/OpenUtau integration
other local apps
```

示意：

```bash
uta-analysis-engine analyze song.flac \
  --profile balanced \
  --output song.ustx
```

或者：

```bash
uta-analysis-engine analyze lead.wav \
  --input-role lead_vocal \
  --output song.mid
```

Standalone Engine 不包含：

```text
music library
project DB
large editor
playlist
cover management UI
```

---

# 5. Runtime / Model 管理边界

Engine 独立使用时也不应要求用户自行研究模型目录与 runtime 组合。

推荐发行关系：

```text
uta-runtime
  setup
  models
  verify
  repair
  status

uta-analysis-engine
  analyze
  export
  doctor
```

`uta-analysis-engine doctor` 可以调用 Runtime Manager，但不能实现第二套安装逻辑。

明确禁止：

```text
analyze 时发现缺模型
-> Engine 偷偷联网下载
```

必须：

```text
missing model
-> model_unavailable
```

由 Studio 或 `uta-runtime setup` 显式处理。

---

# 6. Engine Contract 与 UTZ Contract 不是一回事

Engine 请求协议：

```text
uta.singing-engine.request
```

用于执行。

UTZ：

```text
uta.song / 0.3.x
```

用于交换与打包。

Engine 请求可以有：

```text
original_mix
vocal_stem
clean_lead_vocal
```

这些执行语义。

而 UTZ audio standard roles 是：

```text
instrumental
guide_vocals
original
lead_vocal
backing_vocal
harmony_vocal
```

所以需要显式映射，而不是强迫两套 role 完全相同。

---

# 7. UTZ 0.3 固定基线

当前框架假定 UTZ 0.3 的核心规则已经冻结。

## 7.1 Package

```text
format = uta.song
format_version = 0.3.x
```

pre-1.0 不做 0.1 / 0.2 runtime compatibility。

---

## 7.2 Canonical Timebase

所有 Uta-owned canonical timeline：

```text
1 second = 1,000,000 units
```

包括：

```text
song.duration
VocalChart timebase
PitchEvidence timebase
visual timing
Engine wire time
boundary constraints
analysis result timeline
```

禁止 canonical wire protocol 使用 float seconds。

---

## 7.3 VocalChart

当前开发线：

```text
format = uta.vocal-chart
format_version = 0.3.x
feature = vocal-chart/0.3
media type = application/vnd.uta.vocal-chart+json;version=0.3
```

它是权威音乐唱谱。

---

## 7.4 PitchEvidence

当前开发线：

```text
format = uta.pitch-evidence
format_version = 0.3.x
feature = pitch-evidence/0.3
media type = application/vnd.uta.pitch-evidence+json;version=0.3
```

它是连续物理证据，不是评分谱。

---

## 7.5 Audio

UTZ：

```text
audio.assets
  role -> AssetRef
```

标准 role：

```text
instrumental
guide_vocals
original
lead_vocal
backing_vocal
harmony_vocal
```

所有 audio assets：

- 与 instrumental 共享 time zero；
- 共享 song timeline；
- 不允许隐藏 offset；
- 不允许 time-stretch / warp。

---

## 7.6 Visuals

```text
visuals.assets
  role -> AssetRef

visuals.timing
  role -> { timebase, offset }
```

标准 role：

```text
cover
background
video
thumbnail
```

---

## 7.7 Representations

```text
representations
  id -> AssetRef
```

用于外部格式表示：

```text
midi
kar
ust
ustx
musicxml
ultrastar
lrc
srt
```

规则：

```text
VocalChart = authoritative
representations = alternate / derived serialization
```

representation 不参与 feature negotiation。

---

## 7.8 Extensions

`extensions` 用于新的结构化领域：

```text
singing-analysis/0.3
tempo-map/0.3
presentation/0.3
chords/0.3
```

每个 extension key 必须且只能属于：

```text
required_features
或
optional_features
```

例如：

```text
singing-analysis/0.3
```

默认应是：

```text
optional_features
```

---

# 8. Candidate 与 Authored

Engine 生成：

```text
Candidate VocalChart
```

不是：

```text
Authored VocalChart
```

Studio 保存的 authored chart 拥有最终用户意图。

规则：

```text
Engine MAY generate candidate
Engine MUST NOT overwrite authored truth
```

Studio 可以：

```text
compare
merge
accept
replace
edit
commit
```

Standalone Engine 虽然可以直接导出 Candidate，但 provenance 应保留生成来源。

---

# 9. Analyze 与 Export 必须分离

分析请求只需要：

```text
audio
constraints
profile
requested analysis artifacts
```

UTZ export 还需要：

```text
title
artist
package_id
revision
instrumental
optional cover/video
destination
representation choices
```

所以：

```text
AnalyzeRequestV1
!=
ExportRequestV1
```

Standalone CLI 可以提供便利：

```text
analyze + export
```

但 core contract 不应把两者混成一个请求。

---

# 10. 一个重要的 UTZ 限制

UTZ 0.3 package 必须拥有：

```text
audio.assets.instrumental
```

因此：

```text
只给 lead_vocal.wav
```

Engine 可以：

- 转写；
- 对齐；
- F0；
- Note；
- Fusion；
- 生成 Candidate VocalChart；
- 导出 USTX；
- 导出 MIDI；
- 导出 loose analysis assets。

但是不能凭空构造合法 UTZ。

合法 UTZ 需要：

- caller 另外提供 instrumental；
- 或从 original_mix 生成 instrumental。

禁止：

```text
缺 instrumental
-> 塞静音伴奏
```

---

# 11. Input Contract v1

建议继续使用：

```text
AnalyzeRequestV1
├── contract
├── request_id
├── audio_sources[]
├── lyric_constraints?
├── boundary_constraints[]
├── musical_context?
├── analysis
├── requested_artifacts
├── execution_policy?
└── extensions
```

---

# 12. 输入契约原则

## 12.1 Caller 提供身份与语义

Caller 必须提供：

```text
path
sha256
role
primary
source_start
```

Caller 不负责提供权威：

```text
sample rate
channels
codec
decoded duration
frame count
```

这些由 Engine decode 后确认。

---

## 12.2 一个 primary source

v1 每个 AnalyzeRequest：

```text
exactly one primary=true
```

其他 source 可作为：

```text
instrumental reference
known vocal stem
secondary material
```

但不能出现两个互相竞争的主 timeline。

---

## 12.3 本地文件

Worker v1 只允许：

```text
local_file
```

禁止：

```text
HTTP URL
directory scan
implicit discovery
```

---

## 12.4 Source identity metadata

兼容 wire 中的 `sha256` 字段承载 caller 已持久化的 source identity / provenance metadata。Engine 不在 decode 前重新计算或比较 source hash，也不以 hash 一致性作为接受/拒绝 gate。

仍然必须验证：授权本地路径、普通文件、路径 confinement、可解码性、声明的语义角色、canonical timeline 与 decoded facts。

---

# 13. Audio input roles

Engine execution roles：

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

其中：

### `original_mix`

完整歌曲 mix。

### `vocal_stem`

已与伴奏分离，但可能包含：

```text
lead
backing
harmony
adlib
multiple singers
```

### `guide_vocals`

完整人声参考。

### `lead_vocal`

主要 foreground singing。

### `clean_lead_vocal`

分析工作 stem，允许经过分析目的 cleanup。

### `instrumental`

不能单独作为唱声分析 primary。

---

# 14. Timeline Contract

每个 AudioSource：

```json
{
  "timeline": {
    "timebase": 1000000,
    "source_start": 32100000
  }
}
```

含义：

```text
local audio time 0
=
song/source timeline 32.1 s
```

v1 允许：

```text
crop
resample
channel conversion
codec conversion
```

只要保持 1:1 elapsed time。

v1 禁止未声明的：

```text
time stretch
variable speed
tempo warp
non-linear time mapping
```

所有输出必须映射回 source/song timeline。

---

# 15. DecodedAudioFacts

Engine decode 后必须产生事实记录：

```text
container
codec
sample_rate
channels
frame_count
duration
peak
decode_backend
```

这些不是 caller 可覆盖的语义。

它们进入：

```text
diagnostics
provenance
analysis fingerprint
```

---

# 16. Lyrics Contract

三种模式：

```text
none
reference
canonical
```

## `none`

没有已知歌词。

Engine 可：

```text
ASR
或
no-lyrics note path
```

取决于 profile/request。

---

## `reference`

用户提供参考歌词，但不保证完全正确。

允许：

```text
ASR transcript
+
reference text
+
sequence / DTW matching
```

修正文本 identity。

必须保证：

> 文本修正不能破坏已经由声学证据得到的 note geometry / melisma。

---

## `canonical`

caller 声明文本权威。

Engine：

- 可以做 reading；
- 可以做 phonemes；
- 可以做 alignment；
- 不可以替换显示文本。

---

# 17. Lyric token identity

每个歌词 token 应具有稳定 request-local / chart-local ID：

```json
{
  "id": "lyric-001",
  "text": "爱",
  "reading": null,
  "phonemes": null
}
```

Melisma 后续 note 通过 continuation 引用原 token ID。

---

# 18. BoundaryConstraintV1

结构示例：

```json
{
  "token_id": "lyric-001",
  "level": "word",
  "start": 12000000,
  "duration": 800000,
  "confidence": 0.96,
  "authority": "soft",
  "source": "qwen3-forced-aligner"
}
```

level：

```text
phrase
word
syllable
phoneme
```

authority：

```text
soft
hard
```

---

# 19. soft / hard boundary 的语义

`soft`：

- 模型 evidence；
- 可以在 Fusion 中被其他强声学证据推翻；
- 进入 boundary prior。

`hard`：

- 人工确认；
- locked structure；
- 形成不可跨越的结构 barrier。

重要：

> hard boundary 不等于“这个位置一定开始一个新音符”。

一个 lyric span 内仍允许多 note / melisma。

---

# 20. MusicalContext

可选：

```text
BPM
key
time signature
authority
```

它是 rhythm / symbolic prior。

不能覆盖强声学证据。

---

# 21. AnalysisSpec

稳定产品级参数：

```json
{
  "profile": "balanced",
  "track_target": "lead",
  "enable_quantization": true,
  "preserve_continuous_pitch": true
}
```

不要暴露稳定 API：

```text
GAME threshold
RMVPE internal batch
RoFormer FFT size
OpenVINO device string
specific model filename
```

这些属于 Runtime Recipe / Execution Policy。

---

# 22. Engine Inner Plan 与 Studio Outer DAG

Studio outer DAG：

```text
Original
   |
   +--> Instrumental
   |
   v
Vocal Separation
   |
   v
Lead Isolation
   |
   v
Transcript
   |
   v
Alignment
   |
   v
Pitch / Notes
   |
   v
Fusion
   |
   v
Candidate VocalChart
```

Engine inner plan：

```text
decode
resample
RoFormer recipe
Qwen frontend
Qwen ASR
Qwen aligner
RMVPE frontend
GAME frontend
expert reruns
calibration
fusion
HSMM
quantization
```

原则：

> Studio DAG 表达产品 artifact dependency；Engine Plan 表达算法执行 dependency。

两者不能混为一个模型图。

---

# 23. Engine capability registry

Engine 是音频分析 capability 的真相源。

示例：

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
notes.game
notes.basic_pitch
notes.rosvot
pitch.secondary

technique.analyze

fusion.singing
rhythm.quantize
```

Studio Processing Studio 读取 capabilities，不再维护第二份模型节点真相。

---

# 24. Audio Separation Plan v1

音频拆分不定义成：

```text
mix -> vocal + instrumental
```

而定义成两个不同优化目标：

```text
Karaoke playback
    -> high-quality instrumental

Singing analysis
    -> analysis-ready lead
```

不能为了减少一次 inference 强行把两条路径合并。

---

# 25. Separation semantic artifacts

## `instrumental`

面向：

```text
karaoke playback
UTZ export
```

优化：

```text
low vocal leakage
low musical damage
preserve instruments
```

---

## `guide_vocals`

完整原唱人声参考。

允许：

```text
lead
backing
harmony
double
adlib
multiple singers
```

---

## `lead_vocal`

foreground / primary singing stem。

目标：

```text
reduce support vocals
preserve audible musical vocal
avoid aggressive cleanup
```

它可以进入 UTZ：

```text
audio.assets.lead_vocal
```

---

## `clean_lead_vocal`

Engine 内部分析工作 stem：

```text
lead_vocal
-> optional denoise
-> optional dereverb
-> analysis normalization
```

默认不作为 UTZ standard audio role。

---

## `vocal_residual`

内部 residual。

不能直接声称：

```text
backing_vocal
```

只有通过 quality/classification 才能提升语义。

---

# 26. Separation 标准路径

`original_mix`：

```text
                         Original Mix
                              |
                  Decode / canonicalize
                              |
             +----------------+----------------+
             |                                 |
             v                                 v
     Vocal extraction                   HQ instrumental
             |                                 |
             v                                 v
       guide_vocals                       instrumental
             |
             v
       Lead isolation
             |
        +----+----------+
        |               |
        v               v
    lead_vocal      vocal_residual
        |
        v
      cleanup
        |
        v
 clean_lead_vocal
        |
        v
 singing analysis
```

---

# 27. 当前 RoFormer Recipes

当前推荐实现：

```text
BS-RoFormer Vocals EP317
    -> audio.extract_vocals

MelBand-RoFormer Inst V2
    -> audio.extract_instrumental

MelBand-RoFormer Karaoke
    -> audio.lead_isolate

MelBand-RoFormer Denoise
    -> audio.denoise

MelBand-RoFormer Dereverb
    -> audio.dereverb
```

但公共 contract 只认识 capability。

模型可替换。

---

# 28. 输入已是 stem 时的路由

| Input role | Engine path |
|---|---|
| `original_mix` | full separation |
| `vocal_stem` | lead isolation + cleanup |
| `guide_vocals` | lead isolation + cleanup |
| `lead_vocal` | cleanup |
| `clean_lead_vocal` | direct analysis |
| `instrumental` | secondary/reference only |

绝不能因为用户提供：

```text
lead_vocal
```

又重新执行 vocals extraction。

---

# 29. Production instrumental 必须独立优化

默认禁止：

```text
original_mix - analysis_lead
-> production instrumental
```

推荐：

```text
original_mix
-> dedicated HQ instrumental recipe
-> instrumental
```

分析路径独立：

```text
original_mix
-> vocals
-> lead
-> cleanup
```

---

# 30. Separation Quality Gates

至少包含：

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

每个 gate 应产生结构化 evidence，不是只有 true/false。

---

# 31. Lead Purity Gate

可综合：

```text
multi-F0/polyphony
Basic Pitch simultaneous activity
foreground/residual correlation
F0 stability
speech dominance
separator consistency
```

结果：

```text
high
medium
low
```

行为：

```text
high
-> normal downstream

medium
-> disagreement escalation

low
-> degraded / unresolved
```

不能把低纯度 lead 当成正常 monophonic input。

---

# 32. Cleanup Safeguard

Denoise / Dereverb 可能伤害：

```text
breath
rasp
soft consonant
vibrato sidebands
ornament onset
room tail
```

Balanced / Maximum 应保留：

```text
lead_vocal
clean_lead_vocal
```

进行一致性检查。

如果 cleanup 导致：

```text
onset
voicing
pitch contour
```

显著变化：

```text
cleanup_damage_suspected
```

受影响窗口回退 raw lead。

---

# 33. VocalTopologyEstimate

内部 evidence：

```text
VocalTopologyEstimate
├── mode
├── confidence
├── overlap_regions[]
└── support_regions[]
```

mode：

```text
single_lead
alternating_multi_lead
overlapping_multi_lead
lead_with_support
unknown
```

当前 Engine 实现由 `audio-quality-gates-v2` 在任何 GAME/F0/Fusion 结果被当作可信单旋律结果之前生成这项 evidence。它使用独立解码的 lead 与、若 Harmony 路由存在、`vocal_residual` 的短窗 profile；不会从 `singing.is_some()` 推断 foreground 或 topology。未分离的 caller vocal 输入以及证据不足都必须输出 `unknown`、降低结果状态，并把受影响范围写入 typed review regions。当前 heuristic 未经校准，所以 `confidence` 保持 `null`。

separator 的 `vocal_residual` 本身不是独立 singer/foreground identity evidence。仅凭 lead/residual 能量交替的窗口仍输出 `lead_with_support` foreground/support ambiguity 并降级；`alternating_multi_lead` 保留给未来具备合格 part/identity evidence 的 expert，不能由 residual 自动升级。

`overlap_regions[]` / `support_regions[]` 使用 canonical integer timeline。Engine 通过当前 evaluation context 约束普通 gate region，并保持既有 report v1 wire shape（旧 `audio-quality-gates-v1` report 仍可读取）；Studio 再验证唯一 primary source、canonical timebase、Plan source-route/role binding，并以 app-owned source duration 与请求的 primary-source start 独立约束范围，而不信任 Engine 自报 duration 来放宽范围。所有普通 gate region 和 topology region 都必须有序、无重叠并位于 canonical source range 内。它们可以标记需要复核或 challenger evidence 的范围，但不会自动命名 Singer A/B，也不会生成 BackingVocal/HarmonyVocal stem；`audio.lead_partition` 仍是 future/optional capability。

---

# 34. Duet：交替唱

例如：

```text
A: ██████        ██████
B:       ███████       ███████
```

不要求 source separation。

可以：

```text
shared lead_vocal
+
lyrics / alignment
+
part constraints / singer identity evidence
```

在时间窗口内进行 monophonic analysis。

---

# 35. Duet：同时唱

例如：

```text
A: ███████████
B: ███████████

A = C4
B = E4
```

这是 polyphonic foreground。

基础 RMVPE / GAME 不能可靠同时表达两条 F0/note line。

v1 行为：

```text
detect overlap
-> mark unresolved/polyphonic
-> no fake certainty
```

---

# 36. lead_isolate 与 lead_partition

```text
audio.lead_isolate
```

回答：

> foreground singing vs support vocals。

```text
audio.lead_partition
```

回答：

> 多个同时 foreground singer 是否能拆成多个 analysis streams。

v1：

```text
lead_isolate   baseline
lead_partition optional / future
```

---

# 37. Duet 的 Candidate VocalChart 行为

## 37.1 有可靠 part/singer 约束

如果：

- reference lyrics 已标 part；
- Studio 有 authored singer boundaries；
- lead_partition / identity evidence 足够可靠；

Engine 可生成多个 Candidate track：

```text
track A
  role=lead
  part=1

track B
  role=lead
  part=2
```

---

## 37.2 只有交替多歌手但无法可靠识别身份

不要猜：

```text
Singer A
Singer B
```

可以输出：

- `part=null` 的 lead track；
- VocalTopologyEvidence；
- unresolved singer assignment evidence。

由 Studio 后续 authoring 处理。

---

## 37.3 同时多歌手且无法 partition

Engine 不应伪造第二条 note track。

可：

- 输出 dominant/primary candidate line；
- 明确标记 overlap region incomplete；
- 在 `singing-analysis/0.3` 保存 unresolved polyphony；
- confidence 下调。

---

# 38. Speech Runtime

推荐 worker：

```text
qwen-speech-worker
├── Qwen3-ASR-1.7B
└── Qwen3-ForcedAligner-0.6B
```

同一个进程。

支持内部 mode：

```text
transcript-only
align-known-text
transcribe-and-align
```

逻辑 Artifact 仍然保持：

```text
Transcript
Alignment
```

即使物理执行合并。

---

# 39. Transcript Experts

基线：

```text
Qwen3-ASR 1.7B
```

可选 transcript expert：

```text
FireRedASR2-AED
Whisper Large-v3
```

定位：

- Qwen：mandatory native baseline；
- FireRedASR2：中文/方言/唱声 challenger；
- Whisper：兼容/备选 expert。

不建立 `qwen_*` public Studio API。

---

# 40. Transcript Fusion

reference lyrics 模式下：

```text
ASR
+
reference text
+
sequence matching / DTW
```

目标是修正：

```text
text identity
```

而不是重新决定：

```text
note geometry
```

关键规则：

> lyric identity 和 note geometry 是相互约束但独立的数据层。

---

# 41. Forced Alignment

Qwen ForcedAligner 输出：

```text
word/token boundaries
confidence
```

进入：

```text
BoundaryConstraint
```

默认 authority：

```text
soft
```

人工 lock 才是：

```text
hard
```

---

# 42. RMVPE — Primary Continuous F0 Sensor

RMVPE 只负责：

```text
continuous F0
voicing
confidence
```

不负责最终 Note segmentation。

推荐：

```text
16 kHz mono
host-side mel frontend
OpenVINO native inference
```

每帧：

```text
f0_hz
voiced
confidence
```

连续 MIDI：

```text
m = 69 + 12 * log2(f0 / 440)
```

不能逐帧 nearest-MIDI 量化成最终音符。

---

# 43. PitchEvidence 0.3

最终连续曲线进入：

```text
PitchEvidence 0.3
```

典型结构：

```text
timebase = 1_000_000
start
hop
frequency_hz[]
confidence[]
model
```

它是：

```text
editor evidence
continuous performance evidence
```

不是：

```text
target scoring note track
```

---

# 44. GAME — Primary Singing Note Expert

GAME 负责：

```text
note region
boundary
base pitch
voicing/presence
note confidence
```

歌词 timing 可通过 known durations / boundaries 约束。

参考 Vocal2Midi 已验证：

```text
Alignment
-> known durations
-> GAME dur2bd / known boundaries
-> constrained segmentation
```

这条路线应保留。

Production baseline：

```text
official GAME ONNX
-> OpenVINO C++
```

GAME-rs 可作为未来 backend candidate。

---

# 45. Basic Pitch — Independent Cross-check

输出：

```text
onset [T,88]
note [T,88]
contour [T,264]
```

它提供：

```text
onset evidence
note occupancy
pitch-bend/contour evidence
generic AMT disagreement
```

不作为唱声 primary model。

Balanced 主要跑：

```text
disagreement windows
```

Maximum 可扩大覆盖。

---

# 46. ROSVOT — Secondary Singing Expert

ROSVOT 的价值：

- singing-specific transcription；
- word boundary predictor；
- RMVPE conditioning；
- external boundary/duration conditioning。

定位：

```text
second singing-specific expert
```

初期：

```text
optional
Maximum
offline teacher
```

直到 native/ONNX runtime 足够可靠再升级生产权重。

---

# 47. Secondary F0 Expert

可加入：

```text
FCPE
```

或等价 lightweight F0 expert。

主要用途：

```text
octave disagreement
voicing disagreement
dirty-separation disagreement
```

而不是替代 RMVPE。

---

# 48. Technique Expert

第一阶段优先纯 DSP：

```text
vibrato
glissando
portamento
ornament
breath/voicing transitions
```

STARS 作为后续 native expert candidate：

```text
official STARS CKPT
-> inference-only export wrapper
-> ONNX
-> numerical parity
-> OpenVINO IR
-> technique / note / alignment evidence
```

不要把整个上游 Python inference program 强行导成单个 ONNX。

应拆分：

```text
neural graph
+
native deterministic postprocess
```

Python-side boundary regulation、`.item()` 驱动的动态 shape、DP/Viterbi、TextGrid/MIDI 写出等逻辑应留在 Rust/C++ host。

如果 STARS OpenVINO 性能和 parity 足够好，就直接作为 Maximum optional expert；不再把额外学生模型蒸馏作为产品路线。

---

# 48.1 STARS Native Conversion Strategy

STARS 的 `.ckpt` 不视为部署障碍。

推荐 conversion spike：

```text
official CKPT
-> exact upstream config/state_dict
-> PyTorch golden intermediate tensors
-> inference-only ONNX subgraphs
-> ONNX Runtime parity
-> OpenVINO CPU parity
-> OpenVINO GPU parity
-> native semantic postprocess parity
```

优先拆分 neural heads，而不是导出整个 Python 程序：

```text
shared acoustic/backbone features
phoneme/boundary head
note-boundary head
pitch/note head
technique head
style head
```

最终是否进入 production 取决于：

```text
numerical parity
semantic parity
latency/memory
full-song stability
Intel Arc/Xe validation
license audit
```

STARS 不得仅因 CKPT 成功导出而自动升级。当前仓库已通过显式的 repository-owner catalog release policy 将其有效非 CPU 路线纳入 `ProductionPinned`；数值/语义 parity、运行稳定性、provenance、license 与广泛质量限制仍必须作为可见证据或 advisory caveat 保留，安装、runtime、结构与输出校验继续 fail-closed。

---

# 49. Evidence Timeline

所有 expert evidence 投影到 canonical timeline。

推荐内部逻辑 resolution：

```text
~10 ms
```

但 wire/artifact 时间仍使用整数：

```text
1_000_000 units/s
```

Evidence 可能包含：

```text
F0
voicing
onset
offset
note-region
base-pitch candidate
alignment boundary
polyphony
technique
cleanup quality
separator quality
```

---

# 50. Confidence 不能直接比较 raw score

禁止：

```text
GAME 0.9
vs
Basic Pitch 0.8
=> GAME 更可信
```

必须做模型/任务特定 calibration。

建议：

```text
temperature scaling
isotonic regression
Platt-style calibration
reliability curves
```

输出统一：

```text
calibrated probability / confidence
```

---

# 51. Correlation Discounting

多个模型不是独立证据。

例如：

```text
GAME 与 Vocal2Midi pipeline
RMVPE 与 ROSVOT 中 RMVPE-derived features
多个相同 separator variants
```

存在相关性。

Fusion 需要：

```text
effective expert weight
=
calibrated weight
*
correlation discount
```

防止同源证据重复投票。

---

# 52. Context-aware Dynamic Weights

不存在一个永久 global winner。

例如：

### Vibrato

提高：

```text
RMVPE continuous contour
Technique DSP
```

降低：

```text
frame-level semitone transitions
```

### Glissando

提高：

```text
continuous F0 shape
duration evidence
boundary evidence
```

降低：

```text
intermediate semitone occupancy
```

### Fast melisma

提高：

```text
GAME boundary
Basic Pitch onset
Alignment lyric span
```

### Dirty separation

提高：

```text
separator disagreement evidence
cleanup consistency
secondary F0
```

---

# 53. Vibrato 规则

Vibrato：

```text
同一个 note
+
周期性 cents deviation
```

不应该因为跨过半音边界就拆成多个 note。

建议分析：

```text
vibrato rate
vibrato extent
periodicity
center pitch
```

最终：

```text
base note stable
continuous pitch curve retains vibrato
```

---

# 54. Glissando / Portamento

规则：

> glissando 中跨过的中间半音默认是 pitch bend，不是新音符。

只有出现：

```text
stable plateau
strong onset
boundary expert evidence
duration evidence
lyric/articulation evidence
```

才考虑新 note。

---

# 55. Melisma

歌词和 note 是：

```text
1 lyric token
-> multiple notes
```

不是强制：

```text
1 word = 1 note
```

UTZ VocalChart continuation 语义直接表达：

```text
first note:
  Text(token)

following notes:
  Continuation(continuation_of=token)
```

这也是 Vocal2Midi 已经验证的路线。

---

# 56. Candidate Boundary Generation

候选 boundary 来源：

```text
GAME
ROSVOT
Basic Pitch onset
Alignment
F0 discontinuity
voicing transitions
DSP articulation
hard user constraint
```

形成：

```text
boundary candidate set
```

而不是直接把某个 expert 的边界当最终答案。

---

# 57. Segment Pitch Candidates

对候选 segment：

```text
[start, end)
```

计算：

```text
robust median F0
weighted cents center
pitch stability
voicing ratio
expert note candidates
```

得到有限 pitch state：

```text
MIDI n
MIDI n±1
octave alternatives
unpitched states
```

---

# 58. Octave Correction

禁止 blanket：

```text
f0 * 2
或
f0 / 2
```

Octave correction 必须基于上下文：

```text
RMVPE vs secondary F0
GAME base pitch
Basic Pitch
neighboring notes
vocal range
transition cost
```

作为候选 state 由全局 decoder 决定。

---

# 59. Candidate Graph

节点可表示：

```text
segment boundary
pitch state
vocal mode
lyric association
technique state
confidence
```

边代表：

```text
timing continuity
pitch transition
duration prior
lyric constraint
voice topology constraint
```

---

# 60. Exact second-order HSMM path decoder

最终 note semantic track 使用有界、确定性的 segment-level second-order dynamic programming，从真实 Candidate Pool 中选择完整路径。

在当前产品设计中，这一节定义 **Algorithm** 决策模式的确定性默认实现。用户显式选择的 **AI judgment** 是同一完整 Candidate Pool（包含规范化 caller-hard boundary set）之上的受约束替代 selector，必须遵守 `UTA_AI_JUDGMENT_FUSION_MODE_v1.0.md`：只能选择真实候选、Runtime Manager 管理 adapter tool、允许显式联网、失败不回退 Algorithm，并保留独立的非确定性决策 provenance。两个 selector 必须经过同一 membership、exact voiced-component coverage、hard-boundary 和 canonical output validation。

完整 Candidate Pool 和解码工作量有精确上限：扩展后最多 `100000` 个 Candidate state；每个 duration state 最多 `64` 个不同 pitch proposal；metadata clone 前、以及 pitch-state 扩展后（包含复制的嵌套证据）均最多 `10000000` 个 Candidate-to-boundary/word/technique evidence relation；通过排序区间索引保守计算的 Candidate-to-local-F0/Acoustic/Basic-Pitch frame visit 最多 `10000000` 个；整个图最多检查 `65536` 个 second-order pair state 和 `2000000` 个 pair transition。AI adapter 请求另有独立的 `8 MiB` 序列化上限。到达上限允许执行，超过一项即 fail closed；这些预算跨全部 disconnected voiced component 累计，不能由不可达或分离的子图绕过。

原因：

- note 是有持续时间的；
- vibrato 不应造成 state chatter；
- glissando 需要 duration-aware transition；
- melisma 需要 boundary-aware segmentation。

目标：

```text
argmin
ObservationCost
+ TransitionCost
+ SecondOrderMelodyCoherenceCost
+ DurationCost
+ ConstraintCost
```

或等价最大后验形式。

---

# 61. Observation Cost

综合：

```text
F0 compatibility
GAME evidence
Basic Pitch note/onset
ROSVOT
alignment
voicing
technique
separator quality
cleanup quality
```

所有输入必须保留 typed measured / unknown 语义；未经校准的跨模型 raw score 不得伪装成可直接比较的概率。

---

# 62. Transition Cost

示例：

### Vibrato

相邻半音频繁切换：

```text
high penalty
```

### Strong onset

新 note transition：

```text
lower penalty
```

### Glissando

若只有连续 F0 滑行：

```text
intermediate note transition high penalty
```

### Caller-authored hard barrier

只有 caller 明确声明为 `Hard` 的规范化 pool-level boundary 才是结构 barrier。跨 barrier：

```text
infinite / forbidden
```

Alignment、onset、voicing transition 等模型/上下文证据可以影响分数或重置 melody prior，但不会自行升级为结构 barrier。

---

# 63. Duration Prior

防止：

```text
极短假 note
```

但不能粗暴抹掉真实 ornament。

duration prior 需要 context：

```text
tempo
onset
melisma
technique
boundary confidence
```

---

# 64. Rhythm Quantization

顺序必须：

```text
先得到 semantic notes
再做 rhythm quantization
```

而不是：

```text
先把 frame/边界吸到 grid
再判断 note
```

输入：

```text
note onset/duration
BPM
time signature
confidence
```

可以采用：

```text
DP
Bayesian timing optimization
```

参考 Vocal2Midi 的 quantization 思路。

---

# 65. Disagreement Windows

核心成本控制机制。

Fast pass：

```text
separation
Qwen
RMVPE
GAME
DSP
baseline fusion
```

触发 disagreement：

```text
F0 experts octave conflict
GAME vs RMVPE pitch mismatch
onset expert conflict
polyphony detected
cleanup changes pitch
separator instability
alignment conflict
low confidence
duet overlap
```

Balanced / Maximum 只对相关窗口升级：

```text
Basic Pitch
ROSVOT
secondary F0
secondary separation
Technique model
consistency rerun
```

---

# 66. Fast / Balanced / Maximum

## Fast

目标：

```text
最低生产成本
稳定 baseline
```

典型：

```text
required separation
Qwen ASR/align as needed
RMVPE
GAME
DSP
baseline Fusion/HSMM
```

---

## Balanced

增加：

```text
secondary F0 disagreement
Basic Pitch disagreement windows
full lead purity
cleanup consistency
vocal topology
richer technique DSP
```

---

## Maximum

增加：

```text
ROSVOT
STARS/Technique expert
secondary separator
optional lead partition
additional ASR challenger
consistency reruns
```

仍然：

> Maximum ≠ 所有专家整首歌无条件运行。

---

# 67. SingingAnalysis extension

机器证据不要塞进 VocalNote。

建议：

```text
feature:
singing-analysis/0.3

extensions:
singing-analysis/0.3
    -> analysis/singing-analysis.json

optional_features:
singing-analysis/0.3
```

包含：

```text
final confidence
uncertain regions
expert evidence
calibration
agreement
alternatives
technique probabilities
vocal topology
separator quality
cleanup quality
fusion version
```

---

# 68. SingingAnalysis 的 note 引用

机器证据通过稳定 ID 引用 VocalChart：

```text
track id
phrase id
note id
lyric token id
```

不要复制第二套 authoritative note timing。

---

# 69. Result Contract

建议：

```text
AnalysisResultManifestV1
├── status
├── candidate_vocal_chart
├── pitch_evidence
├── singing_analysis?
├── transcript?
├── alignment?
├── stems[]
├── diagnostics
├── provenance
├── fingerprint
└── degraded_reasons[]
```

结果文件放 Engine run-temp。

Studio 模式：

```text
Engine writes temp
Studio validates
Studio hashes
Studio semantic-checks
Studio atomically commits
```

---

# 70. Worker Protocol

无 HTTP。

```text
stdin  -> NDJSON commands
stdout -> NDJSON machine frames
stderr -> human logs
```

command：

```text
hello
capabilities
validate
analyze
cancel
export
```

---

# 71. Engine status

建议：

```text
ok
ok_degraded
failed
cancelled
```

---

# 72. Error Contract

稳定错误码：

```text
unsupported_contract_version
invalid_contract
invalid_audio_role
multiple_primary_sources
decode_failed
timeline_invalid
invalid_constraints
missing_required_input
missing_capability
model_unavailable
runtime_unvalidated
runtime_failed
inference_failed
output_validation_failed
cancelled
export_failed
```

---

# 73. Degraded Success

不是所有 expert failure 都要让全任务失败。

例如：

```text
Dereverb failure
ROSVOT unavailable
Basic Pitch disagreement rerun failure
```

如果 baseline still valid：

```text
ok_degraded
```

并记录原因。

但是：

```text
decode failure
required vocal extraction failure
baseline analysis path failure
invalid output
```

不能伪装成功。

---

# 74. Analysis Fingerprint

必须覆盖：

```text
input SHA-256
contract version
audio role
timeline mapping
constraint hash
AnalysisSpec
model IDs/hashes
RuntimeRecipe digest
backend/device
calibration version
fusion version
postprocess version
quantization version
cleanup recipe
separation recipe
```

用于：

```text
cache
lineage
reproducibility
non-regression
debug
```

---

# 75. Runtime architecture

推荐：

```text
Analysis Engine process
│
├── RoFormer Runtime
│   └── C++ / GGML
│
├── qwen-speech-worker
│   └── qwen3-asr.cpp
│
├── OpenVINO experts
│   ├── RMVPE
│   ├── GAME
│   └── Basic Pitch
│
└── Fusion core
```

具体 process isolation 可按稳定性调整。

Engine contract 不绑定 process topology。

---

# 76. Intel Arc / Xe 优先策略

生产 baseline：

```text
OpenVINO
Vulkan / GGML where validated
CPU fallback where approved
```

所有 fallback 必须：

```text
validated
fingerprinted
visible in provenance
```

不能 runtime 随意切设备又不记录。

RoFormer Intel Arc 需要持续验证：

```text
batch behavior
history reset
long-run stability
denoise/dereverb sustained path
```

短音频成功不等于 production validated。

---

# 77. Native-only 原则

正式发布包：

```text
no Python inference runtime
no PyTorch
no transformers package
no external model CLI dependency
```

Python 仅用于：

```text
conversion
training
evaluation
dataset tooling
offline distillation
```

---

# 78. Production Model Lifecycle — 不做自我进化

Analysis Engine 不训练、不微调、不在线修改任何 production model weights。

明确删除以下产品路线：

```text
online self-training
multi-teacher pseudo-label training
partial pseudo-label promotion
production-time consistency learning
automatic teacher/student distillation
user-analysis-driven fine-tuning
```

原因：

```text
reproducibility
artifact provenance
cache identity
validation simplicity
runtime determinism
upstream model replacement speed
```

生产模型定义：

```text
immutable external model/checkpoint
+
pinned conversion/export recipe
+
pinned runtime recipe
+
validation evidence
```

---

# 79. External Model Upgrade Flow

模型能力升级来自新的 upstream model/checkpoint，而不是 Engine 自己学习。

统一流程：

```text
new upstream model/checkpoint
        ↓
offline benchmark
        ↓
Gold Set / real-song regression
        ↓
ONNX / OpenVINO / GGUF / native conversion
        ↓
numerical parity
        ↓
semantic parity
        ↓
runtime/hardware validation
        ↓
new Runtime Manager catalog recipe
        ↓
explicit user install/reinstall
```

只要 capability contract 不变：

```text
pitch.track
notes.game
notes.stars
speech.transcribe
audio.lead_isolate
```

替换模型不需要改变 Studio workflow 或 Engine public contract。

---

# 80. Robustness Tests — 不用于训练

以下过去可能用于 consistency learning 的变换，现在只作为 regression/robustness tests：

```text
gain
EQ
noise
cleanup variants
chunk offset
codec perturbation
```

目标：

> 相同音乐语义在合理输入扰动下保持稳定。

测试失败产生 regression，不触发任何自动训练。

---

# 81. Pitch-shift Equivariance Regression

如果测试输入明确：

```text
+2 semitone
```

预期：

```text
note pitch ≈ +2 semitone
timing structure ≈ unchanged
lyric identity unchanged
```

这是自动 regression signal，不是训练目标。

---

# 82. Time-stretch Consistency Regression

离线测试允许显式 time-stretch：

```text
pitch/order same
timing scales predictably
```

这仅属于测试工具。

注意这不是 Engine v1 wire contract 的 hidden time warp。

---

# 83. Gold Validation Set

必须有一个小规模人工确认 Gold Set：

```text
never used to mutate production weights
```

覆盖：

```text
stable notes
vibrato
glissando
melisma
rap/spoken
duet
harmony
dirty separation
octave errors
cleanup damage
```

新 upstream model / 新 runtime recipe 只有在：

```text
Gold improves
or
non-regression holds
```

并完成平台/runtime 验证后才可 promote。

---

# 84. Immutable Production Models

禁止：

```text
用户分析一首歌
-> production weights 自动更新
```

也禁止：

```text
后台收集 pseudo-label
-> 自动更新本机模型
```

Production Engine 只使用：

```text
versioned immutable model generations
```

模型升级必须是显式 catalog/runtime lifecycle 事件。

---

# 85. Standalone exports

Engine 可输出：

```text
UTZ
USTX
MIDI
representations
```

但 Analyze 和 Export core contract 分开。

---

# 86. USTX

两种模式：

## Faithful

```text
VocalChart base note
+
PitchEvidence continuous F0
-> pitch deviation curve
```

避免再叠 parametric vibrato，防止 double-vibrato。

---

## Editable

可以尝试：

```text
decompose vibrato parameters
+
residual pitch curve
```

更适合编辑。

---

# 87. MIDI

MIDI 是 lowest-common-denominator representation。

会损失：

```text
continuous expressive F0
uncertainty
expert provenance
technique probability
lyric continuation semantics detail
```

因此不能作为 Engine canonical result。

---

# 88. UTZ representations

Engine standalone 或 Studio export 可以附加：

```text
representations.midi
representations.kar
representations.ust
representations.ustx
representations.musicxml
representations.ultrastar
```

这些都不能覆盖 VocalChart。

---

# 89. Studio API 原则

优先复用现有 Uta! Studio app API：

```text
list_audio_models
get_audio_model_status
install_audio_model
reinstall_audio_model
remove_audio_model

analysis_runtime_status
trigger_setup
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

不要因为内部换成：

```text
Qwen
RMVPE
GAME
RoFormer
```

就新建模型特定 app API。

---

# 90. Engine API 与 Studio API 分层

Studio API：

```text
product commands
```

Engine protocol：

```text
execution commands
```

Native ABI：

```text
runtime/internal
```

三层不能混为一个 registry。

---

# 91. Studio 的模型 UI

Studio：

```text
Settings > Models & runtime
```

本质上是 Runtime Manager frontend。

它不应该维护独立模型真相。

---

# 92. Requested Artifacts

AnalyzeRequest 示例：

```json
{
  "vocal_chart": true,
  "pitch_evidence": true,
  "singing_analysis": true,
  "transcript": true,
  "alignment": true,
  "stems": [
    "instrumental",
    "guide_vocals",
    "lead_vocal"
  ]
}
```

Engine 内部：

```text
clean_lead_vocal
vocal_residual
```

不需要默认成为外部 requested role。

---

# 93. Neutral Candidate Chart Policy

Engine 不做游戏设计决定。

默认：

```text
bonus = normal
```

推荐 scoring：

```text
Pitched    -> pitch
Rap        -> rhythm
Spoken     -> rhythm
Freestyle  -> none
```

Studio/Ruleset 后续可以修改。

---

# 94. VocalChart 与 continuous pitch 分离

目标 note：

```text
VocalChart
```

真实演唱曲线：

```text
PitchEvidence
```

不要把连续 F0 回写成 scoring target。

这条原则是防止：

```text
vibrato
glissando
performance error
```

污染 authored note truth 的关键。

---

# 95. Audio analysis final data flow

```text
                       INPUT CONTRACT
                             |
                             v
                  Decode / Verify / Timeline
                             |
                             v
                  Audio Separation Plan v1
                   /                    \
                  v                      v
        production instrumental      guide vocals
                                         |
                                         v
                                   lead isolation
                                         |
                              +----------+----------+
                              |                     |
                              v                     v
                         lead_vocal           vocal topology
                              |
                              v
                          cleanup
                              |
                              v
                       clean_lead_vocal
                              |
            +-----------------+-------------------+
            |                 |                   |
            v                 v                   v
      Speech/Align       Continuous F0       Note Experts
      Qwen3 ASR          RMVPE + sec. F0     GAME
      ForcedAligner                           Basic Pitch
                                              ROSVOT
            |                 |                   |
            +-----------------+-------------------+
                              |
                              v
                       Canonical Evidence
                              |
                              v
                    Confidence Calibration
                              |
                              v
                  Context-aware Expert Fusion
                              |
                              v
                    Candidate Segment Graph
                              |
                              v
                       HSMM / Viterbi
                              |
                              v
                     Rhythm Quantization
                              |
               +--------------+---------------+
               |              |               |
               v              v               v
          VocalChart     PitchEvidence   SingingAnalysis
            0.3              0.3          extension 0.3
```

---

# 96. Studio integration flow

```text
Uta! Studio
    |
    | choose inputs / artifacts / constraints
    v
compile EnginePlan
    |
    v
Uta Analysis Engine
    |
    v
run-temp results
    |
    v
Studio validation
    |
    v
Artifact DB
    |
    v
Candidate review
    |
    v
Editor
    |
    v
Authored VocalChart
```

---

# 97. Reanalysis 示例

## 改歌词

```text
Transcript/Authored Lyrics changed
    |
    v
Alignment rerun
    |
    v
GAME constrained rerun
    |
    v
Fusion rerun
```

不需要重跑：

```text
instrumental
RMVPE
```

如果 artifact dependency 允许复用。

---

## 只重分析 pitch

```text
Vocal stem frozen
Transcript frozen
Alignment frozen

RMVPE rerun
secondary pitch as needed
GAME/Fusion downstream
```

---

## 只换 separation recipe

```text
Vocal Separation rerun
-> invalidate dependent lead/cleanup
-> downstream analysis rerun
```

Instrumental 是否 downstream 取决于独立 path。

---

# 98. Development Phases

## Phase A — Shared contracts

实现：

```text
AnalyzeRequestV1
AnalysisResultManifestV1
capabilities
worker protocol
canonical time
```

---

## Phase B — Runtime Manager

统一：

```text
catalog
recipe
status
verify
resolve
install/remove
```

---

## Phase C — Separation baseline

实现：

```text
generic RoFormer Runtime
audio.extract_vocals
audio.extract_instrumental
audio.lead_isolate
cleanup
quality gates
```

---

## Phase D — Speech baseline

实现：

```text
Qwen3-ASR
ForcedAligner
Transcript
Alignment
```

---

## Phase E — Vocal2Midi parity

实现：

```text
RMVPE
GAME
known boundary conditioning
melisma
quantization
Candidate VocalChart
```

---

## Phase F — Fusion MVP

加入：

```text
calibration
candidate graph
HSMM
uncertainty
```

---

## Phase G — Secondary experts

加入：

```text
Basic Pitch
secondary F0
ROSVOT
Technique
```

disagreement windows 优先。

---

## Phase H — Multi-singer improvement

研究：

```text
lead_partition
speaker-conditioned separation
multi-F0
singer identity constraints
```

不能阻塞 v1 baseline。

---

# 99. 测试矩阵

至少覆盖：

```text
clean solo
vibrato
glissando
fast melisma
breathy vocal
rap
spoken
octave ambiguity
dirty separation
strong backing vocals
lead + harmony
alternating duet
simultaneous duet
short clips
very long songs
silent regions
clipping
denoise damage
dereverb damage
wrong lyrics
reference lyrics mismatch
canonical lyrics
```

---

# 100. Separation 验收

需要测：

```text
instrumental vocal leakage
instrumental musical damage
lead purity
timeline preservation
cleanup consistency
long-run GPU stability
```

不能只做：

```text
听起来还可以
```

---

# 101. ASR / Alignment 验收

指标至少包括：

```text
text accuracy
word timing error
boundary calibration
reference-lyrics robustness
singing-specific timing
```

---

# 102. Pitch 验收

连续 F0：

```text
voicing F1
raw pitch accuracy
octave error rate
cents error
```

同时单独测试：

```text
vibrato center stability
glissando continuity
```

---

# 103. Note 验收

不能只比较 frame pitch。

至少：

```text
note onset F1
note offset F1
note-with-pitch F1
fragmentation rate
false split rate
false merge rate
melisma correctness
```

特别关注：

```text
vibrato -> false note chatter
glissando -> false semitone notes
```

---

# 104. Confidence 验收

需要 calibration metric：

```text
ECE
reliability diagram
Brier score
```

目标：

> confidence 的数值必须有实际概率意义，而不是装饰。

---

# 105. Duet 验收

分别测：

```text
alternating duet
overlapping duet
lead+support
```

v1 成功标准不是：

> 所有 duet 都能完美分两个人。

而是：

> 能识别什么时候 monophonic 假设不成立，并避免输出虚假确定结果。

---

# 106. Runtime 验收

Windows / Linux：

```text
CPU
Intel Arc
Intel Xe
```

按可用性再验证：

```text
AMD
NVIDIA
```

必须测试：

```text
short run
long run
repeated runs
cancel
retry
worker restart
device loss
memory pressure
```

---

# 107. Production invariants

以下必须作为 code review checklist：

1. Engine 不下载模型。
2. Engine 不修改 source。
3. Studio 不准备 model tensors。
4. Studio 不拥有模型算法 preprocessing。
5. Runtime Manager 不执行分析。
6. Candidate 不覆盖 Authored。
7. PitchEvidence 不覆盖 VocalChart。
8. Representations 不覆盖 VocalChart。
9. Optional extension 可安全忽略。
10. Required extension 必须能 feature-negotiate。
11. 所有 canonical time 使用 1,000,000 units/s。
12. 所有 audio stems 共享 source timeline。
13. 分析结果必须 fingerprint。
14. 降级必须显式。
15. 不建立 HTTP control server。
16. Production 不依赖 Python runtime。

---

# 108. 最终架构摘要

```text
                         UTA! STUDIO
               product / workflow / authoring
                            |
                            v
                       EnginePlan
                            |
                            v
                 UTA ANALYSIS ENGINE
                            |
        +-------------------+--------------------+
        |                   |                    |
        v                   v                    v
 Audio Preparation       Speech              Experts
 & Separation            Qwen                RMVPE
 RoFormer                Aligner              GAME
                                             Basic Pitch
                                             ROSVOT
                                             Technique
        |                   |                    |
        +-------------------+--------------------+
                            |
                            v
                    Evidence Calibration
                            |
                            v
                  Context-aware Fusion
                            |
                            v
                       HSMM Decode
                            |
                            v
                  Candidate Singing Data
             +--------------+---------------+
             |              |               |
             v              v               v
       VocalChart 0.3 PitchEvidence 0.3 SingingAnalysis/0.3
             |
             v
                     Uta! Studio Artifact DB
             |
             v
                     Candidate / Editor
             |
             v
                     Authored VocalChart
```

---

# 109. 最终结论

当前架构不再以：

```text
“Studio 里放几个 AI 模型”
```

来组织。

正式心智模型是：

```text
UTZ
= interoperable domain contract

Runtime Manager
= models + runtimes lifecycle

Analysis Engine
= audio inference + evidence fusion execution plane

Uta! Studio
= workflow + artifact + authoring product
```

唱声分析内部也不再以：

```text
RMVPE
GAME
RoFormer
Qwen
```

这些模型名字作为顶层架构。

顶层架构是：

```text
Semantic Input
-> Audio Preparation
-> Speech / Alignment
-> Continuous Evidence
-> Note Experts
-> Calibration
-> Fusion
-> Global Decode
-> Candidate UTZ-compatible Assets
```

模型只是其中可替换的 Runtime Recipes。

Audio Separation Plan v1 的核心原则：

> **高质量 karaoke instrumental 与 analysis lead 是不同优化问题。**

Fusion 的核心原则：

> **连续真实演唱曲线与离散音乐 note 是不同对象。**

Duet 的核心原则：

> **lead-vs-backing 和 singer-A-vs-singer-B 是不同 separation 问题；v1 必须识别能力边界而不是伪造确定性。**

Studio/Engine 的核心原则：

> **Studio 管“跑什么与使用什么结果”，Engine 管“具体怎么跑”。**

这一版可以作为后续 `uta-analysis-engine`、`uta-runtime-manager` 和 Uta! Studio Processing Studio 集成的统一音频分析架构基线。


---

# Appendix A — 参考项目吸收矩阵

这些项目用于验证路线与吸收设计，不代表生产依赖。

| 项目 | 吸收内容 | 不直接复制的部分 |
|---|---|---|
| Vocal2Midi | `Alignment -> GAME constrained segmentation`、RMVPE continuous F0、melisma continuation、DP/Bayesian quantization、USTX continuous pitch | Python runtime、单专家最终决定 |
| SoulX-Singer Preprocess | separation / dereverb / RMVPE / ASR / ROSVOT 的完整预处理链 | 具体训练/服务框架 |
| ComfyUI-MIDI-Edit | reference lyrics + character-level matching；文本 identity 与 note geometry 分离 | ComfyUI product/runtime |
| GAME | singing note boundary / duration / pitch / voicing | 不把 GAME confidence 当全局真值 |
| GAME-rs | Rust/GGUF/WGPU/Vulkan/Metal/DX12/GL 的 native backend 参考 | 初期 production 仍优先 official ONNX/OpenVINO parity |
| ROSVOT | 第二唱声专用 transcription expert；external word duration/boundary conditioning | 初期不作为 mandatory baseline |
| Basic Pitch | 独立 onset/note/contour cross-check | 不作为 singing-specific primary |
| STARS | alignment/transcription/technique 多任务 expert；CKPT→ONNX→OpenVINO candidate | 不复制 Python inference orchestration；动态后处理留 native host |
| pYIN | multiple candidates + temporal/global decoding 的思想 | 不替代 RMVPE |
| Vocal2Midi-rs | Rust/native/process-owned worker 迁移参考 | 不直接沿用其产品边界 |
| MIDI-SAG | GAME 作为更大 vocal-to-MIDI pipeline 的架构证据 | 不引入伴奏生成域 |
| qwen3-asr.cpp | Native Qwen3 ASR + ForcedAligner 同进程实现路线 | 不使用上游 HTTP server |

核心结论：

> Vocal2Midi 已经证明单路线 baseline 可行；Uta 的增量价值在于多专家 calibration、dynamic fusion、disagreement escalation、uncertainty、duet topology 和严格的 Candidate/Authored 边界。

---

# Appendix B — Maximum Profile 可选实验专家

Maximum 可以在 disagreement window 使用额外 symbolic / semantic prior，例如：

```text
VocalParse-like symbolic prior
secondary separation expert
speaker/singer identity expert
multi-F0 expert
STARS OpenVINO native candidate
```

这些均为：

```text
optional expert
```

不能成为 VocalChart 的第二权威来源。

如果 experimental expert 缺失：

```text
baseline valid
-> ok / ok_degraded
```

不能导致正常 Balanced pipeline 不可用。

---

# Appendix C — Uta! Studio 工程约束

与 `bintis/uta-studio@native-inference` 集成时继续遵守：

1. 不允许在应用启动、页面 render、diagnostics 时自动下载模型。
2. 模型安装只能由明确 setup / Models & Runtime 操作触发。
3. Source music read-only。
4. Native editor playback 保持 local process boundary。
5. App-owned feature 需要 local in-process command API 或其等价表示。
6. `api_capabilities` 与实现同步。
7. Diagnostics 必须 non-destructive。
8. Linux 目标为 Wayland。
9. 单个 app-owned source file 不超过 2000 行。
10. 不建立 unauthenticated HTTP control server。
11. Native inference release 必须通过格式、API registry、音频 decode、UTZ/representation export 与平台 build 验证。

---

# Appendix D — 新 API 判定规则

新增模型或替换 runtime 默认不构成新 Studio public API。

只有当现有：

```text
run_analysis_plan
run_analysis_node
run_analysis_node_downstream
run_analysis_request
reanalyze_*
```

无法忠实表达新的用户行为时，才考虑新增 app API。

同样适用于：

```text
worker protocol
native ABI
runtime manager API
```

优先 backward-compatible extension。

---

# Appendix E — 文档冻结规则

本文 v2.0 RC 之后：

- 算法 recipe 可以继续变化；
- calibration 参数可以继续变化；
- model catalog 可以继续变化；
- expert 组合可以继续变化；

但以下边界应视为架构基线：

```text
Studio / Engine / Runtime Manager / UTZ ownership
Input semantic contract
canonical timeline
Audio Separation semantic roles
Candidate / Authored separation
VocalChart vs PitchEvidence authority
extensions vs representations
disagreement-window execution model
```

若需要改变这些边界，应形成新的架构版本，而不是在实现中静默漂移。
