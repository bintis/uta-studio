# Uta Studio 原生推理重写计划

状态：执行中
分支：`native-inference`
更新日期：2026-08-22

## 1. 目标

将 Uta Studio 的分析推理从 Python、PyTorch、ONNX Python 包和 Python
常驻分析服务迁移到 C++/Rust 原生运行时，同时保留当前分析 DAG、缓存成果物、
进度事件、失败处理和编辑/导出能力。

迁移的首要原因之一是当前 Intel 图形栈中的 Level Zero context/queue 路径曾导致
桌面黑屏。Linux Intel GPU 默认优先走 Vulkan，但不把 SYCL/Level Zero 一概禁止；
任何使用该路径的候选运行时必须先完成隔离冒泡测试和真实负载稳定性测试，再决定
是否进入支持矩阵。

本计划不授权应用在启动、页面渲染或诊断时下载运行时或模型。所有正式安装仍由
用户在 **Settings > Models & runtime** 中明确发起并确认。

## 2. 阶段边界

### 阶段一：安装运行时并证明模型能跑

阶段一不替换生产分析管线。目标是为每个候选原生后端取得直接运行证据：

1. 固定上游仓库提交和构建参数。
2. 构建并验证 Vulkan 默认运行时；若候选构建包含 SYCL/Level Zero，则单独记录其
   动态依赖并按第 5 节执行隔离冒泡测试，不与 Vulkan 结果混记。
3. 优先采用 Hugging Face 上与现有模型完全同源的 FP16 GGUF 权重。
4. 找不到可信预转换权重时，在隔离的开发转换环境中把现有 checkpoint 转为
   FP16 GGUF；转换工具不是产品运行时依赖。
5. 使用用户已经安装的模型和只读音频进行真实推理，不用空模型或仅加载测试代替。
6. 记录模型来源、版本/提交、许可、文件大小、加载结果、运行时间、内存/显存、
   实际 GPU 后端和设备信息。
7. 仅当模型真实运行通过后，才进入阶段二。

阶段一产物：

- 版本化的 Speech Runtime 二进制；Qwen3 强制对齐候选固定为
  `predict-woo/qwen3-asr.cpp` + GGML/Vulkan，其他 ASR 候选单独记录；
- 版本化的 Uta Studio RoFormer Runtime（直接 GGML/Vulkan）二进制；
- 版本化的 Pitch Runtime（OpenVINO C++ + 原始 `rmvpe.onnx`）二进制；
- 经审计的模型清单和安装清单；
- 真实音频 smoke 记录与兼容矩阵；
- 后续接入 Settings 安装流程所需的命令、校验和错误分类。

### 阶段二：替换应用内 Python 管线

阶段一通过后，新增 Rust 原生分析进程，保留现有经过认证的本机 NDJSON 协议和 Rust
任务队列。逐节点替换后再删除 Python 实现：

1. 保留 Rust 侧分析 DAG、缓存路径、成果物版本、进度和取消语义。
2. 用经验证的原生 Speech Runtime 替换 Whisper、语言检测和 VAD；Qwen3 强制
   对齐使用 `predict-woo/qwen3-asr.cpp` 的独立 GGML runtime，不把 ASR 生成头
   与 Forced Aligner 时间戳分类头混用。
3. 用 Uta Studio 自有的直接 GGML/Vulkan helper 替换 RoFormer 分离与清理节点。
4. 用 OpenVINO C++ 直接加载原始 `rmvpe.onnx`，替换 RMVPE Python 推理。
5. 用 Rust 原生音频分析替换当前 Python 音乐分析辅助代码。
6. 将运行时/模型安装、状态和诊断接入 **Models & runtime**；安装只能由明确确认
   的 mutation/external 命令触发。
7. 所有节点产物和错误路径等价后，删除 Python server、Python vendor 安装、uv、
   Python 包以及不再使用的模型。

## 3. 模型去留与职责

### 最终运行时矩阵

| # | 模型 | 用途 | Runtime Component | 底层 | 阶段一要求 |
| -: | --- | --- | --- | --- | --- |
| 1 | BS-RoFormer Vocals EP317 | 人声 + 伴奏 | RoFormer Runtime | GGML/Vulkan | 同源 FP16 GGUF，真实分离 |
| 2 | MelBand-RoFormer Inst V2 | 高质量纯伴奏 | RoFormer Runtime | GGML/Vulkan | 同源 FP16 GGUF，真实分离 |
| 3 | MelBand-RoFormer Karaoke | 主唱隔离 | RoFormer Runtime | GGML/Vulkan | 同源 FP16 GGUF，真实分离 |
| 4 | MelBand-RoFormer Denoise | 人声去噪/伪影 | RoFormer Runtime | GGML/Vulkan | 同源 FP16 GGUF，真实处理 |
| 5 | MelBand-RoFormer Dereverb | 人声去混响 | RoFormer Runtime | GGML/Vulkan | 同源 FP16 GGUF，真实处理 |
| 6 | Whisper Large v3 | 歌词识别 | Speech Runtime | GGML/Vulkan | FP16 GGUF，真实日语识别 |
| 7 | Qwen3-ForcedAligner-0.6B | 歌词强制对齐 | Speech Runtime | GGML/Vulkan | FP16 GGUF，给定歌词真实对齐 |
| 8 | RMVPE | F0 / 音高检测 | Pitch Runtime | OpenVINO C++ | 原始 `rmvpe.onnx`，真实 F0 对比 |

目标 GPU 拓扑固定如下，不为统一格式而转换 RMVPE：

```text
Intel Arc
├── Vulkan
│   ├── RoFormer Runtime ── GGML ── 五个 RoFormer GGUF
│   └── Speech Runtime  ── GGML ── Whisper / Qwen3 GGUF
└── OpenVINO GPU
    └── Pitch Runtime ── OpenVINO C++ ── rmvpe.onnx
```

Runtime 优先采用模型作者或对应原生项目的最上游模型契约。RoFormer 由仓库内
`native-inference/roformer` helper 直接调用 GGML；其图结构和 GGUF 契约以
`yasoukyoku/BSRoformer.cpp` 的 MIT 实现为来源并保留归属，但不构建、分发或调用
第三方 CLI/runtime。Speech 按模型契约选择原生实现：Qwen3 强制对齐采用
`predict-woo/qwen3-asr.cpp`；`handy-computer/transcribe.cpp` 的 Qwen3-ASR 1.7B
本身不具备 Forced Aligner 的 5,000 类时间戳头，也不进入当前最终八模型矩阵。
`predict-woo` 已合并 ASR 1.7B 和 `--transcribe-align` 支持；一次本地 GGUF
metadata/tensor-name 兼容重打包后，它能在同一 Vulkan 进程加载本地 ASR 1.7B 与
Forced Aligner 0.6B，但整曲日语演唱质量失败，因此只是统一运行时候选，尚不能替换
已验证的独立运行时。两套 Qwen runtime、仓库 revision、模型来源、GGML revision、
实测性能和限制详见
[Qwen runtime and repository validation record](validation/qwen-runtime-validation.md)。
RMVPE 保持原始 ONNX 模型契约并通过 OpenVINO C++ API 直接执行。Rust 负责本机
进程编排、现有命令 API 和成果物适配。

### 删除

| 模型/运行时 | 原因 |
| --- | --- |
| HTDemucs 及其 PyTorch/OpenVINO 变体 | 用户确认不再保留；不进入原生迁移范围 |
| MDX ONNX/Karaoke 2 | 用户确认不再保留；由 RoFormer 卡拉 OK 模型承担对应职责 |
| faster-whisper/CTranslate2 Python 运行时 | 由 CrispASR 替换 |
| Parakeet 与 sherpa-onnx Python 路径 | 不进入最终八模型矩阵 |
| 各语言 Wav2Vec2 CTC 与 whisperx Python 路径 | 由 CrispASR ASR + Qwen3 强制对齐替换 |
| 现有 Python/PyTorch XPU 推理路径 | 随 Python/PyTorch 管线一起删除；这不等于禁止原生运行时以后采用经冒泡测试通过的 SYCL/Level Zero 后端 |

MMS Karaoke 日语对齐模型暂不列入保留清单。CrispASR 上游因模型许可没有提供该
实现；只有找到许可与分发方式都可接受的原生实现，并完成真实兼容测试后才能恢复。

## 4. 精度策略

- 新的 GGUF 主模型统一以 FP16 作为第一验收目标。
- 不把 Q4/Q5/Q8 的“能加载”当成 FP16 兼容证明。
- `rmvpe.onnx` 保持原始模型文件和模型本身声明的张量精度，不转 GGUF，也不为了
  统一格式强制改成 FP16；Pitch Runtime 直接通过 OpenVINO C++ 加载该模型。
- CPU 音频预处理、STFT/ISTFT 和最终 PCM 输出不强行降为 FP16；这不属于模型权重
  精度，盲目降精度会损害音频质量。

## 5. GPU 后端与安全约束

- Speech Runtime 的默认 Linux Intel 构建启用 `GGML_VULKAN=ON`；其他 GPU 后端
  使用独立构建和独立测试记录，不混入默认 Vulkan 产物。
- Qwen3 Forced Aligner 调用必须显式锁定 Vulkan 设备，并在采用
  `predict-woo/qwen3-asr.cpp` 时设置 `QWEN_USE_VRAM=1`；不允许静默回退到另一个
  GPU、OpenVINO 或 CPU。CPU fallback 必须由调用方显式选择。
- Uta Studio RoFormer helper 构建启用 `GGML_VULKAN=ON`，显式关闭 CUDA/SYCL，
  并直接初始化 Vulkan device 0；设备不可见时失败，不允许静默回退 CPU。
  Intel Xe2/Xe3 的默认 GGML 路径使用
  `VK_KHR_cooperative_matrix`，其保守测试开关是
  `GGML_VK_DISABLE_COOPMAT=1`；`GGML_VK_DISABLE_COOPMAT2` 仅控制 NVIDIA
  `VK_NV_cooperative_matrix2`，不能替代 Intel 冒泡。
- Pitch Runtime 使用 OpenVINO C++ GPU plugin；它与 Vulkan runtime 分进程运行，
  不在 RoFormer/Speech 进程中创建 OpenVINO 或 Level Zero context。
- 对所有二进制执行动态依赖扫描，明确记录 Vulkan、Level Zero、oneAPI、SYCL 等
  实际依赖。依赖扫描用于确认测试对象，不用来代替真实推理。
- SYCL/Level Zero 候选路径先在独立 helper 进程中冒泡：枚举设备、加载一个真实
  模型、对短真实音频完成推理、正常释放资源；同时记录超时、退出码、stderr、
  内核/图形栈错误、设备重置和桌面黑屏情况。
- 冒泡通过后，再执行连续多模型推理、取消/超时、进程退出以及真实 chart 播放期间
  的稳定性测试。一次短推理成功不能直接视为生产支持。
- 冒泡或稳定性测试失败时，将该后端标记为当前不支持，并显式回退到 Vulkan 或
  CPU；不得在同一进程内静默切换到未经测试的 Level Zero 路径。
- 不启动 CrispASR HTTP server；Uta Studio 继续使用本地进程命令边界和认证协议。

### 当前主机的已知 Level Zero 基线

[BS-RoFormer XPU 实测记录](validation/bs-roformer-vocals-ep317-xpu-test-record.md)
已经覆盖当前 Intel Arc B580 上的 Python/PyTorch XPU（Level Zero）路径：隔离的
12 秒真实推理可以完成，但重复歌曲测试和 248 秒真实文件测试都曾造成整机硬锁。
因此该现有实现应标记为当前不支持，不能因为短冒泡通过而恢复为生产路径，也不要
无人值守地重复同一全曲实验。

这项结果不预先否决实现和运行时均不同的原生 SYCL/Level Zero 候选后端。出现新的
候选后，应从独立 helper 的短真实音频冒泡重新开始，并把候选版本、实际动态依赖和
结果与上述 Python/PyTorch 基线分开记录。

2026-08-21 的 GGML/Vulkan 原生冒泡也暴露了独立的稳定性失败：EP317 和 Inst V2 的
12 秒调用都返回并写出成果物，但第二次调用数分钟后整机硬锁，旧启动没有正常关机或
最终 GPU 错误记录。详见
[RoFormer Runtime Vulkan phase-one record](validation/roformer-vulkan-phase1.md)。在
`GGML_VK_DISABLE_COOPMAT=1` 的独立延迟观察通过前，当前 Intel Arc 默认 Vulkan 路径
同样不得进入支持矩阵，也不得连续运行剩余模型。

随后五个模型在禁用 cooperative matrix 后都通过了 12 秒短冒泡，但 EP317 对
354.88 秒原始歌曲的持续负载仍在第一分钟内造成整机黑屏和硬重启。测试进程仅链接
GGML/Vulkan，没有 Torch、SYCL、Unified Runtime 或 Level Zero。因此当前失败范围已
扩展为 Intel Arc 上的持续 GGML/Vulkan 负载，不能再把 `GGML_VK_DISABLE_COOPMAT=1`
视为足够的稳定性修复，也不得直接开始 Speech Runtime 的 Intel Vulkan 长负载测试。

随后仓库内直接 GGML helper 固定到官方当前 HEAD
`8c63e70982c95ceb862e3a1073a2c1beef75d60a`（0.20.2），恢复所有
`GGML_VK_DISABLE_*` 功能并加入同步持久日志。一次 354.88 秒 EP317 整曲在逐
submission fence 诊断模式下完成 184/184 块，55,568 对 submission 边界完整配对，
总进程 610.07 秒且设备内存计数释放到 0 B。该结果说明硬锁不是在相同负载下必现，
但串行 fence 改变了执行时序，不能单独作为异步生产路径的稳定性结论。异步快速模式
继续保留逐块上传、计算、回读和保存的同步落盘日志；同一整曲随后以 354.761 秒完成，
总进程 355.332 秒，输出与串行诊断结果逐字节一致，运行窗口未出现内核 GPU 错误。
第二首 `Asphodelos` 的 batch=2 全曲在 159 块中的第 97-98 块计算开始后造成整机硬
重启；最后已累积 60.41%，显存空闲仍稳定在约 8.87 GB，因此不是 OOM 或逐块显存
泄漏。batch=2 计算缓冲为 1,857,807,360 字节，而 batch=1 为 623,780,352 字节。
重启后显式使用 batch=1 跑完 159/159 块，推理 307.169 秒，总进程 308.497 秒，完整
WAV 解码通过且没有 NaN/Inf。当前 Arc B580 只接受 batch=1；更大 batch 仅保留为会
输出持久风险警告的开发参数。

该故障与 Intel IGCIT #1330 中 Arc + GGML/Vulkan 大负载触发黑屏或硬重启、并可由
`GGML_VK_DISABLE_COOPMAT=1` 缓解的报告高度吻合。本机 AMD IOMMU 确实以 translated
模式管理 Arc 所在设备组，但失败前后没有 `AMD-Vi IO_PAGE_FAULT`，所以现阶段把
cooperative-matrix/驱动路径列为首要嫌疑，IOMMU 冲突列为待验证的次要假设。该结果
仍不足以把 Intel Vulkan 提升为生产支持，也不得直接开始 Speech Runtime 长负载。

随后按已知 workaround 在 `--vulkan-fast` 之后显式设置
`GGML_VK_DISABLE_COOPMAT=1`，日志确认 `matrix cores: none`。batch=2 短样本可以
完成，但同一首 Asphodelos 全曲只到 30/159 块（18.25%）便再次造成整机硬重启，
最后边界是第 31-32 块计算开始且没有结束。由此排除“仅关闭 cooperative matrix
即可稳定 batch=2”；当前直接 helper 的 CLI 与公共 API 都只接受 batch=1，并在模型
或 GPU 初始化前拒绝其他值。只有更换了实质不同的驱动/运行时并重新获得高风险测试
授权后，才允许放宽这个约束。

继续把已验证的 EP317 batch=1 人声 WAV 作为输入交给 Denoise FP16 模型时，模型和
1,834-node 图均成功加载，第一个 chunk 的计算、回读与 ISTFT 也成功；第二个
batch=1 chunk 在计算开始后约 3.23 秒造成整机硬重启。此时显存仍空闲约 9.84 GB，
排除模型加载失败、batch=2 压力和显存 OOM。由此 batch=1 只能作为 EP317 当前稳定
边界，不能外推到 MelBand Denoise；Denoise 在当前直接 GGML/Vulkan 栈上的这条异步
路径标记为失败，不得无人值守重跑或进入支持矩阵。后续 OpenVINO GPU 和保守调度
结果属于不同运行时/调度配置，必须按各自验证记录判断，不能反向清除这项失败证据。

2026-08-22 经用户明确授权后，Qwen3 Forced Aligner 使用独立进程、`QWEN_USE_VRAM=1`
和锁定的 Arc B580 Vulkan 设备完成 12.8 秒短片及 305.813 秒整曲。短片 46/46 个
CPU/Vulkan 起止边界一致；两次整曲分别在 4.872 秒和 5.400 秒 runtime 时间内完成，
测试窗口没有 boot 变化或内核 GPU 错误。该 Speech 图使用 `predict-woo` 模型图和
兼容的 GGML `8c63e709` Vulkan 后端，工作负载、图结构和进程生命周期都不同于上述
RoFormer 故障，因此只证明 Qwen 候选路径，不把 Intel Vulkan 或其他模型整体提升为
生产支持。完整证据和版本偏差见
[Qwen runtime validation record](validation/qwen-runtime-validation.md)。

## 6. Hugging Face 权重审计规则

预转换权重只有满足下列条件才可采用：

1. 仓库明确给出原始模型仓库/checkpoint 和转换工具版本。
2. 架构、配置字段、采样率、通道、目标 stem 和 chunk/overlap 语义匹配。
3. 文件是实际 FP16，不以仓库名或文件名代替 GGUF metadata 检查。
4. 记录模型许可、来源 URL、revision 和文件摘要。
5. 使用真实音频通过加载和推理；RoFormer 还要检查输出 WAV 时长、采样率、通道、
   有限采样值和非静音。
6. 权重安装只写入新的版本化目录；不得覆盖或删除现有 `.ckpt`、ONNX 或用户缓存。

若任一条件缺失，则标记为“候选未验证”，不能进入正式模型目录。

## 7. 阶段一测试矩阵

### 运行时层

- Speech Runtime 的 backend/device probe 成功并报告实际 Vulkan 设备；Qwen3
  Forced Aligner 还必须记录 `predict-woo/qwen3-asr.cpp` 和 GGML 的精确 revision。
- `uta-roformer-runtime --help` 成功，并由动态依赖扫描记录实际 GPU 后端；包含
  Level Zero/SYCL 的候选构建另行完成第 5 节的冒泡测试。
- Pitch Runtime 能通过 OpenVINO C++ 加载现有 `rmvpe.onnx`，列出输入/输出张量、
  明确实际 GPU 设备，并执行一段真实人声。
- 所有进程有超时、非零退出码、stderr 保存和取消处理。

### 模型层

- 五个 RoFormer 模型逐个完成加载与至少一段真实音频推理。
- Whisper Large v3 完成真实日语人声识别。
- Qwen3-ForcedAligner-0.6B 完成“给定歌词 + 音频”的独立强制对齐；语言能力按实测
  标注，并按
  [Qwen runtime validation record](validation/qwen-runtime-validation.md) 区分短片
  数值证据、整曲性能证据和使用准确完整歌词的质量验收。
- RMVPE 输出帧数、hop、频率范围、无声段和音符分段与当前结果做对比。

### 资源与稳定性层

- 记录冷启动和热运行耗时、峰值 RAM/显存、实时倍率。
- 连续运行多个模型后关闭进程，确认所测 GPU 后端资源释放且桌面没有黑屏。
- 在应用播放真实 chart 时不得运行高并行构建；最终播放验证按项目规则单独执行。

## 8. 进入阶段二的准入条件

只有同时满足以下条件，才开始删除 Python 代码：

- RoFormer 与 Speech Runtime 在目标 Intel Vulkan 设备上构建并真实运行，Pitch
  Runtime 在目标 Intel OpenVINO GPU 上构建并真实运行；任何其他准备纳入支持矩阵
  的 SYCL/Level Zero 候选路径也必须完成第 5 节的冒泡和稳定性测试；
- 五个保留的 RoFormer 模型都有明确的 FP16 GGUF 结论；
- Whisper Large v3、Qwen3-ForcedAligner-0.6B 和 RMVPE 通过真实 smoke；
- 兼容矩阵没有把“未验证”误写成“支持”；
- 运行时/模型来源、revision、许可与安装位置已记录；
- 已设计出不会在启动或诊断时下载内容的 Settings 显式安装流程。

## 9. 最终完成条件

阶段二完成后执行仓库规定的完整验证：Rust format/check/test、原生 UI 测试与构建、
API registry contract、真实音频 decode、真实 UTZ 与 UltraStar 导出、项目名扫描、
`nix build path:.#uta-studio`，并从 Nix 包装后的可执行文件做 Wayland smoke launch。

在这些检查以及真实模型端到端分析都通过前，“脱离 Python”不得标记为完成。
