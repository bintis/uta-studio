# Uta! Studio — 全模型真实整曲 XPU 运行结果

2026-09-11：**18/18 个模型资源完成真实整曲执行**，不代表准确率、完整模型数值一致性、听感、Studio 路由或生产验收。用户授权记录：`20260911T063344-cba0c64793b8`。

## 输入、设备与证据

- 原曲：`/home/bintis/Documents/uta!/崔子格 - 卜卦.flac`，**216.88 秒**，44.1 kHz 双声道；源媒体及已安装模型/运行库保持只读。
- 原生 LibTorch **2.13.0**，明确选择 **`libtorch_xpu` / device 0 / Intel Arc B580 / xe `0000:07:00.0`**。没有 CPU/GGML 模型推理回退。
- 六个分离/清理模型使用既有 `mixed_attention`：FP32 主计算和显式 FP16 attention；其余模型使用 `strict`。不是全模型 FP32 的统一成绩。
- 原曲先经 XE90 分离；下游始终读取未削波的浮点人声。JBM555 另读真实原曲。ROSVOT 使用本次 RMVPE 与 Qwen 转录/对齐。STARS 使用明确的 FireRed 中文转录 → Qwen 对齐分支及同一真实 RMVPE，不制造音高、歌词或音素，不替换产品主转录器。
- 全部记录位于 `test-artifacts/libtorch-xpu-fullsong-real/`。`summary.json` 列出每个模型的请求、结果目录、启动提交、operation、观察记录及当前发布文件；汇总操作 `20260911T080342-2add780ca14d`。
- 每次 GPU 调用前记录并查看主机负载及 `nvtop`，运行中持续观察。其他用户进程未被终止，也没有轮询等空闲、电源/频率变更或 Vulkan 压测。负载明显变化，耗时不能用作受控性能对比。

## 逐模型结果

下表全部为完整 216.88 秒输入。秒数为 `execution_seconds`：模型封装执行、后处理、模型销毁及当时的音频发布；不含初始解码/重采样和运行库/权重加载。最终 32 位 FLAC 重新发布另行记录，未重复推理。

| 模型资源 | 执行秒数 | 实际输出 |
| --- | ---: | --- |
| `bs_roformer_leap_xe90_vocals` | 43.915 | 完整人声及残差伴奏 |
| `bs_roformer_leap_xe90_instrumental` | 44.825 | 独立 checkpoint 的完整伴奏及残差人声 |
| `bs_polarformer_public_instrumental` | 46.626 | 修复后完整伴奏及残差人声 |
| `melband_roformer_harmony` | 51.225 | 完整 lead 及 residual |
| `melband_roformer_denoise_aufr33` | 85.550（受竞争影响） | 完整去噪人声；同配置复测 **48.405 秒**，见下节 |
| `melband_roformer_dereverb_anvuew` | 27.041 | 完整去混响人声 |
| `rmvpe` | 4.758 | 21,689 帧，保留真实 voiced 决策 |
| `fcpe` | 0.705 | 21,689 帧 |
| `basic_pitch` | 1.546 | 18,680 帧 |
| `game_1_0_3_small` | 9.862 | 21,688 帧，424 音符 |
| `game_1_0_3_medium` | 14.579 | 21,688 帧，448 音符 |
| `game_1_0_3_large` | 19.981 | 21,688 帧，446 音符 |
| `jbm555_cectc_80` | 4.324 | 真实双输入，12 音符 |
| `qwen3_asr_1_7b` | 33.868 | 完整转录：30 段、521 token，无 unfinished window |
| `qwen3_forced_aligner_0_6b` | 5.434 | 368 个真实识别词的对齐结果，其中 128 个标记为未解决 |
| `firered_asr2_aed` | 19.309 | 完整转录：163 窗、150 token，无 unfinished window |
| `rosvot` | 3.173 | 40,665 帧，357 音符 |
| `stars` | 4.216 | 40,665 帧，177 音符、55 个 style 项及 348 个 technique 项 |

通常结果目录为 `cases/<resource>/`；PolarFormer 通过目录为 `cases/polar-equal-width/`，STARS 为 `cases/stars-real-lexicon/`。原失败目录均保留。四个双输出分离器的完整浮点重构最大绝对误差均为 `5.960464477539063e-8`；这仅说明残差重构，不代表分离质量。

## 去噪耗时复核：85.550 秒不是正常性能基准

用户指出去噪计时异常后，先读取既有日志和 GGUF 元数据，再进行一组明确的整曲对照；本次复核未修改推理实现、权重、输入、重叠或精度，也没有停止其他用户进程。

| 项目 | 去噪原记录 | 去噪复测 | Harmony 同期对照 |
| --- | ---: | ---: | ---: |
| `execution_seconds` | 85.550 | **48.405** | **46.162** |
| 原生 forward 及同步（秒） | 75.588 | 39.396 | 37.152 |
| 分块数 | 115 | 115 | 115 |
| 目标 CCS 客户活动均值（%） | 39.06 | 68.85 | 73.80 |
| Hyprland RCS 客户活动均值（%） | 29.53 | 8.49 | 4.55 |

- 去噪、Harmony、去混响的声明网络参数均为 228,203,172 个，dim 384、depth 6、60 bands、8 heads、head dimension 64，首层与 buffer 形状相同。块长均为 352,800 samples（8 秒）。去噪/Harmony 默认重叠 4、步长 2 秒；去混响重叠 2、步长 4 秒，因此本曲分别执行 115/115/57 块。不能直接拿去混响的 27.041 秒作为同工作量对照。
- 去噪原记录中，原生 profile 外只有 0.777 秒；不是 FLAC 发布、模型加载或文件写入造成那几十秒差异。`compute` 是含提交、等待和同步的原生 forward 墙钟，不是纯 GPU 核计时。
- 原去噪的 83 个 during-target 主机区间持续观察到两个播放器的 RCS 客户活动，均值分别约 14.25% 和 8.49%；其原生 forward 比同样 115 块的原 Harmony 多 34.366 秒，其他阶段合计接近。复测期间外部图形负载较低，耗时降至与同期 Harmony 接近，**支持资源竞争显著污染原计时的判断，不是一次模型代码优化**。
- 原去噪使用 `runtime/` 与 `diagnostic-build/`；复测两者统一使用已有的 `runtime-equal-width/` 与 `publication-build/`。Mel 模型的 Q/K/V 等宽，不进入 PolarFormer 的补零分支；发布封装已有前述 32 位 FLAC 修复。因此与首次运行不是逐二进制、逐环境隔离的因果实验，不能把全部下降量精确归给某一个变化。
- 引擎数字是每个可用区间中对应 owner/engine 的最大客户占比，再取均值；缺失区间不补零，不将不同客户/引擎相加，不等同于整卡或 XMX 利用率。复测仍有桌面活动及约 5.6–5.9% 主机 iowait；单次串行对照不能精确分摊每个外部进程的影响，也不是隔离环境的吞吐保证。
- 去噪及 Harmony 两个 stem 共 **57,386,448** 个浮点输出值与原记录完整比较：均有限，最大绝对差 **2.384185791015625e-7**；去噪 RMSE **1.0624977683274984e-8**，不是逐位一致。没有用减重叠、缩短上下文、降低精度或后台切换换取耗时下降。原始输出和当前 `publications/` 选择均保留。
- 复测操作：去噪 `20260911T085338-756565e19f01`、Harmony `20260911T085516-05b86b080bf2`；完整对照 `20260911T085734-edc777d4d7df`。顶层观察错误均为空，采样读取错误分别为 1/0。前一准备调用 `20260911T085225-e34fd18beaa0` 因观察目录父目录缺失而失败，堆栈位于目标启动之前；修正目录准备后才执行模型，并非模型失败重试。

证据：`test-artifacts/libtorch-xpu-fullsong-real/denoise-timing-review/{geometry,analysis,confirmation}.json` 及其 `cases/`、`observations/`。原始 `summary.json` 的 85.550 秒不改写；本次问题的最新实测为 **去噪 48.405 秒、Harmony 46.162 秒**，均为同一 216.88 秒完整输入。

## 实现与修复

1. **完整语音封装与真实依赖诊断**：`eb2f154`、`8f388a8`、`5052f73` 将共享的元数据、前端、分窗、tokenizer/拼接及时间戳处理与 GGML 执行解耦，加入原生 Qwen ASR/对齐、FireRed 整曲封装及独立 Rust `native_audio_check`。学习图仍在所选 XPU 上执行。`50eaeeb` 修正 STARS/ROSVOT 的具名分数进度；`a02724c` 按真实 RMVPE voiced 决策处理无声条件帧。
2. **STARS 中文词典**：首次整曲调用因缺少“吹”的读音失败（`20260911T071629-b6d4ed3bb05a`）。离线语言数据导出器 `17ac2b6` 与词典/回归 `6cc0725` 正确展开缩合韵母、恢复 ü；源数据 41,923 个字符及 47,111 个词组均可表示，不是手工补本曲几个字。导出器不参与运行时模型推理。修复后真实整曲操作：`20260911T072711-bd79ff37e5ef`。
3. **PolarFormer attention**：首次整曲在两个分块后返回 `UR_RESULT_ERROR_DEVICE_LOST`。观察器记录 exit 1、203.339 秒，显存峰值 11,858,404 KiB，GTT 峰值 8,743,824 KiB（分别采样，不相加为同时总量）。外层工具超时，operation `20260911T073148-8a009eb49aea` 缺少完成记录；不能伪造其完成状态或推断未启动。内核日志因权限不足不可读，驱动故障根因未确定。
   - `58f05b5` 针对 Polar 编码将 Q/K 加倍但不加倍 V 的形状，对 V 精确补零，使 oneDNN 使用等宽融合路径，再裁去输出零列；完整上下文、原始缩放和精度策略不变。
   - CPU 双精度 oracle、XPU 小输入及普通 attention/gating 回归通过。整曲修复操作 `20260911T074346-03847548b76b` 完成，显存峰值 **3,421,512 KiB（约 3.26 GiB）**，GTT 峰值 22,608 KiB。不能据此宣称退出后的主机稳定性。
4. **Harmony 旧失败输入**：本曲通过后，用当前代码回查原 `20260910T215111-9b8cb57be73d` 的同一真实 12 秒输入；`20260911T074916-eb7f1de1987f` 通过并保留全部输出。旧非有限 mask 在当前代码下未复现；不把它归因为只影响不等宽 attention 的 PolarFormer 修复。
5. **音频发布**：`826279e` 保留精确 `.f32`/浮点 WAV，并仅为整数 FLAC 编码施加明确记录的防削波增益。实解码又发现 FFmpeg 默认将请求的 32 位降为 24 位；`393590e` 显式启用编码器所需的 `-strict experimental`，并加入真实 STREAMINFO 位深与完整解码回归。该选项仅影响 FLAC 编码，不改变推理精度。

## 音频发布验证

当前发布文件只选用 **`publications/<case>/<stem>/audio.flac`**；旁边的 `audio.f32` 与 `publication.json` 保留未缩放的精确浮点输出和编码增益。`publication-requests.json` 指向只读的原始推理 WAV。

- 12 个发布文件：10 个整曲 stem，加上 Harmony 历史片段的两个 stem。
- 全部用 FFmpeg 实际解码，检查 codec、扩展名、采样率、声道、实际 **32 位**位深及每个样本；总计 **193,404,960 个值**。
- 与原始浮点输出乘以声明增益的最大绝对差为 **`2.3283064365386963e-10`**；检测到的削波样本为 **0**。
- 记录：`flac-verification.json`，操作 `20260911T075950-a2278c838d18`。原削波、24 位发布及失败验证记录保留，不再作为当前发布文件。重新发布没有再次运行模型。

## 验证范围与剩余工作

- 最终 Rust 定向检查 `20260911T080254-73861fd161fa`：格式通过；**123 tests passed、7 ignored**。忽略项是一个显式 CPU 前端计时测试及六个需独立 GGML/参考夹具的 Qwen 测试，未冒充通过。另有三个离线词典导出测试、原生 CPU primitives、XPU Polar attention 与普通 gating 回归。
- 当前模型运行全部有独立完成记录；观察器无顶层错误，但部分运行有 1–4 个采样读取错误，详见 `summary.json`。采样不能证明无短暂活动、无驱动重置或持续主机稳定。
- Qwen 转录存在英文猜词；FireRed 也未做准确率验收。ROSVOT 仅使用 240 个已解决真实词边界；STARS 的 FireRed 对齐分支使用 124 个，另有 46 个未解决。所有遗漏均记录，未伪造时间戳或读音。
- **仍未完成**：完整模型数值/听感与文本质量验收；Studio/Runtime Manager/Analysis Engine 的 LibTorch 正式路由；生产资格。现有 `integration_ready` 与 `production_ready` 不因本次诊断自动提升。
- 没有执行整工作区、Nix 打包、正式发布或编辑器连续试听验收。原有 `docs/XPU_CAPACITY_STUDY.md` 与 `tools/test_summarize_attention_study.py` 改动保持不动。

参见 [执行设计](design/runtime/LIBTORCH_EXECUTION.md)、[任务状态](../tasks/remaining-models/STATE.md) 与 [操作记录规则](ROFORMER_OPERATION_RECORDING.md)。
