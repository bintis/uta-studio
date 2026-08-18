# Uta Studio 通用 UVR 模型框架落地指南

**适用仓库：** `bintis/uta-studio`
**参考实现：** `upseem/uvr5-cli-no-ui`
**文档日期：** 2026-08-18
**目标读者：** 后续负责 Python Analyzer、Rust Core、桌面 UI、模型安装、Intel XPU/OpenVINO 和测试的实施 Agent

---

## 1. 最终架构决策

可以借鉴 `uvr5-cli-no-ui`，把不同模型统一到一套调用协议和参数传递机制中，但**不能把所有模型压平为一组无约束参数**。

最终采用以下结构：

```text
Model Catalog
    定义模型文件、架构、输入输出、许可、后端与参数能力
        │
        ▼
Parameter Schema
    公共参数 + 架构参数 + 模型锁定参数 + 设备策略
        │
        ▼
Audio Processing Settings
    全局设置 / 歌曲配置 / 单次运行覆盖
        │
        ▼
Immutable Run Snapshot
    任务入队时冻结完整模型链、参数、输出绑定和后端策略
        │
        ▼
Processing Plan Executor
    按步骤执行 RoFormer、MDX、Demucs、去噪、去混响
        │
        ▼
Runner Adapters
    Torch MDXC / Torch Demucs / ONNX OpenVINO / CPU fallback
        │
        ▼
Semantic Artifacts
    analysis_vocal / instrumental / karaoke / drums / bass / ...
```

这里的“统一”包含：

1. 统一模型 ID。
2. 统一模型安装和状态检查。
3. 统一参数描述和校验。
4. 统一任务传输协议。
5. 统一 Runner 返回值。
6. 统一进度、设备和 fallback 记录。
7. 统一缓存签名。
8. 统一 UI 参数生成。

这里的“统一”**不包含**：

* 不强迫所有模型使用同一个底层推理库。
* 不允许用户覆盖 checkpoint 的网络结构、STFT 和训练配置。
* 不把 `overlap`、`segment_size` 等同名但不同语义的参数混在一起。
* 不在分析运行时动态访问 UVR 模型目录。
* 不根据输出文件名猜测 stem。
* 不让 `.onnx` 模型伪装成 PyTorch XPU 模型。

---

## 2. 对参考项目的分析结论

### 2.1 值得采用的部分

参考项目已经形成了一个统一 Facade：

```python
separator = Separator(
    common_options...,
    mdx_params={...},
    vr_params={...},
    demucs_params={...},
    mdxc_params={...},
)

separator.load_model(model_filename)
separator.separate(audio_path)
```

其 CLI 将公共参数和 MDX、VR、Demucs、MDXC 四组架构参数传给同一个 `Separator`；`Separator` 再根据模型类型动态加载对应实现。这证明“统一模型入口、架构内部分派”的方向可行。

它还提供了几项可直接借鉴的能力：

| 能力      | 参考项目实现                                 | Uta Studio 应采用的形式               |
| ------- | -------------------------------------- | ------------------------------- |
| 多架构统一入口 | `Separator`                            | `AudioProcessorRunner` 协议       |
| 模型与配置绑定 | checkpoint + YAML 或 hash metadata      | 固定版本 Model Catalog              |
| 架构参数分组  | `mdx_params`、`vr_params` 等             | 类型化、带单位的参数 schema               |
| 模型输出描述  | primary/secondary stem、instrument list | 语义化 Artifact Contract           |
| 多 stem  | Demucs 2/4/6 stem、MDXC multi-stem      | `dict[semantic_role, artifact]` |
| 模型列表    | 合并本地模型表和远程 UVR 表                       | 显式更新的固定模型目录                     |
| 硬件抽象    | CUDA/MPS/DirectML/CPU                  | CUDA/XPU/OpenVINO/CPU 路由        |

参考项目的 `CommonSeparator` 已统一了 Vocals、Instrumental、Drums、Bass、Guitar、Piano 等 stem 名称，并根据模型 metadata 推导 primary/secondary stem；MDXC 也能从 YAML 的 `training.instruments` 和 `target_instrument` 读取模型输出语义。

### 2.2 不能原样复制的部分

#### 运行时联网

参考实现会在模型列表或加载过程中获取远程 `download_checks.json`、UVR metadata 和模型配置，再把多个远程目录动态合并。

这与 Uta Studio 的工程约束冲突：模型只能在 **Settings > Models & runtime** 中经过明确用户动作安装，应用启动、页面渲染、分析运行和诊断都不得隐式下载。

因此：

* 模型目录必须随应用版本固定。
* 分析运行必须完全离线。
* 远程 UVR 模型表只能由开发工具显式导入，不能作为生产运行时依赖。
* 模型 URL、SHA-256、配置文件和许可证必须进入 Uta Studio 自己的 Catalog。

#### 设备自动选择

参考项目自动选择 CUDA、MPS、DirectML 或 CPU，没有 PyTorch XPU 分支，也不会完整记录 requested/actual backend。

Uta Studio 当前已经显式安装 Intel XPU PyTorch wheel，并在安装后验证 `torch.xpu.is_available()`、XPU 矩阵运算和同步，因此底层 XPU 环境基础已经存在。

实施时必须保留 Uta Studio 的显式后端选择，不得恢复“检测到什么就静默使用什么”的行为。

#### 输出依赖文件名

参考项目主要返回文件路径列表，并将 stem 名称和模型名称写进输出文件名。当前 Uta Studio 也通过查找 `"(Vocals)"` 和 `"(Instrumental)"` 来识别输出。

这无法正确支持：

* `Dry`
* `Noise`
* `No Reverb`
* `Reverb`
* `Guitar`
* `Piano`
* `Other`
* 任意多 stem 模型
* 自定义输出文件名
* 非英语 stem 标签

新架构必须根据模型 metadata 和 Runner 返回结构识别输出，不得继续使用 substring 匹配。

#### `mdxc_overlap` 语义不一致

参考实现中：

* RoFormer 分支把 `overlap` 乘以采样率，解释成窗口步长的秒数。
* 非 RoFormer MDXC 分支把 `overlap` 作为除数，使用 `hop_size = chunk_size // overlap`。
* 模型 YAML 自己又包含 `inference.num_overlap`。

这三者不是同一个单位。

当前上游 `audio-separator` 的相应代码仍保留类似行为，因此不能把现有 `mdxc_overlap` 原样变成 Uta Studio 的公开通用参数。

Uta Studio 必须拆成带单位的参数：

```text
overlap_count
overlap_ratio
step_seconds
```

初始版本只支持 `model_default` 和经过验证的 `overlap_count`。

#### 隐藏参数变化

参考 MDXC Runner 会针对短于 10 秒的音频自动修改 `override_model_segment_size`。

任何这种自适应变化在 Uta Studio 中必须：

1. 进入 `resolved_parameters`。
2. 出现在运行历史中。
3. 进入缓存签名。
4. 在 UI/日志中可见。

不得在 Runner 内静默改变有效参数。

#### 其他禁止复制的实现细节

* 不使用可变字典作为 Python 默认参数。
* 不用 `argparse type=bool`。
* 不用部分 MD5 作为文件完整性校验。
* 不在库代码中调用 `sys.exit(1)`。
* 不吞掉单文件异常后返回部分成功结果。
* 不根据文件名判断 RoFormer 架构。
* 不在每一个处理阶段重新编码 MP3。
* 不允许模型包升级时自动改变 Torch wheel。
* 不直接暴露未经验证的网络结构参数。

参考项目代码声明为 MIT；如维护 Uta Studio 专用 fork，必须保留其版权和许可证文本。模型权重的授权仍须单独核验，不能由代码许可证替代。

---

## 3. 本次初始模型范围

首批 Catalog 必须包含以下六类模型：

| Uta Studio 模型 ID                   | 文件                                                   | 架构                      | 用途       |
| ---------------------------------- | ---------------------------------------------------- | ----------------------- | -------- |
| `bs_roformer_vocals_ep317`         | `model_bs_roformer_ep_317_sdr_12.9755.ckpt`          | BS-RoFormer / MDXC      | 人声提取     |
| `melband_roformer_inst_v2`         | `melband_roformer_inst_v2.ckpt`                      | MelBand-RoFormer / MDXC | 高质量伴奏    |
| `htdemucs_6s`                      | `htdemucs_6s.yaml` + `.th`                           | Demucs                  | 六声部分离    |
| `melband_roformer_denoise_aufr33`  | `denoise_mel_band_roformer_aufr33_sdr_27.9959.ckpt`  | MelBand-RoFormer / MDXC | 去噪       |
| `melband_roformer_dereverb_anvuew` | `dereverb_mel_band_roformer_anvuew_sdr_19.1729.ckpt` | MelBand-RoFormer / MDXC | 去混响      |
| `uvr_mdxnet_karaoke_2`             | `UVR_MDXNET_KARA_2.onnx`                             | MDX / ONNX              | 卡拉 OK 伴奏 |

参考模型表将这些模型分别归类为人声、伴奏、六声部、去噪、去混响和卡拉 OK 用途。

已确认的模型配置关系包括：

```text
model_bs_roformer_ep_317_sdr_12.9755.ckpt
  -> model_bs_roformer_ep_317_sdr_12.9755.yaml

melband_roformer_inst_v2.ckpt
  -> config_melbandroformer_inst_v2.yaml

denoise_mel_band_roformer_aufr33_sdr_27.9959.ckpt
  -> denoise_mel_band_roformer_aufr33_sdr_27.9959_config.yaml

dereverb_mel_band_roformer_anvuew_sdr_19.1729.ckpt
  -> dereverb_mel_band_roformer_anvuew.yaml

htdemucs_6s.yaml
  -> 5c90dfd2-34c22ccb.th

UVR_MDXNET_KARA_2.onnx
  -> UVR MDX hash metadata
```

这些配对来自 UVR 模型目录和参考项目模型索引。

BS-RoFormer 配置将目标定义为 `Vocals`，Inst V2 配置将目标定义为 `Instrumental`；它们的输出语义必须由配置读取，而不是由文件名推断。

---

## 4. 不可违反的实施规则

### 4.1 模型 ID 与文件名分离

应用配置和运行协议只能保存：

```json
{
  "modelId": "bs_roformer_vocals_ep317"
}
```

不能保存或判断：

```json
{
  "separator": "model_bs_roformer_ep_317_sdr_12.9755.ckpt"
}
```

checkpoint 文件名是 Catalog 内部实现细节。

### 4.2 模型结构参数锁定

下列字段必须由 checkpoint 对应的 YAML/hash metadata 控制，用户不得覆盖：

```text
architecture type
model dim/depth/heads
frequency bands
sample rate
n_fft
STFT hop length
STFT window length
dim_f
dim_t 的模型默认值
number of stems
instrument ordering
target instrument
compensation factor
mask estimator structure
```

这些参数与权重形状、STFT 输入形状和训练设置直接相关。

### 4.3 分析运行必须离线

只有模型安装命令可联网。

下列操作不得联网：

```text
list_audio_models
validate_audio_processing_profile
preview_analysis_plan
run analysis
retry node
diagnostics
application startup
settings page render
```

### 4.4 后端 fallback 必须是整模型 fallback

禁止：

```text
XPU model
  ├─ some layers on XPU
  ├─ some layers silently moved to CPU
  └─ UI still says XPU
```

允许：

```text
requested: torch_xpu
attempt: torch_xpu
failure: unsupported operation
discard partial output
retry complete model on cpu
actual: cpu
fallback_reason: ...
```

### 4.5 输出必须语义化

Runner 返回：

```python
{
    "vocals": StemArtifact(...),
    "instrumental": StemArtifact(...),
}
```

而不是：

```python
[
    "/tmp/song_(Vocals)_model.wav",
    "/tmp/song_(Instrumental)_model.wav",
]
```

### 4.6 每个有效参数都必须进入缓存签名

只要下列任一项发生变化，就必须失效对应节点缓存：

```text
model_id
checkpoint SHA-256
config SHA-256
runner build/version
parameter values
parameter units
input artifact revision
processing chain order
selected output binding
effective backend
precision
compatibility overrides
```

### 4.7 不开放任意 checkpoint 导入

第一版只允许 Catalog 中经过完整 SHA-256 校验的模型。

原因包括：

* `.ckpt` 可能经过 Python pickle 加载。
* 不可信权重存在代码执行和资源耗尽风险。
* 未知 YAML 无法保证网络结构与权重匹配。
* 未知输出 stem 无法满足 Artifact Contract。

自定义模型导入属于后续独立安全设计，不在本次范围内。

---

## 5. 目标代码结构

### 5.1 Python Analyzer

新增：

```text
app-core/analyzer/audio_models/
├── __init__.py
├── catalog.py
├── catalog.yaml
├── errors.py
├── parameters.py
├── plan.py
├── schema.py
└── configs/
    └── README.md

app-core/analyzer/audio_processors/
├── __init__.py
├── contracts.py
├── executor.py
├── outputs.py
└── runners/
    ├── __init__.py
    ├── base.py
    ├── demucs_torch.py
    ├── mdx_onnx.py
    └── mdxc_torch.py
```

开发工具新增：

```text
tools/import_uvr_audio_catalog.py
```

该工具只用于维护 Catalog，不在应用运行时调用。

### 5.2 Rust Core

新增：

```text
app-core/src/audio_model.rs
app-core/src/audio_processing.rs
```

修改：

```text
app-core/src/config.rs
app-core/src/analysis_profile.rs
app-core/src/analysis_graph.rs
app-core/src/analysis_plan.rs
app-core/src/analysis_artifact.rs
app-core/src/analyzer.rs
app-core/src/vendor.rs
app-core/src/api.rs
app-core/src/lib.rs
```

### 5.3 Desktop

修改：

```text
desktop/src/studio/settings.rs
desktop/src/studio/song_settings.rs
desktop/src/studio/analysis_model.rs
desktop/src/studio/analysis_render.rs
desktop/src/studio/analysis_actions.rs
desktop/assets/i18n/en.json
desktop/assets/i18n/zh-CN.json
desktop/assets/i18n/ja.json
```

---

## 6. Model Catalog 设计

### 6.1 Catalog 必须是机器可读的固定数据

建议使用 YAML 编写、加载后转为规范化结构：

```yaml
schema_version: 1
catalog_version: "2026.08.1"

models:
  - id: bs_roformer_vocals_ep317
    display_name: "BS-RoFormer Vocals EP317"
    architecture: mdxc_bs_roformer
    operation: separate_vocals
    runner: mdxc_torch

    input_contract:
      accepted_roles:
        - source_mix
      channels: 2
      sample_rate_policy: model_native

    files:
      - role: checkpoint
        filename: model_bs_roformer_ep_317_sdr_12.9755.ckpt
        source_id: trvlvr_public_uvr
        sha256: REPLACE_WITH_VERIFIED_FULL_SHA256

      - role: model_config
        filename: model_bs_roformer_ep_317_sdr_12.9755.yaml
        source_id: trvlvr_application_data
        sha256: REPLACE_WITH_VERIFIED_FULL_SHA256

    model_metadata:
      target_stem: Vocals
      expected_stems:
        - Vocals
        - Instrumental

    output_contract:
      Vocals: extracted_vocal
      Instrumental: residual_instrumental

    supported_backends:
      - torch_cuda
      - torch_xpu
      - torch_cpu

    parameter_schema_id: mdxc_roformer_v1
    license:
      status: review_required
      source_attribution: "UVR public model catalog"
```

**CI 必须拒绝以下值：**

```text
REPLACE_WITH_VERIFIED_FULL_SHA256
TODO
UNKNOWN
empty hash
duplicate model ID
missing config
unreviewed production source
```

### 6.2 文件完整性与 UVR metadata hash 分离

Catalog 同时允许保存：

```yaml
integrity:
  sha256: "完整文件 SHA-256"

compatibility:
  uvr_metadata_hash: "UVR 用于查找 MDX metadata 的 hash"
```

两者用途不同：

* `sha256`：安全、安装完整性、缓存身份。
* `uvr_metadata_hash`：仅用于兼容 UVR metadata 索引。

禁止用 UVR 的部分 MD5 代替完整 SHA-256。

### 6.3 Catalog 中必须显式记录架构

禁止通过以下方式识别架构：

```python
if "roformer" in filename.lower():
    ...
```

必须使用：

```yaml
architecture: mdxc_melband_roformer
```

### 6.4 MDX 模型 metadata

`UVR_MDXNET_KARA_2.onnx` 没有配套 RoFormer YAML，其结构参数来自 UVR MDX hash metadata。参考 MDX Runner 依赖以下模型字段：

```text
compensate
mdx_dim_f_set
mdx_dim_t_set
mdx_n_fft_scale_set
primary_stem
```

并且当用户 segment size 与模型 `dim_t` 不一致时，会把 ONNX 转成 PyTorch，导致推理后端和性能路线改变。

Catalog 导入工具必须：

1. 下载或读取经过校验的 ONNX。
2. 计算 UVR metadata hash。
3. 提取该模型对应 metadata。
4. 转成 Uta Studio 自己的规范化结构。
5. 将规范化 metadata 的 SHA-256 写入 Catalog。
6. 不在生产运行时再次访问远程 metadata。

---

## 7. 参数系统设计

## 7.1 四层参数模型

### 第一层：模型锁定参数

来自 YAML/hash metadata，不可修改。

### 第二层：安全公共参数

跨模型使用相同语义的参数：

```text
normalization_threshold
amplification_threshold
precision_policy
memory_policy
```

### 第三层：架构参数

按架构分别校验：

```text
MDX
VR
Demucs
MDXC/RoFormer
```

### 第四层：设备策略

描述用户请求的计算路线：

```text
torch_backend
onnx_backend
precision
fallback_policy
```

## 7.2 参数合并优先级

有效参数按以下顺序解析：

```text
模型锁定参数
    不允许覆盖
        │
Catalog 推荐默认值
        │
全局 Analysis 设置
        │
歌曲 Profile 覆盖
        │
单次运行覆盖
        │
运行时能力解析与 clamp
        │
最终 effective_parameters
```

其中模型锁定参数始终优先，不参与用户覆盖。

### 示例

```python
effective = resolve_parameters(
    model_spec=model_spec,
    global_overrides=global_settings,
    song_overrides=song_profile,
    run_overrides=run_overrides,
    device_capabilities=capabilities,
)
```

返回结果必须包含来源：

```json
{
  "mdxc.segmentPolicy": {
    "value": "model_default",
    "source": "model_default"
  },
  "mdxc.overlapCount": {
    "value": 4,
    "source": "song_profile"
  },
  "runtime.precision": {
    "value": "fp32",
    "source": "backend_resolution"
  }
}
```

## 7.3 参数值类型

Rust 不使用任意 `serde_json::Value`，而使用受限值类型：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(untagged)]
pub enum AudioParameterValue {
    Bool(bool),
    Integer(i64),
    Number(f64),
    Text(String),
}
```

参数存储：

```rust
pub type AudioParameterMap =
    std::collections::BTreeMap<String, AudioParameterValue>;
```

每个 key 必须经过 Catalog `ParameterSpec` 校验。

## 7.4 参数描述结构

```rust
pub struct AudioParameterSpec {
    pub key: String,
    pub value_type: AudioParameterType,
    pub default: AudioParameterValue,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub allowed_values: Vec<AudioParameterValue>,
    pub advanced: bool,
    pub affects_quality: bool,
    pub affects_memory: bool,
    pub affects_cache: bool,
    pub unit: Option<String>,
    pub applicable_backends: Vec<String>,
}
```

## 7.5 参数命名必须包含单位

禁止：

```text
overlap
segment_size
```

使用：

```text
mdx.overlapRatio
mdx.segmentFrames

demucs.overlapRatio
demucs.segmentSeconds

mdxc.overlapCount
mdxc.stepSeconds
mdxc.segmentFrames
```

---

## 8. 各架构参数规范

### 8.1 公共参数

| 参数                              |                    范围 | 是否公开 | 说明                          |
| ------------------------------- | --------------------: | ---- | --------------------------- |
| `common.normalizationThreshold` |                   0–1 | 是    | 峰值归一化上限                     |
| `common.amplificationThreshold` |                   0–1 | 高级   | 低电平放大下限                     |
| `runtime.precisionPolicy`       | `fp32/fp16/bf16/auto` | 是    | 设备精度策略                      |
| `runtime.memoryPolicy`          |   `normal/low_memory` | 是    | 预设，不直接修改模型结构                |
| `output.selectedRoles`          |               role 列表 | 内部   | 控制保存哪些输出                    |
| `output.format`                 |                     无 | 否    | 由 Uta Studio 音频政策控制         |
| `output.sampleRate`             |                     无 | 否    | 由模型和 Uta Studio pipeline 控制 |
| `output.useSoundfile`           |                     无 | 否    | Runner 内部实现细节               |

参考项目把 output format、sample rate、soundfile 和单 stem 也放在公共参数中；Uta Studio 不应把这些全部暴露到 Analysis UI，因为存储编码必须继续服从项目的 FLAC/MP3 政策。

### 8.2 MDX 参数

| 参数                        | 初始策略          |
| ------------------------- | ------------- |
| `mdx.segmentPolicy`       | `model_shape` |
| `mdx.segmentFrames`       | 初始锁定          |
| `mdx.overlapRatio`        | 可配置，需 clamp   |
| `mdx.batchSize`           | 可配置           |
| `mdx.enableDenoisePass`   | 高级            |
| `mdx.invertUsingSpectrum` | 高级            |
| `mdx.hopLength`           | 模型锁定          |

对于 `UVR_MDXNET_KARA_2.onnx`：

* OpenVINO 路线必须锁定与模型 `dim_t` 匹配的 segment。
* 如果 segment 改变会触发 ONNX→PyTorch 转换，则应在 OpenVINO 模式下直接拒绝，而不是静默切换后端。
* 如未来开放 segment override，UI 必须说明它会改变执行路线。

### 8.3 VR 参数

| 参数                        | 范围或选项          |
| ------------------------- | -------------- |
| `vr.batchSize`            | 正整数            |
| `vr.windowSize`           | `320/512/1024` |
| `vr.aggression`           | `-100..100`    |
| `vr.enableTta`            | bool           |
| `vr.enablePostProcess`    | bool           |
| `vr.postProcessThreshold` | 模型支持范围         |
| `vr.highEndProcess`       | bool           |

参考 VR Runner 会从模型 metadata 读取 `vr_model_param`，再使用上述用户参数；模型频带参数仍不可由用户覆盖。

首批六个模型中没有 VR 模型，但 schema 应一次设计完成，以支持未来扩展。

### 8.4 Demucs 参数

| 参数                      | 初始范围                   |
| ----------------------- | ---------------------- |
| `demucs.segmentPolicy`  | `model_default/custom` |
| `demucs.segmentSeconds` | 正数或 `model_default`    |
| `demucs.shifts`         | `0..20`                |
| `demucs.overlapRatio`   | `0.01..0.99`           |
| `demucs.splitEnabled`   | bool                   |

`htdemucs_6s` 输出必须根据 `model.sources` 绑定，而不是只根据数组长度和固定索引绑定。参考实现支持 6 stem，但使用硬编码映射；Uta Studio 应利用模型自身的 source 名称做校验。

### 8.5 MDXC/RoFormer 参数

| 参数                         | 初始策略                              |
| -------------------------- | --------------------------------- |
| `mdxc.segmentPolicy`       | 默认 `model_default`                |
| `mdxc.segmentFrames`       | 高级，首版可禁用                          |
| `mdxc.overlapPolicy`       | `model_default/overlap_count`     |
| `mdxc.overlapCount`        | 从 YAML `inference.num_overlap` 读取 |
| `mdxc.pitchShiftSemitones` | 高级                                |
| `mdxc.processAllStems`     | bool                              |
| `mdxc.batchSize`           | 非 RoFormer 才显示                    |

RoFormer 路线当前代码明确说明 batch size 没有实际使用，因此 UI 不得显示一个看似有效、实际被忽略的 batch 参数。

首版建议：

```text
segmentPolicy = model_default
overlapPolicy = model_default
precision = fp32
pitchShift = 0
```

只有在 CPU/CUDA/XPU golden tests 完成后，才开放高级覆盖。

---

## 9. 持久设置与运行 Snapshot

### 9.1 持久设置保持面向用途

用户配置不应直接保存任意执行图。

```rust
pub struct AudioProcessingSettings {
    pub vocal_model_id: Option<String>,
    pub vocal_cleanup_chain: Vec<String>,
    pub accompaniment_model_id: Option<String>,
    pub karaoke_model_id: Option<String>,
    pub multistem_model_id: Option<String>,

    pub common_overrides: AudioParameterMap,
    pub per_model_overrides:
        std::collections::BTreeMap<String, AudioParameterMap>,

    pub torch_backend: String,
    pub onnx_backend: String,
    pub precision_policy: String,
}
```

### 9.2 入队时生成不可变执行 Snapshot

```rust
pub struct AudioProcessingPlanSnapshot {
    pub schema_version: u32,
    pub catalog_version: String,
    pub steps: Vec<AudioProcessingStep>,
    pub output_bindings: Vec<AudioOutputBinding>,
    pub requested_runtime: AudioRuntimeRequest,
}
```

步骤：

```rust
pub struct AudioProcessingStep {
    pub step_id: String,
    pub model_id: String,
    pub input: AudioInputReference,
    pub selected_output_roles: Vec<String>,
    pub effective_parameters: AudioParameterMap,
}
```

输入引用：

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioInputReference {
    SourceMedia,
    StepOutput {
        step_id: String,
        role: String,
    },
}
```

### 9.3 为什么必须冻结 Snapshot

Uta Studio 当前已经有全局、歌曲和单次运行三层配置解析基础，但现有队列部分路径仍可能在执行时重新读取配置。

新音频模型链必须在任务入队时冻结，避免：

```text
任务入队：BS-RoFormer + 去噪
用户修改设置：Demucs + 无去噪
任务开始执行：意外使用新配置
```

---

## 10. 推荐处理 Profile

### 10.1 高质量制谱

```yaml
id: chart_analysis_hq

steps:
  - step_id: extract_vocals
    model_id: bs_roformer_vocals_ep317
    input: source_media
    outputs:
      - extracted_vocal

  - step_id: denoise_vocals
    model_id: melband_roformer_denoise_aufr33
    input:
      step_id: extract_vocals
      role: extracted_vocal
    outputs:
      - clean_audio

  - step_id: dereverb_vocals
    model_id: melband_roformer_dereverb_anvuew
    input:
      step_id: denoise_vocals
      role: clean_audio
    outputs:
      - dry_audio

  - step_id: extract_accompaniment
    model_id: melband_roformer_inst_v2
    input: source_media
    outputs:
      - instrumental

bindings:
  analysis_vocal:
    step_id: dereverb_vocals
    role: dry_audio

  instrumental:
    step_id: extract_accompaniment
    role: instrumental
```

`analysis_vocal` 继续供应：

```text
pitch.extract
lyrics.preprocess
lyrics.transcribe
lyrics.align
```

### 10.2 卡拉 OK 伴奏

```yaml
id: karaoke_hq

steps:
  - step_id: extract_karaoke
    model_id: uvr_mdxnet_karaoke_2
    input: source_media
    outputs:
      - karaoke_instrumental

bindings:
  instrumental:
    step_id: extract_karaoke
    role: karaoke_instrumental
```

### 10.3 六声部导出

```yaml
id: multistem_6s

steps:
  - step_id: separate_6s
    model_id: htdemucs_6s
    input: source_media
    outputs:
      - vocals
      - drums
      - bass
      - guitar
      - piano
      - other

bindings:
  vocals:
    step_id: separate_6s
    role: vocals

  instrumental:
    expression:
      sum:
        - drums
        - bass
        - guitar
        - piano
        - other
```

### 10.4 不要默认运行所有六个模型

默认分析只执行当前 Profile 需要的步骤。

例如：

* 用户只需制谱：运行人声链和选定伴奏模型。
* 用户只需卡拉 OK 伴奏：运行 KARA 2。
* 用户需要多轨导出：运行 `htdemucs_6s`。
* 去噪和去混响分别可以关闭。

### 10.5 不把不同模型的结果视为互补分解

BS-RoFormer 人声和 Inst V2 伴奏由两个独立模型生成。

禁止假设：

```text
BS vocals + Inst V2 instrumental == original mix
```

这两个结果可以分别作为“分析人声”和“播放伴奏”，但不能用于严格混音重建或相位一致性校验。

若需要可重建的 stem 组合，应使用同一模型的一组输出，例如 `htdemucs_6s` 的六个 stem。

---

## 11. Runner 协议

### 11.1 Python 数据结构

```python
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping


@dataclass(frozen=True)
class StemArtifact:
    role: str
    source_stem_name: str
    path: Path
    sample_rate: int
    channels: int


@dataclass(frozen=True)
class ProcessorResult:
    model_id: str
    architecture: str
    artifacts: Mapping[str, StemArtifact]

    requested_backend: str
    actual_backend: str
    precision: str

    fallback_from: str | None = None
    fallback_reason: str | None = None

    effective_parameters: Mapping[str, object] | None = None
```

### 11.2 Runner 接口

```python
from typing import Protocol


class AudioProcessorRunner(Protocol):
    def run(
        self,
        *,
        model_spec,
        input_path: Path,
        work_dir: Path,
        parameters,
        runtime_request,
        progress_sink,
    ) -> ProcessorResult:
        ...
```

### 11.3 Runner Factory

```python
RUNNERS = {
    "mdxc_torch": MdxcTorchRunner(),
    "mdx_onnx": MdxOnnxRunner(),
    "demucs_torch": DemucsTorchRunner(),
}
```

统一协议不要求统一底层：

```text
RoFormer       -> PyTorch MDXC Runner
Demucs         -> 直接 Demucs Runner
KARA 2 ONNX    -> ONNX Runtime/OpenVINO MDX Runner
```

### 11.4 不解析输出文件名

Runner 在加载模型后先取得模型描述：

```python
descriptor = LoadedModelDescriptor(
    target_stem="Vocals",
    source_stems=("Vocals", "Instrumental"),
)
```

然后使用确定性 custom output names：

```python
custom_output_names = {
    "Vocals": "step_extract_vocals__vocals",
    "Instrumental": "step_extract_vocals__instrumental",
}
```

最终按 descriptor 映射，不按文件名中的括号查找。

### 11.5 原子执行

每个步骤：

```text
创建唯一临时目录
加载并验证模型
运行推理
验证所有要求输出
验证音频可解码
验证长度、采样率、通道
提交缓存文件
写 ArtifactRevision
删除临时目录
```

任一输出失败时：

* 不提交任何该步骤输出。
* 不留下成功 marker。
* 不返回部分 artifacts。
* 发出 `node_failed`。

---

## 12. `audio-separator` 依赖策略

### 12.1 不直接复制参考仓库全部代码

建议维护一个**固定 commit 的 Uta Studio 专用 fork**，只增加必要适配：

```text
load_model_from_spec
explicit device override
XPU support
semantic model descriptor
typed exceptions
network-free load path
progress callback
overlap semantics correction
deterministic output mapping
```

当前 Uta Studio 对 `audio-separator` 使用较宽的版本要求；不同后端还存在不同版本范围，并在最后重新安装 Torch 以避免依赖解析器改变 wheel 家族。

本次改造应：

1. 固定 `audio-separator` fork commit 或精确版本。
2. 固定 Torch 主版本。
3. 固定 ONNX Runtime/OpenVINO 兼容组合。
4. 将 runtime marker 从 `runtime-v4` 升级为 `runtime-v5`。
5. 旧 runtime 检测为不兼容，要求用户显式重建。
6. 安装完成后运行每类后端 smoke test。

### 12.2 fork 必须增加的 API

```python
separator.load_model_from_spec(
    model_path=...,
    architecture="MDXC",
    model_data=normalized_model_metadata,
    config_path=...,
)
```

该 API：

* 不访问网络。
* 不读取远程 `download_checks.json`。
* 不通过文件名识别架构。
* 不通过 hash 再访问远程 metadata。
* 不写模型目录。
* 只读取 Model Setup 已安装并校验的文件。

### 12.3 fork 必须移除的行为

* `sys.exit()`。
* per-file exception swallow。
* 自动下载。
* 自动模型目录刷新。
* 隐式设备选择。
* 隐式参数改变。
* MPS/CUDA 专用 cache cleanup。
* RoFormer filename heuristic。
* 可变默认字典。

---

## 13. 模型安装系统

### 13.1 目录布局

```text
<models_dir>/audio-processing/
├── catalog-version.json
├── bs_roformer_vocals_ep317/
│   ├── model.ckpt
│   ├── config.yaml
│   └── install-manifest.json
├── melband_roformer_inst_v2/
│   ├── model.ckpt
│   ├── config.yaml
│   └── install-manifest.json
├── melband_roformer_denoise_aufr33/
├── melband_roformer_dereverb_anvuew/
├── uvr_mdxnet_karaoke_2/
│   ├── model.onnx
│   ├── normalized-metadata.json
│   └── install-manifest.json
└── htdemucs_6s/
    ├── htdemucs_6s.yaml
    ├── 5c90dfd2-34c22ccb.th
    └── install-manifest.json
```

### 13.2 安装 manifest

```json
{
  "schemaVersion": 1,
  "modelId": "bs_roformer_vocals_ep317",
  "catalogVersion": "2026.08.1",
  "files": [
    {
      "role": "checkpoint",
      "filename": "model.ckpt",
      "sha256": "..."
    },
    {
      "role": "model_config",
      "filename": "config.yaml",
      "sha256": "..."
    }
  ],
  "installedAtMs": 0
}
```

### 13.3 下载流程

```text
用户点击安装
    │
确认模型名称、来源、大小和许可
    │
下载到 .part
    │
校验长度
    │
校验完整 SHA-256
    │
解析并验证 YAML/metadata
    │
原子 rename
    │
写 install-manifest.json
```

### 13.4 模型状态 API

新增：

```text
list_audio_models                 read
get_audio_model_status            read
install_audio_model               external
reinstall_audio_model             external
remove_audio_model                destructive
validate_audio_processing_profile read
preview_effective_audio_params    read
```

所有命令必须加入 `api_capabilities`。

不要为每个模型增加一个 Rust enum variant。安装 API 使用稳定字符串 ID：

```rust
pub fn install_audio_model(model_id: &str) -> Result<(), String>;
```

### 13.5 模型移除

移除必须：

* 用户明确点击。
* 明确显示影响范围。
* 不删除其他模型。
* 不删除歌曲源文件。
* 不删除已经分析完成的歌曲缓存，除非用户另行执行缓存删除。
* 测试中只操作隔离目录。

---

## 14. Python Pipeline 集成

### 14.1 `stems.py`

保留旧函数作为兼容层：

```python
def separate_stems_uvr(...):
    result = run_legacy_profile(...)
    return (
        result.artifacts["vocals"].path,
        result.artifacts["instrumental"].path,
    )
```

新增统一入口：

```python
def execute_audio_processing_plan(
    plan,
    *,
    source_path,
    work_root,
    progress_sink,
) -> AudioProcessingExecutionResult:
    ...
```

删除：

```text
KARAOKE_MODEL 硬编码
(Vocals)/(Instrumental) substring 解析
device == xpu 时强制 CPU 的固定逻辑
```

当前 `stems.py` 硬编码单一 Karaoke 模型、Demucs `htdemucs` 和 XPU→CPU fallback；这些行为应迁移到 Catalog 与 Backend Resolver。

### 14.2 `pipeline.py`

第一阶段保持现有 `stems.separate` 外部节点不变，但内部执行完整 plan。

当前节点只缓存一对：

```text
{hash}_vocals
{hash}_instrumental
```

并用 `separator + options` marker 判断缓存。

改为：

```python
audio_result = execute_audio_processing_plan(...)

analysis_vocal = audio_result.binding("analysis_vocal")
instrumental = audio_result.binding("instrumental")
```

兼容输出继续提交为：

```text
{hash}_vocals.flac|mp3
{hash}_instrumental.flac|mp3
```

同时写新式 execution manifest。

### 14.3 中间音频编码策略

禁止：

```text
source.mp3
 -> separated_vocal.mp3
 -> denoised_vocal.mp3
 -> dereverbed_vocal.mp3
```

这会产生多次有损编码。

正确做法：

```text
source
 -> temporary float/WAV
 -> temporary float/WAV
 -> temporary float/WAV
 -> final cache FLAC/MP3
```

首阶段只持久化 Profile 最终绑定的音频和用户要求的辅助 stem。

如果未来要让每个增强节点拥有独立持久 Artifact，必须先单独解决中间缓存与现有 FLAC/MP3 政策的冲突，不得在本次改造中静默增加多代 MP3。

### 14.4 `server.py`

NDJSON analyze 命令新增：

```json
{
  "type": "analyze",
  "audio_processing": {
    "schema_version": 1,
    "catalog_version": "2026.08.1",
    "steps": [],
    "output_bindings": [],
    "requested_runtime": {}
  }
}
```

迁移期间仍接受：

```json
{
  "separator": "karaoke"
}
```

但要立即转换成 legacy plan，Python 内部后续不再传播旧 separator 字符串。

进度 metadata 增加：

```json
{
  "step_id": "denoise_vocals",
  "model_id": "melband_roformer_denoise_aufr33",
  "architecture": "mdxc_melband_roformer",
  "requested_backend": "torch_xpu",
  "actual_backend": "torch_xpu",
  "precision": "fp32",
  "effective_parameters": {}
}
```

Uta Studio 已有 requested device、actual device、fallback、model、implementation 和 per-node attempt 的持久化结构，应直接复用，不另建第二套进度协议。

### 14.5 `model_setup.py`

改成：

```python
install_audio_model(models_dir, catalog, model_id)
install_audio_profile(models_dir, catalog, profile)
```

模型安装测试当前主要覆盖固定 Whisper、Parakeet 和 Alignment 路由；应新增 Catalog 驱动的模型安装测试。

---

## 15. Rust 配置与兼容迁移

### 15.1 `AppConfig`

新增：

```rust
#[serde(default)]
pub audio_processing: Option<AudioProcessingSettings>,
```

旧字段暂时保留：

```rust
pub separator: Option<String>,
pub separator_segment_size: Option<u32>,
pub separator_overlap: Option<u32>,
pub separator_batch_size: Option<u32>,
pub separator_normalization_pct: Option<u32>,
pub demucs_shifts: Option<u32>,
pub demucs_overlap_pct: Option<u32>,
```

当前配置只接受 `karaoke`、`demucs` 和 `openvino_demucs`。

迁移映射：

```text
karaoke
  -> legacy_karaoke_roformer

demucs
  -> legacy_htdemucs

openvino_demucs
  -> legacy_openvino_demucs
```

### 15.2 迁移规则

* 旧配置加载后生成 `audio_processing` 默认值。
* 不删除旧字段。
* 保存配置时可继续双写一个版本周期。
* 旧历史记录必须继续反序列化。
* 旧 stem cache 继续只读识别。
* 不强制已有歌曲重新分析。
* 不自动删除 authored chart。

现有 cache 回归测试已经明确要求 legacy stem 发现保持只读，并保证 stem 缓存不受 key/BPM 变化影响；新增签名体系必须保留这一行为。

---

## 16. Analysis DAG 迁移

### 16.1 第一阶段：兼容复合节点

保持：

```text
stems.separate
    outputs:
      VocalStem
      InstrumentalStem
```

内部执行多个步骤。

优点：

* 不立即重写 pitch/lyrics。
* 不破坏现有 Analysis Graph UI。
* 可先验证六个模型。
* 可先稳定参数和缓存。

### 16.2 第二阶段：拆成真实节点

新增节点：

```text
stems.vocals
vocals.denoise
vocals.dereverb
stems.instrumental
stems.karaoke
stems.multistem
stems.bind_analysis_outputs
```

建议 DAG：

```text
preflight
├── stems.vocals
│   └── vocals.denoise
│       └── vocals.dereverb
│           └── stems.bind_analysis_outputs
│               ├── pitch.extract
│               └── lyrics.preprocess
│
├── stems.instrumental
│   └── stems.bind_analysis_outputs
│
├── stems.karaoke
│
└── stems.multistem
```

### 16.3 ArtifactKind

新增：

```rust
RawVocalStem,
DenoisedVocalStem,
DereverbedVocalStem,
AnalysisVocalStem,

HighQualityInstrumentalStem,
KaraokeInstrumentalStem,

DrumStem,
BassStem,
GuitarStem,
PianoStem,
OtherStem,
```

保留：

```rust
VocalStem,
InstrumentalStem,
```

作为兼容输出或 alias。

当前 `AnalysisNodeId` 是字符串 newtype，适合增加稳定节点 ID；现有 `ArtifactKind` 和 `stems.separate` 仍只表达 Vocal/Instrumental 两个结果。

### 16.4 Freeze、Bypass、Disable

* `vocals.denoise` 关闭：其下游直接使用 raw vocal。
* `vocals.dereverb` 关闭：其下游使用 denoised 或 raw vocal。
* `stems.karaoke` 关闭：不阻塞制谱。
* `stems.multistem` 关闭：不阻塞制谱。
* Freeze 必须复用指定 ArtifactRevision。
* Bypass 必须明确记录替代输入。
* 不得把“节点关闭”和“缓存命中”表示为同一状态。

---

## 17. 缓存签名

### 17.1 单步骤签名

```python
signature_payload = {
    "schema_version": 1,
    "catalog_version": catalog.version,
    "step_id": step.step_id,
    "model_id": model.id,
    "architecture": model.architecture,
    "model_files": {
        file.role: file.sha256
        for file in model.files
    },
    "normalized_model_metadata_sha256": model.metadata_sha256,
    "runner_build_id": RUNNER_BUILD_ID,
    "input_artifact_revisions": input_revisions,
    "effective_parameters": canonical_parameters,
    "effective_backend": effective_backend,
    "precision": precision,
    "selected_output_roles": sorted(step.selected_output_roles),
}
```

使用规范化 JSON：

```python
json.dumps(
    signature_payload,
    sort_keys=True,
    separators=(",", ":"),
    ensure_ascii=False,
)
```

再计算 SHA-256。

### 17.2 处理链签名

最终绑定签名还必须包含：

```text
有序 step 列表
step 输入边
output bindings
后处理顺序
```

以下两条链必须有不同签名：

```text
vocals -> denoise -> dereverb
vocals -> dereverb -> denoise
```

### 17.3 fallback 与缓存

推荐流程：

```text
解析 requested backend
解析预期 effective backend
查询该后端缓存
开始推理
如发生可恢复 fallback：
    删除部分输出
    解析 fallback backend
    重新计算 signature
    查询 fallback backend 缓存
    无缓存则完整重跑
```

不得把 CPU fallback 结果写进 XPU signature。

---

## 18. Intel XPU 与 OpenVINO 路由

### 18.1 后端命名

使用明确命名：

```text
torch_xpu
torch_cuda
torch_cpu

openvino_gpu
openvino_cpu

onnx_cuda
onnx_cpu
```

不要统一显示成笼统的 `gpu` 或 `xpu`。

### 18.2 PyTorch XPU 模型

适用：

```text
BS-RoFormer
MelBand-RoFormer
Denoise RoFormer
Dereverb RoFormer
Demucs
```

PyTorch 当前官方 XPU API面向 Intel GPU，使用 `torch.xpu.is_available()` 检测；官方入门文档说明推理、FP32、BF16、FP16 和 AMP 均受支持。

第一版策略：

```text
precision = fp32
autocast = false
```

只有逐模型通过 CPU/XPU 对比测试后，才允许：

```text
bf16
fp16
autocast
```

### 18.3 `audio-separator` XPU patch

fork 增加：

```python
Separator(
    ...,
    torch_device_override=torch.device("xpu"),
    onnx_providers_override=None,
)
```

补齐：

```text
configure_xpu
torch.xpu.synchronize
torch.xpu.empty_cache
XPU OOM 分类
XPU memory snapshot
XPU autocast feature detection
```

当前参考 CommonSeparator 只在清理逻辑中识别 MPS/CUDA，没有 XPU。

### 18.4 RoFormer 兼容性

重点验证：

```text
torch.stft
torch.istft
complex tensors
einsum
scaled dot-product attention
rotary embedding
flash attention flag
overlap-add
long audio chunk loop
```

若某模型配置声明 `flash_attn: true`，但 XPU 路线不能使用对应实现：

1. 使用明确的兼容性 override。
2. 将 override 写入 `effective_parameters`。
3. 将其加入缓存签名。
4. 在运行详情中显示。
5. 不静默切换。

### 18.5 Demucs XPU

保留 Uta Studio 直接 Demucs Runner。

修改：

```python
model = get_model(model_name)
```

而不是：

```python
model = get_model("htdemucs")
```

输出：

```python
{
    name: tensor
    for name, tensor in zip(model.sources, sources)
}
```

`htdemucs_6s` 的 accompaniment 可由同一模型的非 vocals stem 相加生成，从而保持该分支内部的相位一致性。

### 18.6 KARA 2 OpenVINO

`UVR_MDXNET_KARA_2.onnx` 不经过 PyTorch XPU，使用 ONNX Runtime OpenVINO Execution Provider。

ONNX Runtime 官方 OpenVINO EP 支持 Intel CPU、GPU 和 NPU，并支持 `GPU`、`GPU.0` 等设备选择。当前文档还建议新版本通过 `load_config` 设置 OpenVINO 属性，而不是继续依赖已经弃用的旧 `precision`、`num_streams` 等选项。

初始配置：

```python
import json
import onnxruntime as ort

provider_options = {
    "device_type": "GPU",
    "load_config": json.dumps({
        "GPU": {
            "EXECUTION_MODE_HINT": "ACCURACY",
            "PERFORMANCE_HINT": "LATENCY",
            "NUM_STREAMS": "1"
        }
    }),
}

session = ort.InferenceSession(
    model_path,
    providers=[
        ("OpenVINOExecutionProvider", provider_options),
    ],
)
```

若 GPU session 构造或首次推理失败：

```text
销毁 GPU session
启动完整 CPU session
actual_backend = openvino_cpu
记录 fallback_reason
```

不要使用 HETERO 来掩盖模型局部 CPU 执行，除非后续能准确审计实际设备分配。

### 18.7 独立 helper process

当前 Uta Studio 的 OpenVINO Demucs 已使用独立 helper process，避免 PyTorch XPU 与 OpenVINO 同进程竞争 Level Zero context，并提供 OpenVINO GPU→CPU fallback。

KARA 2 应复用同一模式：

```text
main analyzer process
    PyTorch/XPU models
        │
        └── subprocess
              ONNX Runtime OpenVINO KARA 2
```

建议新增：

```text
app-core/analyzer/openvino_mdx.py
```

不要把 KARA 2 直接塞进已初始化 XPU 的持久 analyzer 进程。

### 18.8 后端支持矩阵

| 架构                        | CUDA         | Intel 首选       | CPU                     | 首发状态            |
| ------------------------- | ------------ | -------------- | ----------------------- | --------------- |
| BS/MelBand RoFormer       | `torch_cuda` | `torch_xpu`    | `torch_cpu`             | XPU 实验后转稳定      |
| Denoise/Dereverb RoFormer | `torch_cuda` | `torch_xpu`    | `torch_cpu`             | XPU 实验后转稳定      |
| Demucs                    | `torch_cuda` | `torch_xpu`    | `torch_cpu`             | 优先完成            |
| MDX ONNX                  | `onnx_cuda`  | `openvino_gpu` | `onnx_cpu/openvino_cpu` | OpenVINO helper |
| VR                        | `torch_cuda` | 首版 CPU         | `torch_cpu`             | 后续验证            |

---

## 19. UI 设计

### 19.1 Models & runtime

每个模型一行：

```text
BS-RoFormer Vocals EP317
用途：人声提取
状态：已安装 / 缺失 / 校验失败
文件：checkpoint + config
后端：CUDA / XPU / CPU
操作：安装 / 重新安装 / 移除
```

必须显示：

* 模型用途。
* 架构。
* 安装状态。
* 文件完整性。
* 支持后端。
* 模型来源和许可状态。
* 所需磁盘空间。
* 当前 Catalog 版本。

### 19.2 Analysis

使用面向用途的控件：

```text
分析人声模型
人声后处理
  去噪模型
  去混响模型

伴奏模型
卡拉 OK 模型
六声部分离

计算后端
精度策略
内存策略
```

不要默认向普通用户展示：

```text
checkpoint filename
n_fft
dim_f
dim_t
STFT hop
model depth
heads
frequency bands
```

### 19.3 高级参数

按当前选中模型动态生成。

例如选择 Demucs 时显示：

```text
Shifts
Overlap
Segment duration
Split processing
```

选择 RoFormer 时显示：

```text
Segment policy
Overlap count
Pitch-shift inference
```

RoFormer batch size 如实际无效，则不显示。

### 19.4 参数来源

每个参数支持展示：

```text
Global default
Song profile
One-run override
Model default
Runtime clamp
```

UI 不自行重新实现解析规则，而调用 Rust Core 的统一 resolver。

### 19.5 Song Detail

可以镜像 Analysis 默认设置，但必须提示：

```text
这些设置只影响下一次分析；已有制谱数据不会立即改变。
```

---

## 20. 安全与许可

### 20.1 模型来源记录

每个 ModelSpec 必须包含：

```text
source identifier
source page/repository
original filename
full SHA-256
license status
redistribution status
attribution
catalog review date
```

### 20.2 VIP 模型

参考实现会识别 UVR VIP 模型并提示订阅授权。

Uta Studio 首版：

* Catalog 不得包含 VIP 模型。
* 不得通过公开 URL 绕过访问控制。
* 不得打包来源不明的权重。
* 模型许可未明确时，只允许用户自行安装且必须经过单独法律评审；默认不实现这一入口。

### 20.3 YAML 加载

不要直接对模型目录中的任意文件使用不受限 `yaml.FullLoader`。

如果配置中使用 `!!python/tuple`：

* 实现只允许 tuple 的受限 SafeLoader constructor。
* 拒绝任意 Python object tag。
* 加载后转成普通 list/tuple 数据。
* 校验所有键和类型。

### 20.4 checkpoint 加载

只允许加载：

* Catalog 中存在。
* 完整 SHA-256 正确。
* 配套 config 正确。
* 模型 ID 与架构一致。
* 来源已审核。

---

## 21. 错误模型

Python 定义类型化异常：

```python
class AudioProcessingError(RuntimeError):
    pass


class ModelNotInstalledError(AudioProcessingError):
    pass


class ModelIntegrityError(AudioProcessingError):
    pass


class ModelConfigurationError(AudioProcessingError):
    pass


class ParameterValidationError(AudioProcessingError):
    pass


class BackendUnavailableError(AudioProcessingError):
    pass


class InferenceOutOfMemoryError(AudioProcessingError):
    pass


class OutputContractError(AudioProcessingError):
    pass
```

错误响应必须包含：

```json
{
  "type": "error",
  "kind": "model_configuration",
  "node_id": "vocals.denoise",
  "step_id": "denoise_vocals",
  "model_id": "melband_roformer_denoise_aufr33",
  "requested_backend": "torch_xpu",
  "actual_backend": "torch_xpu",
  "message": "..."
}
```

禁止：

* `sys.exit()`。
* 只打印 traceback。
* 返回空输出列表。
* 自动换模型。
* 自动换用户未选择的 separator。
* 把 fallback 当成正常 XPU 成功。

---

## 22. 测试计划

### 22.1 Catalog 单元测试

新增：

```text
test_audio_model_catalog.py
```

覆盖：

* 六个初始模型 ID 全部存在。
* ID 唯一。
* architecture 合法。
* checkpoint/config 配对正确。
* SHA-256 格式合法。
* 无 placeholder。
* output role 唯一。
* input role 合法。
* parameter schema 存在。
* backend support 合法。
* model-locked 字段不可覆盖。
* Demucs YAML 引用的权重存在。
* KARA 2 normalized metadata 存在。

### 22.2 参数解析测试

新增：

```text
test_audio_parameters.py
```

覆盖：

* Global < Song < Run precedence。
* model locked override 被拒绝。
* 不适用架构参数被拒绝。
* `overlapCount` 与 `overlapRatio` 不可混用。
* clamp 后值进入 effective parameters。
* 短音频 adaptive 变化进入 effective parameters。
* 参数 canonical JSON 稳定。
* 参数顺序不影响 hash。
* 参数值变化会改变 hash。

### 22.3 Runner contract 测试

新增：

```text
test_audio_runner_contracts.py
```

覆盖：

* 不解析文件名。
* 输出 role 根据 metadata 绑定。
* 缺少要求输出时失败。
* 多 stem 顺序变化仍正确绑定。
* Runner 不访问网络。
* Runner 不写模型目录。
* typed error 不退出 server。
* 失败不提交部分输出。
* custom output name 可重复确定。
* requested/actual backend 正确。

现有 `test_stems.py` 仍围绕硬编码 Karaoke 模型和文件名进行测试，需要迁移为语义化 Runner mock。

### 22.4 缓存测试

覆盖以下变化都会失效：

```text
model ID
checkpoint hash
config hash
runner version
parameter
parameter unit
backend
precision
chain order
input revision
output binding
compatibility override
```

同时保证以下变化不影响 stem 缓存：

```text
detected key
BPM
lyrics
editor chart changes
```

### 22.5 模型集成测试

为每种架构准备可合法分发的短音频 fixture：

```text
RoFormer fixture
MDX ONNX fixture
Demucs fixture
```

检查：

* 输出可被 ffmpeg 解码。
* 采样率正确。
* stereo 通道正确。
* 时长误差在已批准范围内。
* 无 NaN/Inf。
* 无全静音。
* 峰值合理。
* 所有要求 stem 存在。
* 最终音频格式符合 Uta Studio 政策。

### 22.6 CPU/XPU/OpenVINO 对比

每个模型记录：

```text
CPU output
accelerated output
duration
sample rate
channels
peak
RMS
spectral comparison
approved quality tolerance
runtime
peak memory
```

不得未经基线直接设置统一数值容差。每个模型和精度模式建立单独批准阈值。

### 22.7 内存测试

连续处理至少五个短 fixture：

```text
song 1
song 2
song 3
song 4
song 5
```

检查：

* XPU allocated/reserved memory 不持续线性增长。
* CPU RAM 不持续增长。
* helper process 正常退出。
* 模型引用被释放。
* fallback 后旧设备 context 不被继续复用。

### 22.8 Rust 测试

覆盖：

* 旧配置迁移。
* 旧 JSON 反序列化。
* AudioProcessingSettings round trip。
* Snapshot 在入队后不受配置改变影响。
* Global/Song/Run 参数继承。
* DAG 无环。
* 新 ArtifactKind 可持久化。
* API catalogue 完整。
* legacy cache 只读。
* authored chart 不被自动删除。

### 22.9 UI 测试

覆盖：

* 只显示当前架构参数。
* 模型缺失时分析按钮禁用。
* 安装按钮不会自动触发分析。
* 高级参数 clamp。
* parameter source 显示。
* fallback 显示 actual backend。
* 中文、英文、日文文案齐全。

---

## 23. 分阶段 PR 计划

## PR 1：Catalog 与参数 schema

**目标：** 不改变现有分析结果。

实施：

* 新增 Catalog parser。
* 写入六个模型条目。
* 新增 ParameterSpec。
* 新增开发用 UVR catalog importer。
* 完整 SHA-256 机制。
* 新增 Catalog 单元测试。
* 文档记录来源和许可。

禁止：

* 不修改当前默认 separator。
* 不修改现有 DAG。
* 不启用新模型推理。
* 不改 UI。

完成标准：

```text
Catalog 可离线加载
六个模型通过 schema
无 placeholder
无运行时网络请求
```

## PR 2：统一 Runner 与 CPU 基线

依赖 PR 1。

实施：

* `ProcessorResult`。
* `AudioProcessorRunner`。
* MDXC Torch Runner。
* Demucs Runner。
* MDX ONNX CPU Runner。
* 语义化输出。
* 原子输出。
* typed errors。
* CPU smoke tests。

迁移：

* 现有 Karaoke 模型通过新 Runner 运行。
* 对外仍返回 legacy vocals/instrumental。

完成标准：

```text
现有 separator 回归通过
六模型至少能完成 CPU model-load smoke test
不再依赖输出文件 substring
```

## PR 3：Processing Plan 与统一传参

依赖 PR 2。

实施：

* Rust AudioProcessingSettings。
* Python/Rust Snapshot schema。
* 全局/歌曲/运行参数合并。
* plan validation。
* chain executor。
* effective parameter logging。
* legacy separator 转换。

完成标准：

```text
可执行 vocals -> denoise -> dereverb
可并行执行 instrumental
任务入队后配置被冻结
```

## PR 4：缓存与 Artifact

依赖 PR 3。

实施：

* step signature。
* chain signature。
* ArtifactRevision。
* final output bindings。
* fallback signature。
* legacy marker 读取。
* intermediate temp audio policy。

完成标准：

```text
改变模型或参数只重跑受影响步骤
旧缓存继续可用
失败不留下部分 artifact
```

## PR 5：DAG 与 Desktop UI

依赖 PR 3、PR 4。

实施：

* DAG 新节点。
* ArtifactKind。
* Models & runtime 模型列表。
* Analysis 参数 UI。
* Song Profile。
* Node Inspector。
* i18n。
* API capabilities。

完成标准：

```text
用户可选择模型和参数
缺失模型有直接安装入口
DAG 显示每个步骤、模型和实际设备
```

## PR 6：Intel XPU 与 OpenVINO

必须在 CPU Runner 稳定后实施。

实施：

* audio-separator fork XPU。
* accelerator-neutral GPU helper。
* RoFormer XPU FP32。
* Demucs XPU。
* KARA 2 OpenVINO helper。
* GPU→CPU fallback。
* actual backend telemetry。
* memory tests。

完成标准：

```text
Intel runtime 安装后通过 XPU smoke test
至少 Demucs 与一个 RoFormer 实际运行在 XPU
KARA 2 实际运行在 OpenVINO GPU
fallback 可观测且缓存身份正确
```

## PR 7：质量、打包与文档

实施：

* Golden fixtures。
* CPU/accelerator 对比。
* Nix build。
* runtime-v5。
* 用户指南。
* 模型许可和 attribution。
* 完整 smoke export。

---

## 24. 多 Agent 分工

### Agent A：Catalog 与模型安装

**独占文件：**

```text
app-core/analyzer/audio_models/*
tools/import_uvr_audio_catalog.py
app-core/analyzer/model_setup.py
app-core/analyzer/test_model_setup.py
```

**交付物：**

* 六模型 Catalog。
* SHA-256。
* 模型安装器。
* 安装 manifest。
* Catalog tests。
* 许可记录。

**不得修改：**

```text
pipeline.py
server.py
desktop/*
```

### Agent B：Python Runner 与参数解析

**独占文件：**

```text
app-core/analyzer/audio_processors/*
app-core/analyzer/stems.py
app-core/analyzer/test_stems.py
app-core/analyzer/test_audio_*.py
```

**交付物：**

* Runner interface。
* MDXC、MDX、Demucs Runner。
* output contract。
* typed errors。
* CPU tests。

**依赖：** Agent A schema 冻结。

### Agent C：Rust 配置与 Wire Protocol

**独占文件：**

```text
app-core/src/audio_model.rs
app-core/src/audio_processing.rs
app-core/src/config.rs
app-core/src/analysis_profile.rs
app-core/src/analyzer.rs
app-core/src/api.rs
```

**交付物：**

* 持久设置。
* Immutable Snapshot。
* legacy migration。
* NDJSON。
* API capabilities。
* Rust tests。

**依赖：** Agent A Catalog schema。

### Agent D：Pipeline、缓存和 Artifact

**独占文件：**

```text
app-core/analyzer/pipeline.py
app-core/analyzer/server.py
app-core/src/analysis_graph.rs
app-core/src/analysis_plan.rs
app-core/src/analysis_artifact.rs
app-core/analyzer/test_pipeline_cache.py
app-core/analyzer/test_node_events.py
```

**交付物：**

* Plan executor 集成。
* cache signature。
* ArtifactRevision。
* DAG。
* legacy cache。
* node progress。

**依赖：** Agent B、Agent C。

### Agent E：Desktop UI

**独占文件：**

```text
desktop/src/studio/settings.rs
desktop/src/studio/song_settings.rs
desktop/src/studio/analysis_*.rs
desktop/assets/i18n/*
```

**交付物：**

* 模型安装 UI。
* 模型角色选择。
* 动态参数 UI。
* parameter source。
* fallback route。
* 三语文案。

**依赖：** Agent C API 稳定。

### Agent F：Intel 加速

**独占文件：**

```text
app-core/analyzer/gpu.py
app-core/analyzer/openvino_mdx.py
app-core/analyzer/openvino_separation.py
audio-separator fork
app-core/src/vendor.rs
```

**交付物：**

* runtime-v5。
* XPU device override。
* XPU cleanup/memory。
* OpenVINO KARA 2 helper。
* fallback。
* 硬件测试记录。

**依赖：** Agent B CPU Runner 稳定。

### Agent G：集成验证

**主要职责：**

* 合并后测试。
* 音频 fixture。
* CPU/XPU/OpenVINO 比较。
* Nix build。
* UTZ/UltraStar smoke export。
* 文档一致性。
* 项目名扫描。

Agent G 不应在验证阶段顺便重构架构；发现问题应回传负责 Agent 修复。

---

## 25. 文件冲突规则

以下文件不得由多个 Agent 同时修改：

```text
app-core/src/analyzer.rs
app-core/analyzer/pipeline.py
app-core/analyzer/server.py
app-core/src/vendor.rs
desktop/src/studio/settings.rs
```

如确实需要跨工作包修改：

1. 先由主负责 Agent完成结构变更。
2. 其他 Agent 基于新 commit rebase。
3. 不并行维护两个不同 schema。
4. Catalog schema、Wire schema 和 Artifact role 必须先冻结再并行。

---

## 26. Definition of Done

只有全部满足以下条件才算完成：

* [ ] 六个模型均有稳定模型 ID。
* [ ] 每个 checkpoint 都绑定正确 YAML/hash metadata。
* [ ] 所有模型文件都有完整 SHA-256。
* [ ] 没有生产 placeholder。
* [ ] 分析运行期间无网络访问。
* [ ] 模型安装只能由用户明确触发。
* [ ] 参数分为公共、架构、锁定和设备四层。
* [ ] 不存在含义不明的裸 `overlap`。
* [ ] 有效参数在任务入队时被冻结。
* [ ] 有效参数和来源可在运行详情查看。
* [ ] Runner 返回语义化 artifact。
* [ ] 不再依赖输出文件名 substring。
* [ ] BS 人声、Inst V2 伴奏可独立配置。
* [ ] 去噪和去混响可独立启用、关闭和排序。
* [ ] htdemucs_6s 输出六个正确命名 stem。
* [ ] KARA 2 走 ONNX/OpenVINO 路线。
* [ ] 至少 Demucs 和一个 RoFormer 可实际使用 XPU。
* [ ] XPU/OpenVINO 失败时完整回退 CPU。
* [ ] actual backend 与 fallback reason 被持久记录。
* [ ] backend/precision 进入缓存签名。
* [ ] 中间处理不产生多代 MP3。
* [ ] 旧 separator 配置仍可加载。
* [ ] 旧 cache 只读兼容。
* [ ] 已编辑 chart 不会因重新分析自动删除。
* [ ] 所有新功能加入 API catalogue。
* [ ] 中英文日文文案完整。
* [ ] CPU 单元与集成测试通过。
* [ ] XPU/OpenVINO 硬件 smoke test 通过。
* [ ] 连续多歌曲运行无明显内存增长。
* [ ] 实际音频可被 ffmpeg 解码。
* [ ] UTZ 和 UltraStar 真实导出通过。
* [ ] Nix package build 和 wrapped executable smoke launch 通过。
* [ ] 参考项目 MIT attribution 已保留。
* [ ] 模型许可状态逐项记录。

最终执行仓库规定的验证命令：

```sh
nix develop path:. -c cargo fmt --all -- --check
nix develop path:. -c cargo check --workspace --all-targets
nix develop path:. -c cargo test --workspace --all-targets
python3 -m py_compile app-core/analyzer/*.py
nix build path:.#uta-studio --print-build-logs
```

这些检查以及真实音频解码、UTZ/UltraStar 导出和 packaged executable smoke test 均属于项目现有完成标准。

---

## 27. 明确禁止的快捷实现

后续 Agent 不得采用以下方案：

```text
在 config.rs 中再增加六个 separator 字符串
```

```text
把模型文件名直接写进 AppConfig
```

```text
运行分析时调用参考项目的在线模型目录
```

```text
把 models.md 当成生产模型数据库
```

```text
通过 "(Vocals)" 判断输出
```

```text
给所有模型统一传一个无单位 overlap
```

```text
允许用户修改 n_fft、dim_f、网络深度
```

```text
XPU 不支持就静默 CPU，但 UI 仍显示 Intel GPU
```

```text
在多个清理阶段之间写入 MP3
```

```text
用部分 MD5 代替 SHA-256
```

```text
安装 audio-separator>=某版本而不固定实际行为
```

```text
为了快速支持 XPU，在每个不支持的算子处把 tensor 搬到 CPU
```

```text
未经许可核验直接打包模型权重
```

---

## 28. 实施优先级

真正的最小可用顺序是：

```text
1. 固定 Catalog
2. 参数 schema
3. 网络隔离的 Runner
4. 六模型 CPU 推理
5. Processing Plan
6. 缓存签名
7. Rust 配置和 UI
8. Demucs XPU
9. RoFormer XPU
10. KARA 2 OpenVINO
11. DAG 拆分
12. 质量和打包验证
```

不要从 UI 或 XPU patch 开始。没有 Catalog、参数解析、输出契约和 CPU 基线时，XPU 结果无法可靠验证，缓存也无法建立正确身份。

本次架构的核心交付不是“让六个文件能跑”，而是建立一个以后可安全加入更多 UVR、MDX、RoFormer、Demucs 和音频增强模型的稳定模型平台。
