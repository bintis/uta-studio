# Uta Studio 音频处理与唱声分析架构 — 最终定稿

**文档类型**：Architecture Baseline / Design Specification  
**版本**：1.0  
**状态**：FINAL / Approved  
**最终定稿日期**：2026-08-22  
**适用范围**：`native-inference` 重构及后续 Uta Studio 音频分析、自动制谱、歌词/音符时间轴生成  
**代码基线**：`bintis/uta-studio@native-inference`，HEAD `56fdbec50444939360caf2832a7b1d958941fe6b`  
**目标平台**：Windows / Linux，Intel Arc 优先；生产运行时无 Python  
**变更原则**：本文件作为基线。任何会改变模型职责、Artifact Contract、Canonical Singing Track 语义、运行时边界或最终判定逻辑的修改，都必须附带 benchmark / golden evidence 和迁移说明。

---

## 0. 文档目的

本文件定义 Uta Studio 新一代音频处理逻辑的**产品级、长期可维护架构**。

它不是“当前代码说明”，也不是“某个模型排行榜摘录”。它回答以下问题：

1. 一首歌从输入到最终 Karaoke / Singing Track，中间到底经过哪些阶段。
2. 各模型分别解决什么问题，哪些问题**禁止**交给单一模型决定。
3. 音源分离、Lead/Back Vocal、ASR、歌词对齐、连续 F0、音符边界、演唱技法、DSP 证据如何组合。
4. 如何从多个 Expert 的局部证据生成统一的 **Canonical Singing Track**。
5. 如何在 Fast / Balanced / Maximum 三种质量模式下做条件执行。
6. Rust 控制面、OpenVINO-first / Vulkan-fallback 运行时策略、各 worker 的边界如何定义。
7. 如何缓存、复现、验证、迁移并最终删除 Python 运行时。
8. 当未来出现更高分模型时，如何替换模型而不破坏整个系统。

本架构的核心不是“找到一个万能模型”，而是：

> **把不同模型当作不同领域的 Expert；先保留证据与不确定性，再通过校准、条件加权、结构约束和全局时序优化生成最终结果。**

---

# 1. 顶层设计原则

## 1.1 Canonical Truth 不属于任何单一模型

系统最终不能保存：

```text
winner = RMVPE
winner = GAME
winner = Qwen
```

系统最终保存的是：

```text
Canonical Lyrics
Canonical Word Boundaries
Canonical Notes
Canonical F0 Curve
Canonical Technique Track
Canonical Harmony Metadata
```

每个 Canonical 结果都必须带：

- confidence
- uncertainty
- provenance
- contributing experts
- model/runtime version
- input artifact hash
- calibration version
- fusion-policy version

## 1.2 模型职责必须正交化

不同模型的“分数”不可直接比较。例如 RMVPE 的 F0 confidence、GAME 的 boundary score、Basic Pitch 的 onset activation、ASR token probability、Forced Aligner timestamp confidence 都不是同一种概率。

因此系统必须先做 **Confidence Calibration**，然后才能进入 Fusion。

## 1.3 先把音频变成适合分析的问题，再分析

对于主旋律自动制谱，最重要的前置约束是：

> **尽量先把 polyphonic vocal 变成 monophonic lead vocal。**

和声、double vocal、back vocal 若残留在 Lead 输入中，会污染 F0 tracker、note boundary detector、onset detector、voicing、technique detector、ASR 和 alignment。

所以 Lead/Back Vocal 分离不是“附加功能”，而是主唱 Pitch/Note Pipeline 的前置质量门。

## 1.4 最终仓库与生产运行时零 Python

最终 cutover 的目标不是“用户机器不启动 Python”而已，而是 Uta Studio 仓库与产品运行时都彻底移除 Python 依赖：

```text
tracked .py/.pyi             ❌
Python runtime               ❌
PyTorch                      ❌
Transformers                 ❌
venv                         ❌
uv                           ❌
Python TCP inference server  ❌
Python model/setup scripts   ❌
```

最终门槛：

```sh
test -z "$(git ls-files '*.py' '*.pyi')"
```

模型转换、训练、benchmark 若确实需要 Python，只能发生在 Uta Studio 仓库之外的隔离开发环境，并把最终可复现的模型 revision、转换配方、文件 hash 与许可证记录回仓库；不得重新引入产品或仓库 Python 依赖。

## 1.5 Runtime 选择：通用能力路由 + 两个 Qwen 固定例外

绝大多数 AI node 使用统一 Runtime Router：

```text
OpenVINO
   ↓ exact graph 不支持 / parity 未通过 / stability 未通过
Vulkan
   ↓ 未验证或不可用
Fail closed
```

CPU 仅用于 reference / diagnostics / benchmark / developer mode；不属于生产自动 fallback。Native failure 永远不回退 Python。

**两个 Qwen node 是明确的固定例外，不走 OpenVINO-first：**

```text
Qwen3-ASR-1.7B
→ handy-computer/transcribe.cpp
→ runtime ea077b87590bcfb090d7c38c03ab36cd1c7005d3
→ GGML 8c63e70982c95ceb862e3a1073a2c1beef75d60a
→ Vulkan

Qwen3-ForcedAligner-0.6B
→ predict-woo/qwen3-asr.cpp
→ runtime 6dcc586e5073fd6e85ee5728e75f0903d6c70c6c
→ Vulkan build uses GGML override 8c63e70982c95ceb862e3a1073a2c1beef75d60a
→ CPU/reference pin remains 9be313313c8ecb9488911bd64550190e3ed80f38
```

其它模型（RoFormer、FireRed、RMVPE、FCPE、GAME、STARS、Basic Pitch、VocalParse 及未来 Expert）可以拥有 OpenVINO/Vulkan 两条实现，但每一个 `(model revision, backend, runtime recipe)` 都必须独立通过 parity、真实歌曲回归、取消/超时和 Intel GPU 稳定性门槛。

“支持两种 backend”不允许在同一次任务中静默切换，也不允许把未验证 backend 作为兜底。

## 1.6 Rust 是控制面，不是模型执行面

Rust 继续负责 Analysis DAG、queue、retry、cancel、progress aggregation、artifact lineage、cache signature、model/runtime readiness、config snapshot、work directory、atomic artifact commit、final DB state 和 failure reporting。

模型运行放在独立 worker 中。Rust 的 Runtime Router 根据 Model Registry 中的 `runtime_preference`、`validated_profiles` 与当前硬件状态做显式选择。

---

# 2. 整体流水线

```text
Original Song
    │
    ▼
[Stage 0] Decode / Normalize / Audio Plan
    │
    ▼
[Stage 1] Source Separation + Vocal Isolation + Restoration
    │
    ├──────────────► Best Instrumental / BGM
    │
    ▼
Clean Vocal Stem
    │
    ▼
Lead / Back Vocal Separation
    │
    ├──────────────► Back / Harmony Stem
    │
    ▼
Clean Lead Vocal
    │
    ├──────────────► ASR Experts
    ├──────────────► Alignment Evidence
    ├──────────────► F0 Experts
    ├──────────────► Note Boundary Experts
    ├──────────────► Onset / Activation Experts
    ├──────────────► Technique Experts
    └──────────────► Acoustic DSP
                         │
                         ▼
                 Evidence Timeline
                         │
                         ▼
              Confidence Calibration
                         │
                         ▼
              Context-aware Fusion
                         │
                         ▼
                Candidate Graph
                         │
                         ▼
                  HSMM / Viterbi
                         │
                         ▼
               Canonical Singing Track
                         │
         ┌───────────────┼────────────────┐
         ▼               ▼                ▼
      Lyrics           Notes          F0 / Bend
                                          │
                                          ▼
                                     Techniques
```

---

# 3. Stage 0 — Decode、标准化与 Audio Plan

所有后续模型必须消费同一个可复现的源音频计划。

Rust 生成不可变的 `AudioProcessingPlan`，至少记录：

```text
source_hash
source_codec
source_sample_rate
source_channels
decode_policy
analysis_sample_rate
channel_policy
trim_policy
normalization_policy
version
```

要求：

1. 同一次分析的所有 Expert 使用可追踪的共同时间基准。
2. 所有 resample / trim 必须可逆映射回 source time。
3. 模型可以有自己的内部采样率，但输出必须映射回 Canonical Timeline。
4. 不允许 helper 私自做不可见的 destructive trim。

---

# 4. Stage 1 — 音源分离、Lead/Back Vocal 与音频修复

## 4.1 分离目标

分离层最终至少产生：

```text
Instrumental / BGM
Vocal Stem
Lead Vocal
Back / Harmony Vocal
Optional residuals
```

真正进入主唱制谱的默认输入是：

```text
Clean Lead Vocal
```

## 4.2 模型角色分层

模型 registry 不能只保存 `model_id`，必须保存状态：

```text
ProductionPinned
BenchmarkCandidate
Experimental
Deprecated
```

### A. Vocal / Instrumental 主分离

当前质量目标应优先跟踪最新公开 benchmark 冠军，例如 `BS RoFormer 124-band` 一类候选。

任何冠军模型只有在以下条件全部满足后才能从 `BenchmarkCandidate` 升级到 `ProductionPinned`：

- 权重公开可获得
- config 可获得
- hash 可固定
- license / redistribution 处理完成
- GGML/Vulkan graph 可实现
- full-track 稳定性验证完成
- 与旧生产模型做 golden comparison
- 没有不可接受的 machine-level GPU failure

在候选未完全产品化前，现有已验证 RoFormer 路径继续作为生产 fallback / baseline。

### B. Lead / Back Vocal / Harmony 分离

主目标是从 Vocal Stem 中进一步分离：

```text
Lead Vocal
Back Vocal / Harmony
```

当前质量优先候选：

```text
Primary: MVSep #9570 class Mel-RoFormer Karaoke / Duet
Maximum challenger: MVSep #9068 class BS-RoFormer Lead/Back
```

不建议把 MVSep #9205 三模型 AVG 作为正式主路线：当前已比较指标没有超过 #9570，同时推理成本更高。

### C. Denoise

当前保留 `MelBand-RoFormer Denoise aufr33` 类模型。

### D. Dereverb

质量优先跟踪 `anvuew BS-RoFormer Dereverb 22.5050` 类候选，但仍需按 ProductionPinned 升级规则做 checkpoint / config / runtime 验证。

## 4.3 Restoration 顺序

Restoration 顺序不得由代码隐式写死。

建议将其定义为可版本化 profile：

```text
Vocal Isolation
→ Lead/Back Split
→ Denoise
→ Dereverb
```

这是当前优先实验顺序，不应在没有 A/B evidence 前视为不可变规则。

至少验证：

- split → denoise → dereverb
- denoise → split → dereverb
- split → dereverb → denoise

最终选择指标不是单一 SDR，而是后续 ASR、F0、note boundary 和最终人工修正成本。

## 4.4 Harmony 的产品语义

Back Vocal 不是垃圾 stem。

第一阶段：

```text
Main Chart 只使用 Lead Vocal
Back Vocal 保存为独立 artifact
```

后续可扩展：

```text
HarmonyTrack[]
EnsembleRegion[]
  ├── unison
  ├── octave_double
  ├── harmony
  └── choir
```

为未来 duet、男女对唱、和声提示、多声部 Karaoke、secondary singer tracks 留接口。

---

# 5. Stage 2 — ASR Pool 与 Canonical Lyrics

## 5.1 ASR 不采用 winner-takes-all

多个 ASR Expert 可以并行输出：

```text
TranscriptCandidate {
    text
    language
    tokens[]
    confidence
    timestamps_hint[]
    source_model
}
```

随后：

```text
token normalization
→ sequence alignment
→ edit-distance lattice / ROVER-like fusion
→ calibrated confidence
→ Canonical Lyrics
```

不能因为某个 ASR 的整句平均置信度高，就整句覆盖其他专家。

## 5.2 ASR Expert 角色

### FireRedASR2-AED — 中文 / 中文唱声 Primary

角色：

```text
Mandarin primary ASR
Chinese singing primary ASR
Chinese dialect / accent primary ASR
Chinese-English code-switching expert
word / character timestamp evidence
ASR confidence evidence
```

2026-08-22 设计快照采用 FireRedTeam 官方 FireRedASR2S 公开结果作为路由依据：

```text
Mandarin 4-set average CER:
FireRedASR2-AED  3.05%
Qwen3-ASR-1.7B   3.76%

Chinese dialect 19-set average CER:
FireRedASR2-AED 11.67%
Qwen3-ASR-1.7B  11.85%

OpenCPOP singing CER:
FireRedASR2-LLM  1.12%
FireRedASR2-AED  1.17%
Qwen3-ASR         2.57%
```

因此中文歌曲不能再把 Qwen3-ASR 当唯一或默认主专家。FireRedASR2-AED 的额外价值是其官方实现可返回置信度和 word/character-level timestamps，这些时间戳必须保留为 Alignment Evidence，而不是只取最终文本。

### Qwen3-ASR-1.7B — 多语言 / 独立唱声 Challenger

角色：

```text
Japanese / Korean / multilingual primary ASR
non-Chinese singing primary quality expert
language / multilingual evidence
Chinese song independent second expert
```

Qwen3 不因 FireRed 在中文领先而删除：它提供不同模型族的独立文本证据，并覆盖 FireRed 不支持的大量语言。中文歌曲中它进入 Transcript Fusion；非中文歌曲中它仍可作为 primary。

Runtime 约束：Qwen3-ASR-1.7B 固定使用 `handy-computer/transcribe.cpp` 的已锁定 GGML/Vulkan recipe；该实现明确支持 1.7B GGUF、Vulkan 和 reference/WER validation，但当前只输出 transcript，不提供 timestamps。

### FireRedASR2-LLM — Maximum 中文 Challenger

FireRedASR2-LLM 的中文与 OpenCPOP 指标进一步略高于 AED，但计算规模显著更大。它仅用于：

```text
Maximum Quality
high-value Chinese disagreement windows
AED vs Qwen transcript conflict
rare dialect / difficult acoustic region
```

它不应成为默认全曲执行模型。

### Whisper Large-v3 — Compatibility / Diversity Challenger

角色从“唯一 ASR”降级为：

```text
fallback benchmark baseline
independent challenger
Maximum disagreement expert
legacy compatibility reference
```

如果 Uta Studio 自有歌曲集长期证明没有增量价值，可以彻底删除。

### FireRedLID — Optional Routing Expert

FireRedASR2S 同时提供独立 FireRedLID。它可作为未来语言/方言路由证据，但不是当前 ASR 主链硬依赖。只有在其 native / OpenVINO 路径通过验证后，才允许取代现有 language-routing evidence。

## 5.3 ASR 路由

```text
Chinese / Mandarin / Cantonese / Chinese dialect / Chinese singing:
    Primary:   FireRedASR2-AED
    Secondary: Qwen3-ASR-1.7B
    Maximum:   + FireRedASR2-LLM
               + optional Whisper diversity challenger

Japanese / Korean / multilingual / non-Chinese singing:
    Primary:   Qwen3-ASR-1.7B
    Secondary: optional specialist / Whisper challenger

Unknown / mixed language:
    language-routing evidence
    → route FireRed for Chinese-family regions
    → route Qwen for broader multilingual regions
    → preserve code-switch candidates from both when needed
```

路由必须是 versioned policy，而不是散落的 `if language == ...`。

## 5.4 Transcript Fusion 必须保留模型独立性与 provenance

中文歌曲至少保留 FireRed AED 与 Qwen3 两套 token candidates。即使 FireRed 当前 benchmark 更强，也禁止整句 winner-takes-all。

Fusion 必须记录：

```text
text source
raw confidence
calibrated confidence
timestamp hint
language/dialect hint
model version
runtime backend
model artifact hash
```

FireRed AED 的 timestamp/confidence 与 Qwen ASR 的文本证据属于不同 evidence channel，后续 Qwen ForcedAligner 仍是独立强制对齐专家。

---

# 6. Stage 3 — Forced Alignment 与 Canonical Word Boundaries

## 6.1 主对齐专家

```text
Qwen3-ForcedAligner-0.6B
```

输入：

```text
Canonical Lyrics
+
Clean Lead Vocal
```

输出 word / token / unit timestamps。

Runtime 约束：该节点与 Qwen3-ASR 保持独立 Artifact / worker 语义。固定实现为 `predict-woo/qwen3-asr.cpp` 的已锁定 GGML/Vulkan recipe；只有未来其它实现通过同等 parity 与 Intel GPU 稳定性门槛后，才允许提出 runtime consolidation。

## 6.2 第二对齐证据

保留 `FireRed timestamps`，未来可加入 `SOFA — Singing-Oriented Forced Aligner`。

最终保存：

```text
CanonicalBoundary {
    start
    end
    confidence
    evidence[]
}
```

而不是“Qwen timestamp”。

## 6.3 Boundary Fusion

```text
Qwen:
12.32 - 12.74

FireRed:
12.28 - 12.78

Canonical:
12.30 - 12.76
confidence = calibrated(...)
```

歌词时间边界同时作为 GAME、HSMM、melisma 判断的结构先验。

---

# 7. Stage 4 — Pitch / Note 多专家系统

这是整个唱声制谱系统的核心。

## 7.1 RMVPE — Primary Continuous F0 Expert

RMVPE 只回答：当前约 10 ms 帧，此刻最可能的基频是多少？

统一输出：

```text
PitchFrame {
    time
    f0_hz
    midi_float
    confidence
    voiced_probability
}
```

### 禁止事项

```text
round(midi_float) == final_note
```

禁止。RMVPE **没有资格直接决定最终音符**。

## 7.2 FCPE — Secondary Continuous F0 Expert

FCPE 作为第二个独立 F0 challenger：

```text
RMVPE = primary robust singing F0
FCPE  = independent lightweight F0 evidence
```

用途：

- 检测 octave disagreement
- 检测弱音 / falsetto 分歧
- 给 Fusion Engine 提供独立 F0 posterior
- 在部分窗口作为 fallback

不能简单平均。

## 7.3 GAME — Primary Note Boundary Expert

GAME 的核心职责：

```text
note boundary
note region
continuous MIDI pitch
voiced / unvoiced
```

它继续作为主 Boundary Expert，尤其负责：

```text
vibrato ≠ new note
glissando ≠ every crossed semitone is a note
```

GAME 可利用 Canonical word boundaries 作为条件输入。

## 7.4 STARS — Secondary Boundary + Technique Expert

Maximum Quality 中加入完整 STARS：

```text
note boundary
note pitch
alignment evidence
vibrato
glissando
falsetto
breathy
ornament
...
```

### 重要：依赖相关性

如果 STARS 内部依赖/使用 RMVPE F0，则：

```text
RMVPE evidence
STARS pitch evidence
```

不能被当成两个完全独立投票。

Fusion Engine 必须保存 `depends_on`、`correlation_group`，并对相关证据做 discount。

## 7.5 Basic Pitch — Auxiliary Onset / Activation / Contour Expert

Basic Pitch 不再作为核心唱声 Pitch Expert，只提供：

```text
onset matrix
note activation matrix
pitch contour matrix
```

主要回答：

```text
这里是不是真的重新发了一个音？
这一段是否仍属于当前 MIDI note？
note 内部是否存在 pitch bend？
```

在 singing-specific 专家已有 GAME/STARS 后，Basic Pitch 的 boundary prior 应降低，但它仍可提供来自不同建模范式的独立 CQT/onset 证据。

## 7.6 VocalParse — High-level Symbolic Structure Expert

VocalParse 不负责 10ms 精确边界。

在 Maximum / disagreement 模式提供：

```text
pitch sequence prior
note-count prior
word-note relationship
rhythm / note-value prior
high-level symbolic structure
```

它是 Candidate Graph / HSMM 的结构先验，不直接输出最终时间边界。

## 7.7 DSP Expert

生产第一版保留传统声学证据：

```text
RMS
spectral flux
periodicity
SNR
pitch slope
energy delta
spectral features
```

DSP 用于 onset evidence、voicing corroboration、vibrato periodicity、glissando slope，并作为 AI 模型失败时最低限度证据。

---

# 8. Technique Detection

## 8.1 第一阶段

使用 RMVPE F0 contour、FCPE disagreement、GAME context、Basic Pitch contour、DSP periodicity/slope/energy 检测：

```text
stable
vibrato
glissando
ornament
```

## 8.2 Maximum Quality

加入 STARS 作为 Technique Expert。

## 8.3 长期生产方案

若完整 STARS 无法满足 native production：

```text
STARS offline teacher
→ pseudo labels
→ lightweight TechniqueStudent
→ ONNX
→ OpenVINO
```

输出：

```text
P(vibrato)
P(glissando)
P(falsetto)
P(breathy)
P(ornament)
```

---

# 9. Canonical Evidence Timeline

系统定义：

```text
Canonical time step = 10 ms
```

所有模型结果映射进同一时间轴。

语义结构：

```cpp
struct EvidenceFrame {
    double time;

    // F0
    float rmvpe_f0;
    float rmvpe_conf;
    float fcpe_f0;
    float fcpe_conf;

    // GAME
    float game_boundary;
    float game_pitch;
    float game_voiced;

    // STARS
    float stars_boundary;
    float stars_pitch;
    float stars_vibrato;
    float stars_glissando;

    // Basic Pitch
    float bp_onset[88];
    float bp_note[88];
    float bp_contour[264];

    // alignment
    int word_id;
    float word_boundary_conf;

    // symbolic prior
    float symbolic_note_prior;
    float symbolic_boundary_prior;

    // acoustic
    float rms;
    float spectral_flux;
    float periodicity;
    float snr;
};
```

内存实现可以使用稀疏结构；以上只是语义 contract。所有字段必须记录 availability，不能用 0 代替“模型没跑”。

---

# 10. Confidence Calibration

严禁：

```text
GAME 0.8
RMVPE 0.8
BasicPitch 0.8
→ equal confidence
```

每个 Expert 必须先经过自己的 calibrator：

```text
raw score
→ calibration model
→ calibrated probability / likelihood
```

允许 temperature scaling、Platt scaling、isotonic regression、small learned calibrator。Calibration 必须 versioned。

---

# 11. Dependency-aware Fusion

除了 score calibration，还要处理 Expert 之间的统计相关性。

```text
ExpertEvidence {
    expert_id
    task
    calibrated_score
    correlation_group
    dependencies[]
    runtime_version
    model_hash
}
```

同一 correlation group 的证据不能线性重复累加。

---

# 12. 初始 Prior

这些仅用于冷启动，不是永久权重。

## 12.1 Base Pitch

```text
RMVPE             0.40
GAME              0.25
FCPE              0.20
STARS*            0.05
Context / DSP     0.10
```

`STARS*` 必须做 RMVPE correlation discount。

## 12.2 Note Boundary

```text
GAME              0.40
STARS             0.25
Basic Pitch       0.10
Acoustic onset    0.10
Lyric boundary    0.10
VocalParse prior  0.05
```

## 12.3 Technique

```text
STARS             0.65
RMVPE features    0.15
GAME context      0.10
Basic Pitch       0.05
DSP               0.05
```

## 12.4 Word Boundary

```text
Qwen ForcedAligner   primary
FireRed timestamp    secondary
SOFA                  future challenger
```

实际数值必须由 alignment benchmark 和 calibration 得出，不在架构文件中永久写死。

---

# 13. Context-aware Dynamic Weighting

运行时权重：

```text
w_i(t) =
base_weight_i
× context_modifier_i(t)
× correlation_discount_i(t)
× quality_modifier_i(t)
```

再归一化。

## 13.1 Vibrato

若 vibrato probability 上升、F0 呈周期调制、GAME boundary 弱，则：

```text
new_note penalty ↑
Basic Pitch onset weight ↓
F0 contour weight ↑
Technique weight ↑
```

输出应倾向 `A4 + vibrato`，而不是 `A4 → A#4 → A4 → A#4`。

## 13.2 Glissando

若 glissando 上升、pitch slope 连续、GAME boundary 弱，则中间经过的半音不能自动形成 note。只有 strong boundary + new stable plateau + onset evidence 同时成立时才生成新 note。

## 13.3 Melisma / 转音

若 GAME/STARS boundary 强、Basic Pitch onset 强、出现新稳定平台、持续超过阈值且 glissando 低，则允许同一个 lyric token 对应多个 note。

---

# 14. Candidate Graph 与 HSMM / Viterbi

最终不做逐帧贪心。

先生成 boundary candidates：

```text
GAME
STARS
Basic Pitch onset
Qwen word boundary
FireRed timestamp
DSP onset
```

然后形成 segment candidates。

每个 segment：

```text
SegmentScore =
PitchScore
+ BoundaryScore
+ DurationScore
+ AlignmentScore
+ TechniqueScore
+ SymbolicPriorScore
```

## 14.1 为什么必须用 HSMM

目标状态是 `NOTE A4 持续 420 ms`，不是 `A4 → A4 → A4 → A4 ...`。

HSMM 直接建模 note identity、note duration、rest duration、state transition、melody continuity、lyric association。

## 14.2 Transition Cost

```text
A4 → A4     low
A4 → A#4    normal
A4 → A5     high
```

strong boundary + strong onset 可以降低大跳 transition cost；vibrato high 则临近半音跳变 cost 大幅提高。

---

# 15. Canonical Singing Track

最终核心 artifact：

```text
CanonicalSingingTrack
├── transcript
├── words[]
├── notes[]
├── f0_curve[]
├── pitch_bend[]
├── techniques[]
├── harmony_metadata
└── provenance
```

建议：

```text
CanonicalNote {
    start
    end
    midi_note
    center_pitch
    center_offset_cents
    confidence
    uncertain

    f0_curve[]
    pitch_bend[]

    technique {
        vibrato
        glissando
        falsetto
        ...
    }

    evidence {
        rmvpe
        fcpe
        game
        stars
        basic_pitch
        alignment
        dsp
        symbolic
    }
}
```

---

# 16. 不确定性是一等公民

若：

```text
A4  = 0.52
A#4 = 0.45
```

不能输出 `A4 confidence = 0.99`。

应该保存低 confidence、`uncertain = true`、alternatives。UI 后续可显示绿色=多专家一致、黄色=需要人工检查、红色=高度冲突。

---

# 17. Conditional Expert Execution

并不是每首歌都跑所有模型。

## Fast

```text
primary separation
primary ASR
Qwen ForcedAligner
RMVPE
GAME
DSP
```

agreement 足够高时直接完成。

## Balanced

冲突区域追加：

```text
FCPE
Basic Pitch
Chinese specialist ASR when relevant
```

## Maximum

只在必要时追加：

```text
STARS
VocalParse
secondary lead/back separator
FireRedASR2-LLM
Whisper challenger
SOFA
additional alignment / symbolic experts
```

---

# 18. Disagreement Windows

昂贵 Expert 只分析争议窗口。

例如整首 240 秒只有：

```text
32.1 - 34.5
75.3 - 76.2
133.1 - 135.7
```

出现 expert disagreement high、confidence low、boundary ambiguity high 时，才追加高成本模型。

这必须成为 scheduler 的正式能力，而不是模型内部私有优化。

---

# 19. Key / Rhythm / Acoustic Music Analysis

不需要为了“全 AI”增加不必要模型。

优先将已有确定性 fallback 算法移入 Rust。

## Key

```text
chroma
→ Krumhansl-style key profile matching
```

## Rhythm / Tempo

```text
spectral flux
→ onset strength
→ autocorrelation / tempo candidates
```

## Acoustic

```text
RMS
spectral flux
periodicity
SNR
```

这些结果同样进入 artifact / provenance，并可辅助 Fusion。

---

# 20. Runtime Architecture

运行时按“**统一路由策略 + 专用隔离 worker**”组织。OpenVINO 是优先 backend，但不同 Vulkan/GGML runtime 仍保持进程隔离，避免 GGML revision 与 driver failure 相互污染。

```text
Uta Studio / Rust app-core
│
├── Analysis DAG / Scheduler
├── Cache / Artifact / Lineage
├── Config / Model Registry
├── Progress / Retry / Cancel
├── Native DSP
└── Native Runtime Supervisor
     │
     ├── Runtime Router
     │     preference: OpenVINO → Vulkan → fail closed
     │
     ├── openvino-worker                      [preferred]
     │     OpenVINO GPU / Intel Arc
     │     RMVPE
     │     GAME
     │     FCPE / Basic Pitch where exported+validated
     │     TechniqueStudent where exported+validated
     │     FireRedASR2-AED ONNX candidate after parity validation
     │     any future compatible ONNX/IR model
     │
     ├── roformer-vulkan-worker               [fallback/specialized]
     │     GGML + Vulkan
     │     RoFormer family when no validated OpenVINO equivalent exists
     │
     ├── qwen-asr-vulkan-worker               [fallback/specialized]
     │     handy-computer/transcribe.cpp / GGML + Vulkan
     │     Qwen3-ASR-1.7B primary Vulkan implementation candidate
     │     transcript-only; no timestamps / forced alignment
     │
     ├── qwen-align-vulkan-worker             [fallback/specialized]
     │     predict-woo/qwen3-asr.cpp / GGML + Vulkan
     │     Qwen3-ForcedAligner-0.6B primary Vulkan implementation candidate
     │     Qwen3-ASR-0.6B may remain as validation/reference target
     │
     ├── firered-vulkan-worker                [fallback/specialized]
     │     OpenASR / native GGML implementation
     │     FireRedASR2-AED / LLM where Vulkan lane is audited+validated
     │
     └── fusion-engine
           Rust / C++
           no AI runtime
```

## 20.1 Qwen3-ASR-1.7B runtime policy — transcribe.cpp

`handy-computer/transcribe.cpp` is the preferred **Vulkan implementation candidate** for Qwen3-ASR-1.7B in this design revision. It is not automatically trusted as a prebuilt binary; Uta Studio must pin source revision, model revision, GGUF hash, quantization and GGML revision.

Current audited facts for this revision:

```text
repository: handy-computer/transcribe.cpp
license: MIT
implementation: C/C++ / GGML / GGUF
Python inference dependency: none
Qwen3-ASR-1.7B: explicitly supported
GPU backends: Vulkan / Metal / CUDA (plus explicit CPU reference lane)
Vulkan build flag: TRANSCRIBE_VULKAN=ON
model formats: BF16 / F16 / Q8_0 / Q6_K / Q5_K_M / Q4_K_M GGUF
validation: tensor-by-tensor reference checks + verbatim transcript parity
quality evidence: WER-tested published GGUFs
API: C API + official Rust binding
Qwen3 timestamps: not supported in current family
Forced Aligner: not implemented in current Qwen3-ASR family
```

Production integration rules:

1. `Qwen3-ASR-1.7B` is a **pinned Vulkan runtime exception** and does **not** use the generic `OpenVINO → Vulkan` router. Production integration is pinned to `handy-computer/transcribe.cpp` + the locked GGML revision/model recipe in `04-NATIVE-RUNTIME-LOCK-v1.0-FINAL.json`.
2. Do not add or prefer an OpenVINO path for this node during this refactor. A future runtime change requires a new audited architecture decision and parity/stability evidence; it is not an automatic fallback/preference.
3. Pin exact upstream source commit and exact Qwen model source revision; do not track `main` implicitly.
4. Start validation with F16/BF16 as fidelity reference, then evaluate Q8_0 / Q6_K / Q5_K_M / Q4_K_M against **singing ASR**, not only LibriSpeech WER. Quantization is promoted only when Uta Studio song-set CER/WER and downstream Canonical Lyrics quality remain within threshold.
5. Public Vulkan execution evidence is sufficient to classify this as `BenchmarkValidated/Vulkan-capable`, but public benchmark hardware is not Intel Arc. ProductionPinned still requires Uta Intel Arc evidence: full-song runs, repeated runs, cancellation, worker restart, concurrent RoFormer/OpenVINO load, driver recovery and deterministic artifact comparison.
6. The runtime's long single-call input capacity may simplify song execution, but Uta Studio should still retain explicit chunking policy for cancellation granularity, memory control, disagreement-window replay and stable source-time mapping. Input limit must never become an implicit product guarantee tied to an upstream implementation detail.
7. Uta Studio wraps the library/CLI behind its own stdio NDJSON worker contract. Do not use an external HTTP server as the internal orchestration protocol.

## 20.2 Qwen3-ForcedAligner runtime policy — qwen3-asr.cpp

`predict-woo/qwen3-asr.cpp` is the pinned **GGML/Vulkan implementation** for `Qwen3-ForcedAligner-0.6B`; Qwen3-ASR-1.7B uses the separate `transcribe.cpp` runtime.

Audited facts used by this revision:

```text
license: MIT
implementation: pure C++ / GGML
Python inference dependency: none
Qwen3-ForcedAligner-0.6B: explicitly implemented
Qwen3-ASR-0.6B: explicitly implemented
GGML_VULKAN CMake option: present
combined ASR + alignment path: present
```

Rules:

1. `Qwen3-ForcedAligner-0.6B` is a **pinned Vulkan runtime exception** and does **not** attempt OpenVINO in this refactor. Use the locked `predict-woo/qwen3-asr.cpp` runtime recipe, including the documented Vulkan GGML override.
2. Validate timestamps against the official Qwen reference on speech and singing fixtures, including CJK token/word reconstruction and source-time remapping.
3. Keep ASR and alignment as independent Artifact nodes even if one upstream project offers a combined command. `Canonical Lyrics` must be frozen before forced alignment.
4. Do not route Qwen3-ASR-1.7B back through this runtime merely to reduce worker count. Runtime consolidation is subordinate to model quality and Artifact Contract stability.
5. Future preferred direction: if `transcribe.cpp` gains a production-quality Qwen3-ForcedAligner family with parity and Intel Arc Vulkan evidence, evaluate consolidating Qwen ASR + alignment on one GGML/Vulkan base. This is an optimization, not a current dependency.

## 20.3 FireRed runtime policy

FireRedASR2-AED 优先尝试 OpenVINO：

```text
official FireRedASR2-AED weights
→ audited/reproducible ONNX export
→ encoder + decoder + CTC/timestamp head
→ OpenVINO compile_model(INTEL_GPU)
→ Uta host-side decoding / beam-search / timestamp reconstruction
```

已有社区 INT8 ONNX 包证明该模型可以拆为 `encoder.int8.onnx`、`decoder.int8.onnx`、`ctc.int8.onnx` 并在第三方本地引擎中做到示例输出与参考实现一致；这只证明 ONNX 路线具有可行性，不等于 Uta Studio 的 OpenVINO 实现已经验证。

ProductionPinned 之前必须：

- 从官方 FireRed 权重出发建立可复现转换流程，或对第三方转换做完整来源/hash/算子审计；
- 比较 reference PyTorch 与 ONNX/OpenVINO 的 logits/text/timestamp；
- 在中文普通话、方言、OpenCPOP/真实歌曲集验证 CER；
- 在 Intel Arc 上做 full-track repeated stability；
- 确认 INT8 不破坏 lyrics/timestamp quality；必要时使用 FP16/FP32 部分图；
- 保存 FireRed AED confidence 与 timestamp evidence。

若 OpenVINO 无法满足这些门槛，则 fallback 到独立 `firered-vulkan-worker`。OpenASR 已有 FireRedASR2-AED/LLM 的 native GGML 实现，可作为 Vulkan fallback 的工程参考；但仍必须对 Uta 目标 Intel Arc 的 Vulkan lane 做独立验证。

---

# 21. 为什么第一版坚持进程隔离

即使多个模块都使用 GGML，也不建议第一版强行静态链接成一个进程。

理由：不同 GGML revision、Vulkan driver failure isolation、OpenVINO/Vulkan context isolation、cancellation 可 kill child、crash 不拖垮 app-core、避免 C++ ABI 跨 Rust、helper 可独立测试、日后可替换 runtime 而不改控制面。

---

# 22. Worker Protocol

不再使用 Python loopback TCP server。

统一：

```text
child process
stdin  = NDJSON commands
stdout = NDJSON machine frames
stderr = human/debug logs
```

最小协议：

```json
{"type":"ready","protocol":1}
{"type":"progress","node":"rmvpe","fraction":0.42}
{"type":"output","artifact":"pitch_track","path":"..."}
{"type":"done","status":"ok"}
{"type":"error","code":"...","message":"..."}
```

原则：stdout 在 JSON mode 下只允许机器 frame；helper 只写 run-temp outputs；Rust 验证后 atomic commit；overall DAG progress 由 Rust 聚合；helper 不决定 cache reuse / freeze / bypass。

---

# 23. Artifact Ownership

## Rust owns

```text
stable cache
artifact lineage
freeze / bypass
cache signature
final path
DB state
analysis history
```

## Worker owns only

```text
temporary inference outputs
local progress
model-specific diagnostics
```

---

# 24. Cache Signature

至少包含：

```text
input artifact hash
AudioProcessingPlan version
node config
model id
model file SHA-256
model config SHA-256
runtime build/version
backend
protocol version
post-processing version
fusion policy version
calibration version
```

任何会改变语义结果的因素变化，cache 必须 miss。

---

# 25. Model Registry

每个模型条目至少需要：

```text
id
display_name
role[]
family
architecture
status
source
license
checkpoint filename
checkpoint sha256
config filename
config sha256
expected input
expected output
runtime
supported backend
quality evidence
hardware evidence
known limitations
last validation date
```

不要把 benchmark rank 和 production readiness 混成一个字段。

---

# 26. 模型升级规则

未来看到排行榜新冠军时，不能直接替换。

```text
BenchmarkCandidate
→ Weight/Config Verification
→ License Review
→ Converter / Runtime Support
→ Unit Golden
→ Full-track Golden
→ Hardware Stability
→ End-to-end Chart Benchmark
→ ProductionPinned
```

---

# 27. 调度顺序

推荐生产 pipeline：

```text
Stage 0
Decode / Audio Plan

Stage 1
Primary RoFormer separation
→ Vocal / Instrumental
→ Lead / Back
→ optional denoise / dereverb
→ Clean Lead Vocal

Stage 2 parallel
ASR Pool
RMVPE
GAME
DSP
key / rhythm

Stage 3
Transcript Fusion
→ Canonical Lyrics
→ Qwen Forced Alignment
→ boundary fusion

Stage 4
Initial Pitch / Note Fusion
→ disagreement detection

Stage 5 optional
FCPE
Basic Pitch
STARS
VocalParse
secondary ASR
SOFA
secondary Lead/Back separator

Stage 6
Candidate Graph
→ HSMM / Viterbi

Stage 7
Canonical Singing Track
→ editor/chart artifacts
→ export
```

---

# 28. 产品质量指标

不能只用模型原始 benchmark。必须建立 Uta Studio 自己的 end-to-end benchmark。

## 28.1 Separation

- vocal SDR / SI-SDR
- instrumental SDR
- lead/back separation quality
- vocal bleed
- harmony leakage

## 28.2 ASR

- CER
- WER
- lyric token edit distance
- hallucination rate
- repeated-token rate

## 28.3 Alignment

- average absolute timestamp error
- word onset error
- word offset error
- long-audio drift

## 28.4 F0

- RPA
- RCA
- octave error rate
- voiced/unvoiced F1
- vibrato contour preservation
- glissando continuity

## 28.5 Note transcription

- onset F1
- offset F1
- COnPOff F1
- note pitch accuracy
- note count error
- melisma accuracy
- false-note rate caused by vibrato/glide

## 28.6 产品最终指标

最重要：

```text
manual edits per minute
note timing correction distance
pitch correction count
lyric correction count
boundary correction count
time-to-acceptable-chart
```

最终模型选择应以这些指标为主，公开 leaderboard 只用于候选筛选。

---

# 29. 人工修正回流

用户修改：

```text
system:
A#4

human:
A4 + vibrato
```

必须保存：

```text
EvidenceSnapshot
+
HumanCorrection
```

长期累积后训练 `FusionMetaModel`。

---

# 30. Fusion Meta Model

它不需要重新听声音。

输入：

```text
GAME boundary
STARS boundary
BasicPitch onset
RMVPE pitch delta
FCPE disagreement
vibrato probability
glissando probability
duration
word-boundary distance
RMS delta
SNR
model disagreement
symbolic prior
```

输出：

```text
P(new_note)
P(note_pitch)
P(vibrato)
P(glissando)
```

初期可用 Logistic Regression，后续 LightGBM / small MLP。目标是学习“在什么场景应该相信哪个 Expert”。

---

# 31. Backend / Hardware 原则

目标硬件：Intel Arc。

## 31.1 全局 Runtime Selection Policy

```text
OpenVINO first
    ↓ unavailable / unsupported / parity gate failed
Vulkan fallback
    ↓ unavailable / stability gate failed
Fail closed
```

Rust Runtime Router 对每个 node 读取 Model Registry：

```text
runtime_preference = [openvino, vulkan]
allowed_backends
validated_profiles
required_precision
model_hash
runtime_hash
hardware_allowlist
parity_evidence_id
stability_evidence_id
```

只有 registry 中明确授权的组合才能执行。

## 31.2 当前模型的目标 lane

| Expert / Model | Preferred | Fallback | 当前说明 |
|---|---|---|---|
| RoFormer separation/restoration | OpenVINO if future parity proven | Vulkan | 现有成熟 native path 是 GGML/Vulkan，不能假设 OpenVINO 已等价 |
| FireRedASR2-AED | **OpenVINO** | Vulkan | ONNX 可行性已存在；Uta 仍需自有 Intel Arc parity/stability |
| FireRedASR2-LLM | OpenVINO if feasible | Vulkan | Maximum only；更可能先走 native GGML/Vulkan |
| Qwen3-ASR-1.7B | **Pinned Vulkan** | none | 固定 `transcribe.cpp` runtime recipe；不走通用 OpenVINO-first router；Intel Arc 仍需 Uta 最终验证 |
| Qwen3-ForcedAligner-0.6B | **Pinned Vulkan** | none | 固定 `predict-woo/qwen3-asr.cpp` + Vulkan GGML override recipe；与 ASR 保持独立 Artifact node |
| RMVPE | **OpenVINO** | Vulkan only if separately implemented | 已是 OpenVINO 主路线 |
| GAME | **OpenVINO** | Vulkan only if separately implemented | ONNX → OpenVINO 主路线 |
| FCPE | **OpenVINO if export validated** | Vulkan | independent F0 challenger |
| Basic Pitch | **OpenVINO** | Vulkan | ONNX 结构适合 OV；仍需 artifact parity |
| TechniqueStudent | **OpenVINO** | Vulkan | student ONNX production target |
| STARS full model | OpenVINO if export succeeds | Vulkan | Maximum/offline；不作为基础 runtime 依赖 |
| VocalParse | OpenVINO if feasible | Vulkan | Maximum structural prior |

## 31.3 CPU lane 的地位

CPU 不在生产 fallback 链中。允许：

```text
reference / golden generation
diagnostics
model conversion verification
CI small fixtures
explicit developer mode
```

不允许用户产品在 GPU backend 失败后静默切 CPU 并继续假装同一运行配置成功。

## 31.4 Runtime priority 不得牺牲模型语义

OpenVINO 优先是部署策略，不是质量豁免。若 OpenVINO INT8 比 Vulkan/FP16 造成可测的歌词 CER、timestamp、F0、boundary 或最终 chart regression，则该精度配置不得 ProductionPinned；可以提高 precision，或将该模型保留在 Vulkan fallback。

---

# 32. GPU 稳定性原则

历史上已经存在 Vulkan 组合导致 machine-level failure 的证据。

因此：

1. “能跑一次”不等于 production ready。
2. batch / async / coopmat 等参数必须按**精确验证组合**授权。
3. 未验证参数不得因为理论上更快就默认开启。
4. 验证记录不能因为后续成功而删除早期失败。
5. 稳定性 evidence 必须包括 full-track repeated runs。

---

# 33. Models & Runtime UX

模型下载只能是显式用户动作。

禁止 startup auto-download、diagnostic auto-download、hidden runtime install。

Models & Runtime 页面应该显示：

```text
RoFormer runtime
Vulkan device
OpenVINO runtime/device
Speech runtime
FireRed runtime
selected model ids
model hash
missing models
validation status
backend
```

不再显示 uv available、system python、venv、managed python、analyzer scripts。

---

# 34. 配置迁移

旧 Python-era config 仍可读用于迁移。

新配置只保存：

```text
quality_profile
runtime backend
model role mapping
restoration profile
fusion policy
calibration version
conditional expert thresholds
hardware policy
```

旧项如 python backend、whisperx、ctc python aligner、demucs python runner、torch xpu、venv 迁移后不得重新写回。

---

# 35. Zero-Python Gate

最终必须满足：

```sh
test -z "$(git ls-files '*.py' '*.pyi')"
```

并扫描 active executable/config：

```text
UTA_STUDIO_PYTHON_PATH
UTA_STUDIO_UV_PATH
python_path(
configured_python_path(
uv_path(
venv
server.py
app-core/analyzer
```

历史 validation docs 可以进入审查过的 allowlist，但 executable/config 代码必须零命中。

---

# 36. 重构迁移顺序

## Phase 0 — Freeze Contracts

冻结 Artifact schema、current outputs、golden songs、model hashes、current validation evidence、pitch semantics、transcript semantics、alignment semantics。

## Phase 1 — Native Worker Protocol

实现通用 Rust supervisor ↔ stdio NDJSON child workers；不新增、恢复或依赖 Python fallback。旧 Python 仅作为尚未删除的历史代码存在，新的 native path 不调用它。

## Phase 2 — Rust DSP 与 Artifact Policy

移入 Rust：audio plan、deterministic DSP、key、tempo、progress aggregation、cache/freeze/bypass、artifact commit。

## Phase 3 — Separation Native Production

产品化 RoFormer GGML/Vulkan、primary V/I、lead/back、denoise/dereverb where supported、conservative validated Vulkan profile。

## Phase 4 — OpenVINO Pitch Stack

产品化 RMVPE、GAME、FCPE where chosen、Basic Pitch auxiliary、TechniqueStudent。

## Phase 5 — Native Speech Stack

产品化 FireRedASR2-AED 中文主路由、Qwen3-ASR-1.7B 多语言路由、Qwen ForcedAligner、optional Whisper、Maximum FireRedASR2-LLM、transcript fusion、boundary fusion；优先验证 FireRed ONNX→OpenVINO、Qwen3-ASR-1.7B `transcribe.cpp`→Vulkan，以及 Qwen3-ForcedAligner `qwen3-asr.cpp`→Vulkan。

## Phase 6 — Fusion Engine

实现 Evidence Timeline、calibration、correlation discount、context dynamic weights、disagreement windows、Candidate Graph、HSMM/Viterbi、Canonical Singing Track。

## Phase 7 — Native Default

Native pipeline 成为唯一产品执行路径；golden comparison 使用冻结的历史 fixtures / reference artifacts，不在产品或仓库中保留 Python shadow runtime。

## Phase 8 — Delete Python

删除 analyzer Python、Python server、uv/venv setup、embedded Python scripts、Python model setup、Python CI compile checks、Python developer tools（改 Rust xtask 或删除）、Python runtime config/UI。

## Phase 9 — Final Cutover

完成 clean install、no-model launch、explicit model setup、full-song analysis、reanalysis、freeze/bypass、cancel/retry、editor、UTZ/UltraStar export、packaged Nix smoke、Windows/Linux hardware evidence、no Python process。

---

# 37. 验收门槛

基础工程 gate：

```text
cargo fmt
cargo check
cargo test
cargo clippy -D warnings
cargo xtask docs check
native CMake build
native ctest
Nix build
```

GPU / hardware test 不允许假装由普通 hosted CI 覆盖，必须有实际 Intel Arc/self-hosted/manual evidence。

---

# 38. 必须保留的 Artifact / Debug Evidence

每次分析至少可追踪：

```text
source hash
audio plan
selected model ids
model hashes
runtime versions
backend
per-node attempts
per-node logs
temporary output validation
cache decision
expert raw outputs
calibrated evidence
fusion policy
canonical result
uncertainty
human corrections
```

这样任何错误都能回答：是分离错、ASR 错、alignment 错、F0 错、boundary 错，还是 Fusion 错？

---

# 39. 当前推荐逻辑的职责表

| 问题 | Primary | Secondary / Maximum |
|---|---|---|
| Vocal / Instrumental | newest validated top RoFormer | existing validated RoFormer baseline |
| Lead / Back Vocal | #9570-class Karaoke/Duet candidate | #9068-class Lead/Back |
| Denoise | aufr33 Denoise class | future challenger |
| Dereverb | anvuew 22.5050-class candidate | prior validated model |
| 多语言/非中文唱声 ASR | Qwen3-ASR-1.7B | Whisper / language specialist challenger |
| 中文/方言/中文唱声 ASR | **FireRedASR2-AED** | Qwen3-ASR-1.7B；Maximum + FireRedASR2-LLM |
| Forced Alignment | Qwen3-ForcedAligner | FireRed TS / SOFA |
| Continuous F0 | RMVPE | FCPE |
| Note Boundary | GAME | STARS |
| Onset / Activation | GAME/STARS context | Basic Pitch auxiliary |
| Technique | STARS / Student | DSP |
| Symbolic note structure | HSMM evidence | VocalParse Maximum prior |
| Final note sequence | HSMM / Viterbi | — |

注意：表中 `candidate` 不等于已允许进入 production。

---

# 40. 当前待验证事项

产品 cutover 前必须关闭：

1. BS RoFormer 124-band 冠军权重、config、license、hash 与 native graph 可用性。
2. MVSep #9570 / #9068 对应权重的公开可获得性、license、config、GGML 转换。
3. Denoise / Dereverb 最优链路顺序。
4. STARS 是否直接进入 Maximum runtime，还是只作为 teacher。
5. FCPE 是否值得成为常驻 Balanced Expert，还是只在 disagreement windows 运行。
6. VocalParse native runtime 的可行性与产品增益。
7. SOFA 是否在真实 singing alignment benchmark 上给 Qwen 带来增量。
8. Lead/Back separation 对最终 note F1 / 人工修正量的真实提升。
9. Fusion initial priors 的 calibration 数据集。
10. Canonical Singing Track 的最终稳定 schema。
11. Windows/Linux Intel Arc 上精确允许的 Vulkan 参数组合。
12. 所有旧 Python config/artifact 的 migration policy。

---

# 41. 变更控制

以后若更换模型，不修改上层问题定义。

例如出现新 F0 冠军：

```text
Old:
RMVPE = Continuous F0 Expert

New:
NewModel = Continuous F0 Expert
```

只替换 Expert implementation 与 calibration，不改变 Canonical Timeline、Evidence Contract、Candidate Graph、HSMM semantics、Canonical Singing Track。

这是本架构长期稳定性的关键。

---

# 42. 最终结论

Uta Studio 的音频处理系统不再是：

```text
分离
→ ASR
→ RMVPE
→ round MIDI
→ chart
```

而是：

```text
高质量分离
→ Lead / Harmony 解耦
→ 多 ASR Transcript Fusion
→ Forced Alignment Fusion
→ RMVPE + FCPE continuous F0
→ GAME + STARS note-boundary evidence
→ Basic Pitch auxiliary onset/activation
→ DSP + technique evidence
→ VocalParse symbolic prior（Maximum）
→ Confidence Calibration
→ Dependency-aware Dynamic Fusion
→ Candidate Graph
→ HSMM / Viterbi
→ Canonical Singing Track
→ Editor / Karaoke Chart
```

真正稳定、需要长期维护的是：

```text
Artifact Contracts
Evidence Timeline
Expert Responsibilities
Calibration
Fusion Policy
HSMM / Viterbi
Canonical Singing Track
Provenance
```

模型可以迭代，排行榜可以变化，runtime 可以替换；上面这些语义层必须保持稳定。

---

## Appendix A — 推荐质量模式

### Fast

```text
Primary separation
Primary Lead/Back
Routed Primary ASR
  Chinese: FireRedASR2-AED
  non-Chinese: Qwen3-ASR
Qwen ForcedAligner
RMVPE
GAME
DSP
HSMM
```

### Balanced

```text
Fast
+ Chinese: Qwen3 second ASR expert
+ non-Chinese: optional language specialist
+ FCPE disagreement
+ Basic Pitch disagreement
+ richer boundary fusion
```

### Maximum

```text
Balanced
+ secondary Lead/Back expert
+ STARS
+ VocalParse
+ FireRedASR2-LLM on Chinese disagreement windows
+ Whisper challenger
+ SOFA
+ only-disagreement-window reruns
```

---

## Appendix B — 设计中的关键“不允许”

```text
不允许 RMVPE 直接 round 成 final note
不允许 ASR winner-takes-all
不允许 raw confidence 跨模型直接比较
不允许相关模型证据重复计票
不允许 helper 自己提交 stable artifact
不允许模型 worker 决定 cache/freeze/bypass
不允许 silent backend fallback；生产只允许 OpenVINO→Vulkan 的显式已验证 fallback
不允许 startup 自动下载模型
不允许“benchmark 第一”直接等于 ProductionPinned
不允许删除历史失败证据
不允许 Python 回到生产 runtime
```

---

## Appendix C — Final document governance

```text
1.x:
保持 Canonical Singing Track 与 Artifact Contract 兼容，
允许替换模型与调整 prior。

2.0:
只有在 Canonical schema / time semantics / artifact ownership
发生 breaking change 时升级。
```

每次模型升级单独建立 ADR，例如：

```text
ADR-xxx Replace Vocal Expert A with B
ADR-xxx Route Chinese singing ASR to FireRedASR2-AED
ADR-xxx Route Qwen3-ASR-1.7B Vulkan fallback to transcribe.cpp
ADR-xxx Keep Qwen3 Forced Aligner as independent runtime/artifact node
ADR-xxx Integrate qwen3-asr.cpp as ForcedAligner Vulkan worker
ADR-xxx Add STARS as secondary boundary expert
ADR-xxx Change Lead/Back separator
ADR-xxx Change restoration order
```

ADR 必须记录 old model、new model、benchmark、Uta Studio end-to-end benchmark、model hash、runtime、hardware evidence、migration、rollback。


---

## Appendix D — Final Evidence Snapshot（2026-08-22）

本 Appendix 只记录本版本设计决策依赖的外部事实，避免未来把“当时已验证”和“后来出现的新实现”混为一谈。

### FireRedASR2S

- Official repository: `https://github.com/FireRedTeam/FireRedASR2S`
- License: Apache-2.0
- FireRedASR2-AED: Chinese / 20+ dialects / English / code-switching / speech + singing；支持 confidence 与 word-level timestamps。
- Official reported CER used by this revision: Mandarin avg 3.05% (AED), dialect avg 11.67% (AED), OpenCPOP singing 1.17% (AED) / 1.12% (LLM), versus Qwen3-ASR 2.57% on OpenCPOP in the same FireRed comparison table。
- This is sufficient to route Chinese singing primary ASR to FireRedASR2-AED, but Uta Studio still requires its own song benchmark.

### FireRedASR2-AED ONNX feasibility

- Community deployment artifact observed: `42ailab/FireRedASR2-AED-ONNX`。
- Contains INT8 `encoder.onnx`, `decoder.onnx`, `ctc.onnx`-style split artifacts and reports local end-to-end reference-output verification.
- This is **feasibility evidence only**. Production conversion must be reproducible/audited and independently validated with OpenVINO on Intel Arc.

### transcribe.cpp — Qwen3-ASR-1.7B Vulkan candidate

- Repository: `https://github.com/handy-computer/transcribe.cpp`
- License: MIT; C/C++ GGML/GGUF inference with Vulkan/Metal/CUDA and official Rust binding.
- Qwen3-ASR-1.7B is explicitly supported with BF16/F16/Q8_0/Q6_K/Q5_K_M/Q4_K_M GGUF artifacts.
- Published Q8_0 WER on LibriSpeech test-clean: 1.61%; documentation also records tensor-by-tensor numerical validation and verbatim transcript parity against the author reference implementation. The model card records numerical validation at upstream `transcribe.cpp` commit `3f61df7`.
- The 1.7B model has published real Vulkan benchmark evidence on AMD RADV (documented with `transcribe.cpp` commit `3d16f74`). This proves Vulkan graph execution, **not Intel Arc production stability**.
- Current Qwen3-ASR family is transcript-only: no timestamps, no forced alignment. Therefore it does not replace the dedicated aligner node.
- This revision promotes `transcribe.cpp` to the first Vulkan implementation candidate for Qwen3-ASR-1.7B.

### qwen3-asr.cpp — Qwen3 Forced Aligner candidate

- Repository: `https://github.com/predict-woo/qwen3-asr.cpp`
- License: MIT; pure C++/GGML inference; no Python runtime.
- Main README explicitly implements Qwen3-ASR-0.6B and Qwen3-ForcedAligner-0.6B; CMake exposes `GGML_VULKAN`.
- Its final role is the pinned `Qwen3-ForcedAligner-0.6B` Vulkan runtime. Qwen3-ASR-1.7B is pinned to `transcribe.cpp`.

### OpenASR fallback reference

- OpenASR currently advertises native ggml-backed FireRedASR2-AED and FireRedASR2-LLM model families with no Python inference.
- It is a useful fallback implementation reference; Uta Studio must still validate the exact Vulkan build, model pack, Intel Arc driver, precision and full-track stability combination before promotion.

### Final runtime policy

```text
Production inference backend order:
OpenVINO → Vulkan → fail closed

CPU: explicit reference/developer lane only
Python: never a production inference fallback
```

---

# 41. Processing Studio、Compiled DAG 与 Editor 的统一关系

本节依据 `native-inference` 分支当前代码重新审计后冻结。

当前代码已经存在三套非常有价值、而且应继续保留的基础：

1. `app-core/src/analysis_graph.rs`
   - 静态 `AnalysisGraphSpec`
   - 稳定字符串型 `AnalysisNodeId`
   - Artifact Kind
   - DAG validation / topo order / dependency closure
2. `app-core/src/analysis_artifact.rs`
   - immutable `ArtifactRevision`
   - producer node
   - input revisions
   - config hash
   - algorithm version
   - content-addressed Artifact Store
3. `app-core/src/editor/*` + `desktop/src/studio/editor/*`
   - UI-agnostic `EditorDocument`
   - Native Bevy chart editor
   - Candidate/Authored revision loading
   - revision merge
   - multi-track Lead/Harmony/Backing/Adlib
   - pitch guide、waveform、beat grid、audition、problems、undo/redo

因此目标不是用 Processing Studio 代替 Analysis Graph 或 Editor。

目标关系必须是：

```text
Processing Studio
    ↓
User Workflow Definition
    ↓
Workflow Compiler
    ↓
Compiled Analysis DAG
    ↓
Native execution + Artifact Revisions
    ↓
Canonical Singing Track / CandidateChart
    ↓
Editor
    ↓
Human-authored correction
    ↓
AuthoredChart + HumanCorrection records
```

Advanced Graph 则是 Compiled Analysis DAG 的诊断投影：

```text
Processing Studio ──compile──► Compiled DAG
                                  │
                                  ├──► Advanced Graph
                                  ├──► Execution
                                  └──► Artifact Lineage
```

---

# 42. 当前固定 DAG 与目标动态 Workflow 的差异

当前 `analysis_graph.rs` 的 baseline graph 仍然硬编码了若干业务顺序，例如：

```text
stems.vocals
    ↓
vocals.denoise
    ↓
vocals.dereverb
    ↓
stems.bind_analysis_outputs
```

BGM 路径同样硬编码：

```text
stems.instrumental
    ↓
instrumental.denoise
    ↓
instrumental.dereverb
```

这与 Processing Studio 的目标不再一致。

用户必须可以合法表达：

```text
Vocal → Dereverb → Harmony Split → Lead → Denoise
```

或：

```text
Vocal → Denoise → Harmony Split
                  ├─ Lead → Dereverb
                  └─ Back
```

因此 `baseline_graph_spec()` 不应继续承担“产品业务流程唯一真相”的职责。

目标应拆成：

```text
Node Capability Registry
        +
Workflow Definition
        ↓
Workflow Compiler
        ↓
AnalysisGraphSpec
```

`AnalysisGraphSpec` 继续保留，因为它非常适合：

- validate；
- topo order；
- historical run snapshot；
- Advanced Graph；
- planner；
- scheduler；
- dependency closure。

变化只是：

> Graph 从手工写死的业务图，变成由 Workflow Compiler 产生的执行图。

历史 `AnalysisPlan` / `AnalysisRun` 不应迁移重写；稳定字符串 `AnalysisNodeId` 的设计继续发挥兼容作用。

---

# 43. AudioProcessingPlanSnapshot 是 Workflow Compiler 的现有雏形

当前 `audio_processing.rs` 已经具备重要基础：

```text
AudioProcessingSettings
├─ vocal_model_id
├─ vocal_cleanup_chain[]
├─ accompaniment_model_id
├─ accompaniment_cleanup_chain[]
└─ per_model_overrides
```

以及：

```text
AudioProcessingStep
├─ step_id
├─ model_id
├─ input: SourceMedia | StepOutput
├─ selected_output_roles
└─ effective_parameters
```

这说明当前系统其实已经在执行：

> Settings → Ordered Steps → Immutable Run Snapshot

目标重构不应推翻这一层，而应把它泛化：

```text
AudioProcessingSettings        legacy/simple preset
            ↓ migrate
WorkflowDefinition             user intent
            ↓ compile
WorkflowExecutionSnapshot      immutable run snapshot
            ↓ lower
AnalysisGraphSpec + RuntimePlan
```

现有 `AudioProcessingPlanSnapshot` 可作为 migration input，并逐步演化为或被 `WorkflowExecutionSnapshot` 取代。

---

# 44. 必须引入 Node Capability 与 Node Instance 分离

当前固定节点名：

```text
vocals.denoise
vocals.dereverb
```

隐含“同类节点只能出现一次”。

Processing Studio 需要允许：

```text
Vocal
  ↓
Denoise A
  ↓
Harmony Split
  ↓
Lead
  ↓
Denoise B
```

因此目标数据模型必须分开：

```rust
struct WorkflowNodeInstance {
    instance_id: WorkflowNodeId,
    capability_id: CapabilityId,
    model_id: ModelId,
    params: ParameterMap,
    execution_policy: ExecutionPolicy,
    priority: i32,
}
```

例如：

```text
instance_id = wf:abc:node:17
capability  = audio.denoise
model       = melband_denoise_aufr33
```

第二次：

```text
instance_id = wf:abc:node:23
capability  = audio.denoise
model       = melband_denoise_aufr33
```

ArtifactRevision 的 `producer_node` 应最终记录 instance identity，而不是只能记录 capability identity。

---

# 45. ArtifactKind 不应继续编码每一种处理阶段

当前 ArtifactKind 包含：

```text
RawVocalStem
DenoisedVocalStem
DereverbedVocalStem
AnalysisVocalStem
DenoisedInstrumentalStem
DereverbedInstrumentalStem
...
```

这在固定链中可用，但动态 Workflow 会产生状态组合爆炸：

```text
dereverbed_then_denoised_lead
denoised_then_harmony_lead
harmony_back_dereverbed
second_pass_denoised_lead
...
```

目标方案：

```rust
enum ArtifactKind {
    // 保留历史 variants 用于反序列化
    ...
    AudioStem,
    EvidenceBundle,
    CanonicalSingingTrack,
    HumanCorrectionSet,
}
```

并增加语义描述：

```rust
struct AudioArtifactDescriptor {
    role: AudioRole,
    channels: ChannelLayout,
    sample_rate: u32,
    tags: Vec<ProcessingTag>,
}
```

`AudioRole` 示例：

```text
SourceMix
Vocal
LeadVocal
BackVocal
Instrumental
Drums
Bass
Guitar
Piano
Other
```

“做过什么处理”由 Artifact lineage 表达，而不是 ArtifactKind 枚举名表达。

旧 ArtifactKind 不立刻删除；历史 revision 继续可读，通过 migration/adapter 映射到新的 descriptor。

---

# 46. Editor 的正式系统角色

Editor 是 Uta Studio 的核心产品能力，必须保留并强化。

Processing Studio 的职责：

> 生成最可信的候选结果。

Editor 的职责：

> 让人把候选结果验证、修正、定稿。

因此禁止以下设计：

```text
Processing Studio
    ↓
自动生成 final chart
    ↓
Editor 只做“查看”
```

正确设计：

```text
Canonical Singing Track
    ↓
CandidateChart Revision
    ↓
Editor Working Copy
    ↓
Human edits
    ↓
AuthoredChart Revision
```

系统永远区分：

```text
Model-derived evidence     read-only
Candidate chart            replaceable / regenerable
Authored chart             human-owned
```

重新分析不得静默覆盖 AuthoredChart。

---

# 47. 当前 Editor 已有的能力必须保留

代码审计确认以下能力已经存在，重构不得回退：

## 47.1 多轨

TrackRole 已支持：

```text
Lead
Harmony
Backing
Adlib
```

多个 Lead track 自动形成 duet part。

这与今天设计的 Lead / Back / Harmony stem 直接兼容。

## 47.2 Timeline

现有 timeline 已有：

- piano pitch gutter；
- active-track editable notes；
- other-track ghost notes；
- lyric-bound-note highlight；
- time ruler；
- detected beat grid；
- waveform；
- read-only analyzer pitch contour。

## 47.3 编辑

已有：

- add / delete；
- split / merge；
- quantize；
- copy / cut / paste / duplicate；
- timing nudge / resize；
- semitone / octave transpose；
- lyric split / merge / syllabize；
- lyric boundary nudge；
- phrase split / merge；
- lyric-note bind / unbind；
- tap-to-time；
- global timing shift；
- note kind Normal/Golden/Freestyle/Rap/GoldenRap；
- lock mode。

## 47.4 Audition

已有：

```text
Audio
Pitch
Mixed
```

并支持 selection / before / after / visible region audition。

## 47.5 安全性

EditorDocument 故意允许 drag 过程出现临时 overlap，而不是阻止鼠标操作；最终由 Problems 阻止非法保存。

这个交互原则必须保留。

---

# 48. Editor 要从单一 Pitch Guide 升级为 Evidence Workbench

现有 `NativeEditor.pitch_frames` 是很好的基础，但目标 Canonical Singing Track 已经拥有多个 Expert。

Editor 应增加只读 Evidence Layer：

```text
RMVPE continuous F0
FCPE continuous F0
GAME note boundary
GAME pitch / voiced
Basic Pitch onset / activation
STARS technique / boundary
Qwen word boundary
FireRed timestamp
DSP onset / RMS / spectral flux
Fusion confidence
Disagreement windows
```

默认不全部打开。

推荐 View 菜单：

```text
Evidence
├─ Canonical confidence
├─ F0
│   ├─ RMVPE
│   ├─ FCPE
│   └─ fused
├─ Boundaries
│   ├─ GAME
│   ├─ Basic Pitch onset
│   └─ lyric boundary
├─ Technique
│   ├─ vibrato
│   ├─ glissando
│   └─ ornament
└─ Disagreement regions
```

原则不变：

> Evidence 永远只读；EditorDocument 是 authored truth。

---

# 49. Editor 应增加 Suggestion Layer，而不是自动修改

模型与 Fusion 可以向 Editor 提建议，但不直接改 authored note。

建议数据：

```rust
struct EditorSuggestion {
    id: SuggestionId,
    time_range: TimeRange,
    kind: SuggestionKind,
    confidence: f32,
    evidence_refs: Vec<ArtifactRef>,
}
```

例如：

```text
Possible octave error
GAME boundary here
Lyric boundary mismatch
Candidate A4 vs authored A#4
Possible vibrato, not note split
Low-confidence word alignment
```

用户操作：

```text
Accept
Ignore
Compare evidence
Apply to selection
```

接受 Suggestion 必须走现有 Editor action/undo 系统，成为普通 undoable human action。

不得存在：

```text
model refresh → silently mutate EditorDocument
```

---

# 50. Disagreement-first Editor

今天的 Conditional Expert 设计应继续延伸到人工编辑。

Processing Studio 在生成 CandidateChart 后应产生：

```text
Review Queue
```

包含：

- low confidence；
- expert disagreement；
- suspicious octave jump；
- note-count disagreement；
- word-note mismatch；
- unvoiced / voiced conflict；
- glissando vs new-note conflict；
- lead/back contamination warning。

Editor 增加：

```text
Previous Issue
Next Issue
Review unresolved only
Mark reviewed
```

这将把人工校对从“从头听完整首歌”变成：

> 先看模型自己不确定的 5%–20% 区域。

这是 Maximum Quality 的关键产品能力。

---

# 51. Workflow Artifact 必须直接进入 Editor Source Picker

当前 Editor 的 audio source / waveform source 主要是：

```text
Original
Vocals
Instrumental
```

目标必须改成 ArtifactRevision 驱动。

例如：

```text
Audio Source
├─ Original Mix
├─ Vocal · raw
├─ Vocal · dereverb
├─ Lead Vocal · harmony split
├─ Lead Vocal · dereverb + denoise
├─ Back Vocal
└─ Final BGM
```

允许：

```text
Playback source       Final BGM
Waveform source       Clean Lead
Evidence source       Canonical bundle
```

并增加 A/B audition：

```text
A = Lead before denoise
B = Lead after denoise
```

用于人工判断 Workflow 后处理是否损伤辅音、气声、滑音或瞬态。

---

# 52. Harmony 与 Editor 的直接连接

因为 Editor 已支持 Lead/Harmony/Backing/Adlib：

- Lead Vocal candidate → Lead track；
- Harmony candidate → Harmony track；
- backing chorus candidate → Backing track；
- ad-lib detector → Adlib track。

默认策略：

```text
Lead        scored
Harmony     reference by default
Backing     reference by default
Adlib       reference by default
```

用户可以手动开启 scoring 或改变 role。

对于真正 duet：

```text
Lead Track A
Lead Track B
```

继续沿用现有自动 part 编号。

因此今天新增 Harmony Separation 不需要另造一个“和声编辑器”。

应该直接强化现有 Track Strip。

---

# 53. Candidate / Authored / Re-analysis 生命周期

现有代码已经支持：

- 从 immutable Artifact revision 打开 Editor；
- Candidate/Authored revision merge working copy。

目标生命周期：

```text
Workflow Run #17
    ↓
CandidateChart C17
    ↓
Open in Editor
    ↓
AuthoredChart A1
```

之后 Workflow 改动：

```text
Workflow Run #18
    ↓
CandidateChart C18
```

系统必须显示：

```text
Authored chart exists
New candidate available
```

用户选择：

```text
Keep authored
Compare
Open candidate separately
Merge candidate suggestions into authored working copy
```

禁止自动替换 A1。

---

# 54. HumanCorrection 必须是一等数据

保存 AuthoredChart 时，同时可写增量修正记录：

```rust
struct HumanCorrection {
    song_id: String,
    workflow_revision: String,
    candidate_revision: String,
    authored_revision: String,
    time_range: TimeRange,
    correction_type: CorrectionType,
    before: CorrectionValue,
    after: CorrectionValue,
    evidence_snapshot: Vec<ArtifactRef>,
}
```

CorrectionType 示例：

```text
Pitch
Boundary
LyricText
LyricBoundary
TrackRole
Technique
Voicing
DeleteFalseNote
AddMissedNote
```

这些记录用于：

- calibration；
- expert context weights；
- future Fusion Meta Model；
- regression fixture。

用户的 chart 本身仍是主产品数据；HumanCorrection 是学习/分析数据，不反向绑死 chart format。

---

# 55. 页面层级建议

当前 `StudioRoute` 已经把 Editor 作为一等 route。

目标 song-level 工作区建议：

```text
Processing
Graph
Editor
Results
```

其中：

### Processing
定义 Workflow、模型、Artifact source 和 execution policy。

### Graph
查看 compiled DAG、run、lineage、cache、logs。

### Editor
人工验证/修正 CandidateChart。

### Results
预览、导出、版本与质量摘要。

Editor 不应藏在 Analysis 页面里，也不应在新 Processing Studio 上线后被降级成二级弹窗。

---

# 56. Editor 与 Processing Studio 的状态互相投影

Processing Studio 的 Finalization Node 显示：

```text
Canonical Singing Track
Candidate ready
```

如果用户打开过：

```text
Opened in Editor
```

保存后：

```text
Authored
```

上游 Workflow 改动后：

```text
Authored · new candidate available
```

Editor 顶部显示来源：

```text
Authored from Candidate C17
Workflow: Maximum v4
New Candidate C18 available
[Compare]
```

这样用户能理解：

> Workflow 负责机器版本，Editor 负责我的版本。

---

# 57. 当前代码重构映射

## 保留并扩展

```text
app-core/src/editor/*
desktop/src/studio/editor/*
app-core/src/analysis_artifact.rs
app-core/src/analysis_plan.rs
desktop/src/studio/analysis_*
```

## 重构职责

```text
app-core/src/analysis_graph.rs
```

从：

> static business DAG definition

演化为：

> compiled DAG representation + legacy baseline migration support

新增推荐：

```text
app-core/src/workflow/
├─ capability.rs
├─ definition.rs
├─ compiler.rs
├─ validation.rs
├─ migration.rs
└─ snapshot.rs
```

Desktop 新增：

```text
desktop/src/studio/processing_studio/
├─ state.rs
├─ canvas.rs
├─ node_card.rs
├─ lanes.rs
├─ inspector.rs
├─ drag.rs
├─ validation.rs
└─ actions.rs
```

Editor 新增建议模块：

```text
desktop/src/studio/editor/
├─ evidence.rs
├─ suggestions.rs
├─ review_queue.rs
└─ artifact_sources.rs
```

Core 新增：

```text
app-core/src/editor/
├─ evidence.rs
├─ suggestions.rs
└─ corrections.rs
```

---

# 58. 迁移顺序修订

在原 Phase 6 / Phase 7 之间增加产品工作流迁移：

## Phase 6A — Workflow Domain

- Node Capability Registry；
- WorkflowDefinition；
- unique Node Instance id；
- typed ports；
- compiler；
- migration from AudioProcessingSettings；
- compile to existing AnalysisGraphSpec。

## Phase 6B — Processing Studio

- Audio lanes；
- drag/reorder；
- analyzer attachments；
- model selector；
- validation；
- compiled graph preview。

## Phase 6C — Editor Bridge

- CandidateChart revision → Editor；
- generalized Artifact audio source picker；
- EvidenceBundle；
- disagreement review queue；
- human correction capture；
- Candidate/Authored compare/merge UX。

然后才进行：

```text
Native Default
Delete Python
Final Cutover
```

这样 UI/Editor 不会在推理重构后被迫进行第二次大规模数据模型迁移。

---

# 59. 新的最终产品闭环

```text
                Processing Studio
                       │
                 WorkflowDefinition
                       │
                       ▼
                Workflow Compiler
                       │
                       ▼
                 Compiled DAG
                       │
          ┌────────────┴────────────┐
          ▼                         ▼
      Execution                 Advanced Graph
          │
          ▼
 ArtifactRevision / Evidence
          │
          ▼
 Canonical Singing Track
          │
          ▼
     CandidateChart
          │
          ▼
        Editor
          │
     human review/edit
          │
          ├───────────────► HumanCorrection
          │                       │
          ▼                       ▼
    AuthoredChart          Calibration / MetaModel
          │
          ▼
      Results / Export
```

这应成为 Uta Studio 重构后的完整产品闭环。

---

# 60. Runtime Policy 修订：通用路由 + Qwen Pin Exceptions

以下规则是最终 Runtime Policy。

最终 Runtime Policy 分成两类：

```text
A. Generic Native Runtime Nodes
   OpenVINO preferred
       ↓ unsupported / parity failed / stability failed
   Vulkan
       ↓ unavailable / unvalidated
   Fail Closed

B. Pinned Qwen Runtime Exceptions
   Qwen3-ASR-1.7B
       → transcribe.cpp / GGML Vulkan

   Qwen3-ForcedAligner-0.6B
       → predict-woo/qwen3-asr.cpp / GGML Vulkan
```

CPU 仍然只允许用于：

- reference；
- diagnostics；
- parity；
- development；
- benchmark。

CPU 不作为普通生产 fallback。

Python 永远不作为 production fallback。

---

# 61. Qwen3-ForcedAligner-0.6B Runtime Lock

## 61.1 Runtime source

```text
repository:
  predict-woo/qwen3-asr.cpp

tested_runtime_commit:
  6dcc586e5073fd6e85ee5728e75f0903d6c70c6c
```

该 runtime commit 自己固定的 GGML submodule 为：

```text
9be313313c8ecb9488911bd64550190e3ed80f38
```

这个 GGML revision 是 Forced Aligner CPU reference/test recipe 的固定依赖。

---

## 61.2 Vulkan recipe 是独立的 pinned build recipe

实际 Vulkan 验证不是简单使用 runtime repo 自带的 GGML submodule。

验证组合为：

```text
predict-woo/qwen3-asr.cpp
runtime commit:
  6dcc586e5073fd6e85ee5728e75f0903d6c70c6c

GGML Vulkan override:
  8c63e70982c95ceb862e3a1073a2c1beef75d60a
```

原因：

- 本机当时的 CMake / glslc 环境无法构建 runtime 自带旧 Vulkan backend；
- 因此保留 predict-woo 的 Forced Aligner model graph / implementation；
- Vulkan backend 改为与 `transcribe.cpp` 相同的兼容 GGML revision。

因此 ProductionPinned identity 不能只写：

```text
predict-woo@6dcc586
```

必须写：

```text
runtime_repo      = predict-woo/qwen3-asr.cpp
runtime_commit    = 6dcc586e5073fd6e85ee5728e75f0903d6c70c6c
ggml_commit_cpu   = 9be313313c8ecb9488911bd64550190e3ed80f38
ggml_commit_vk    = 8c63e70982c95ceb862e3a1073a2c1beef75d60a
backend           = vulkan
```

如果为了使用 `8c63e709...` 需要 source patch、CMake patch 或 API compatibility patch：

> 所有 patch 必须进入 Uta Studio vendor tree 并参与 runtime digest。

禁止依赖开发机上的未记录工作树修改。

---

## 61.3 Model lock

```text
model_repository:
  Qwen/Qwen3-ForcedAligner-0.6B-hf

model_revision:
  c07281df297b9905d24a508279258cccf987a064
```

正式 model manifest 还必须记录最终实际分发文件的：

- file name；
- file size；
- SHA-256；
- conversion recipe（若转换为 GGUF）；
- quantization；
- tokenizer/config digest。

模型 revision 与 runtime revision 是两套独立 identity，不能只记录其中一个。

---

# 62. Qwen3-ASR-1.7B Runtime Lock

## 62.1 Runtime source

```text
repository:
  handy-computer/transcribe.cpp

tested_runtime_commit:
  ea077b87590bcfb090d7c38c03ab36cd1c7005d3
```

该 runtime 的 GGML upstream revision：

```text
8c63e70982c95ceb862e3a1073a2c1beef75d60a
```

因此 ASR 1.7B 与 Forced Aligner Vulkan recipe 在 GGML backend revision 上对齐，但模型 graph/runtime implementation 不相同。

---

## 62.2 Model source

原始模型：

```text
repository:
  Qwen/Qwen3-ASR-1.7B

revision:
  7278e1e70fe206f11671096ffdd38061171dd6e5
```

已验证 GGUF：

```text
repository:
  handy-computer/Qwen3-ASR-1.7B-gguf

file:
  Qwen3-ASR-1.7B-Q4_K_M.gguf

sha256:
  b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e
```

因此 ProductionPinned identity：

```text
runtime_repo    = handy-computer/transcribe.cpp
runtime_commit  = ea077b87590bcfb090d7c38c03ab36cd1c7005d3
ggml_commit     = 8c63e70982c95ceb862e3a1073a2c1beef75d60a
model_revision  = 7278e1e70fe206f11671096ffdd38061171dd6e5
gguf_file       = Qwen3-ASR-1.7B-Q4_K_M.gguf
gguf_sha256     = b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e
backend         = vulkan
```

---

# 63. 两个 Qwen Runtime 必须作为两个独立 Runtime Components

不要抽象成一个假的：

```text
QwenRuntime
```

然后在内部依靠模型类型做大量条件分支。

第一阶段应显式维护：

```text
uta-qwen-asr-runtime
    source implementation:
      handy-computer/transcribe.cpp

uta-qwen-align-runtime
    source implementation:
      predict-woo/qwen3-asr.cpp
```

两者可以共享：

- Uta stdio NDJSON envelope；
- process supervisor；
- cancellation；
- progress；
- log format；
- model registry；
- GGML Vulkan device policy；
- artifact commit rules。

但不能假设共享：

- model loader；
- graph implementation；
- tokenizer implementation；
- model metadata；
- CLI flags；
- GGUF conventions；
- internal context lifecycle。

后续如果 `transcribe.cpp` 正式支持 Forced Aligner 且通过相同验收，再考虑 runtime consolidation。

在此之前：

> 不为了“代码看起来统一”而合并两个已经验证成功的实现。

---

# 64. Generic Native Runtime Nodes

除上述两个 Qwen 例外外，其它 inference node 继续走 capability-based routing：

```text
Model Capability
     ↓
Can OpenVINO execute this exact graph
with accepted parity and stability?
     │
     ├─ yes → OpenVINO
     │
     └─ no  → Vulkan
                 │
                 └─ if unvalidated → fail closed
```

该规则适用于当前及未来的：

- RoFormer-family separation / restoration；
- RMVPE；
- FCPE；
- GAME；
- STARS；
- Basic Pitch；
- FireRed family；
- VocalParse；
- 其它新增 native experts。

“可以 OpenVINO 或 Vulkan”不代表可以随运行随机切换。

每一个 `(model revision, runtime backend)` 组合仍然必须经过独立：

- numerical parity；
- artifact parity；
- real-song regression；
- repeated stability；
- cancellation；
- memory bound；
- Intel GPU validation。

---

# 65. Runtime Capability 不等于 Runtime Preference

Model Registry 应记录：

```rust
struct ModelRuntimeCapability {
    runtime_kind: RuntimeKind,
    validation_state: ValidationState,
    evidence_id: Option<String>,
}
```

例如：

```text
RMVPE
├─ OpenVINO  ProductionPinned
└─ Vulkan    BenchmarkCandidate
```

Router 才能解析：

```text
OpenVINO
```

另一个模型可能：

```text
STARS
├─ OpenVINO  Unsupported
└─ Vulkan    ProductionPinned
```

Router 解析：

```text
Vulkan
```

Qwen 例外则：

```text
Qwen3-ASR-1.7B
└─ PinnedRuntime(transcribe.cpp Vulkan)

Qwen3-ForcedAligner-0.6B
└─ PinnedRuntime(predict-woo Vulkan recipe)
```

---

# 66. RuntimeLock Manifest

构建与运行不能继续依赖散落在文档里的 commit。

仓库应增加 machine-readable lock，例如：

```text
native-inference/runtime-lock.json
```

它必须进入：

- build input；
- diagnostics；
- About / runtime report；
- validation evidence；
- cache signature；
- bug report bundle。

RuntimeLock 至少记录：

```text
component id
runtime repository
runtime commit
backend
GGML commit
patch digest
model repository
model revision
model file
model hash
validation profile
```

运行时日志启动帧必须打印 runtime identity，但不得打印无关用户路径。

---

# 67. Cache / Artifact Signature 必须纳入 Runtime Recipe

此前 artifact config signature 已包含：

- node；
- algorithm version；
- normalized parameters；
- input content hashes；
- model digest。

Native cutover 后应进一步保证：

```text
runtime_recipe_digest
```

参与能够影响数值输出的 cache identity。

特别是 Forced Aligner：

```text
predict-woo@6dcc586 + ggml@9be...
```

与：

```text
predict-woo@6dcc586 + ggml@8c63...
```

不能默认视为相同 runtime identity。

即使模型相同，也必须能够区分和审计。

---

# 68. Diagnostics 输出要求

诊断报告应明确显示：

```text
Qwen ASR
  runtime: transcribe.cpp
  runtime commit: ea077b8...
  ggml: 8c63e709...
  model: Qwen3-ASR-1.7B
  gguf: Q4_K_M
  model sha256: b7afe367...
  backend: Vulkan
  status: validated / unvalidated

Qwen Forced Aligner
  runtime: predict-woo/qwen3-asr.cpp
  runtime commit: 6dcc586...
  ggml: 8c63e709... (Vulkan override)
  upstream-pinned ggml: 9be3133... (CPU reference)
  model revision: c07281d...
  backend: Vulkan
  status: validated / unvalidated
```

这样以后用户提交 crash report 时，不会出现：

> “Qwen 跑崩了，但不知道是哪一个 qwen runtime / ggml build。”

---

# 69. UI Runtime 展示修订

Processing Studio 普通用户仍然不管理底层 commit。

Node 显示：

```text
Qwen3-ASR-1.7B
Vulkan · transcribe.cpp
```

```text
Qwen3 Forced Aligner
Vulkan · native aligner
```

Advanced Inspector / Diagnostics 才显示完整 runtime lock。

其它模型继续显示：

```text
RMVPE
OpenVINO · Intel GPU
```

或：

```text
Some Expert
Vulkan · Intel GPU
```

不要给普通用户展示 GGML commit 选择器。

---

# 70. 最终 Runtime Matrix 原则

最终架构应理解为：

```text
                         Runtime Router
                              │
           ┌──────────────────┴──────────────────┐
           │                                     │
    Generic Native Models                  Pinned Exceptions
           │                                     │
  OpenVINO → Vulkan                     Qwen3-ASR-1.7B
           │                             transcribe.cpp
           │                             GGML Vulkan
           │
           │                             Qwen3 Forced Aligner
           │                             predict-woo
           │                             GGML Vulkan override
           │
           └──────────────────────┬──────────────┘
                                  ▼
                       Common Worker Protocol
                                  ▼
                          Rust Control Plane
```

这是最终且唯一的 Qwen runtime 解释。

