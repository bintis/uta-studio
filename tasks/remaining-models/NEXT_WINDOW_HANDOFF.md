# 下一窗口交接：模型 GGML 全量迁移完成

**交接时间：** 2026-09-09

**当前分支：** `main`

## 1. 结论

“把全部模型的运行后端对齐到 GGML” 这项工作已经完成。frontend、graph、decoder 和调度全部由 Rust 持有，通过固定 upstream GGML 的 C ABI 执行；仓库里不再有模型专用 C/C++ graph、shim、CLI、独立推理 `.so`、WGPU 或 OpenVINO 产品执行路线，也不再有任何模型转换或模型重写脚本。

Runtime Manager catalog 现为 17 个模型 + 1 个 `ggml_vulkan` 共享库运行时。Studio 仍然只调用打包的 `uta-analyze` / `uta-runtime` 协议。CPU 只是显式参考/诊断通道，不是生产 fallback。

当前逐模型状态、证据与剩余缺口以 `tasks/remaining-models/STATE.md` 为准；跨领域结论见 `docs/KEY_CONCLUSIONS.md`；模型能力与产品角色见 `docs/AUDIO_MODELS.md`。

## 2. 迁移完成表

“GGML 执行已验证”不等于 production-ready。全部 17 个资源目前仍是 `integration_ready=yes` / `production_ready=no`。

| 模型 / 资源 | GGML 迁移 | 产品纵切 | 真实设备证据 |
| --- | --- | --- | --- |
| Leap XE90、PolarFormer、Harmony、Denoise、Dereverb | 已完成 | 已接通 | AMD 780M 全部通过 |
| RMVPE | 已完成 | 已接通 | AMD 780M 通过 |
| FCPE | 已完成 | 已接通 | CPU + AMD 780M 通过 |
| Basic Pitch | 已完成 | 已接通（可选挑战者） | CPU + AMD 780M 通过 |
| GAME small / medium / large | 已完成 | 已接通（Processing Studio 音符/边界卡片可选尺寸，medium 默认） | 三种尺寸 AMD 780M 通过，medium 另有 32 秒两段长输入 |
| JBM555 CE+CTC | 已完成 | 已接通（可选挑战者，双输入） | CPU + AMD 780M 通过 |
| STARS | 已完成 | 已接通（`notes.stars` + `technique.analyze`） | Stage C / Stage E 的 CPU+AMD 780M 对照通过，GGUF 在 AMD 780M 加载成功；端到端 worker 目前只有 CPU 记录 |
| ROSVOT | 已完成 | 已接通（可选挑战者） | CPU + AMD 780M worker 运行通过 |
| Qwen3-ASR-1.7B | 已完成 | 已接通（`speech.transcribe`） | AMD 780M 完整转写运行通过 |
| Qwen3 Forced Aligner 0.6B | 已完成 | 已接通（`speech.align`） | decoder 在 CPU + AMD 780M 对照官方参考通过 |
| FireRedASR2 AED | 已完成 | 已接通（`speech.transcribe.challenger`，可选） | frontend/subsampling/encoder/decoder 各阶段 CPU 对照 checkpoint oracle 通过，模型在 AMD 780M 加载成功；尚缺 AMD 780M 完整转写 |
| Inst V2 | 永久退役 | 不适用 | 不适用 |

Acoustic DSP、Candidate graph、fusion、`rhythm.quantize`、FFmpeg 编解码和 GPU probe 不是模型。

`audio.lead_partition` 仍报告为未实现，它属于 `tasks/final-features/17_LEAD_BACKING_HARMONY_PARTITION.md` 的主唱/和声分离产品特性，不是后端缺口。

## 2.1 Intel B580 真实冒泡结果（2026-09-09）

17 个模型中 **15 个在独显 B580 上真实执行成功**并发布了 typed artifact（全部标注 `backend: ggml_vulkan`；分离类输出均为 FLAC / 44.1 kHz / 立体声 / 精确 12.000 秒且非静音）。整轮 boot ID 保持 `582d1c5a-d3c9-44b6-b2d1-a1dd0f1a0fb6`，未发生重启或整机掉电——这只覆盖本轮有界用例，不撤销历史断电事实。证据：`test-artifacts/b580-all-models-20260909T0910Z/`。

两个未通过：

- **FireRed**：encoder 在 B580 上跑满，但贪心解码只产生非词元 token，拼装出空转写。显式 CPU 通道同样失败，因此是解码器缺陷，**不是设备问题**。
- **Qwen3-ASR-1.7B**：本地只装了 Q4_K_M，catalog 固定 F16（4,083,087,904 B），worker 在设备工作前按尺寸拒绝。

另有一个产品级发现：STARS / ROSVOT / FireRed 当前**已安装的 GGUF 是迁移前容器**（PyTorch 维度序；STARS/ROSVOT 张量名超过 GGML 64 字符上限），upstream GGML 直接打不开。FireRed 已安装文件的 sha256 恰好等于 catalog 固定的 `manifest_sha256`，也就是说 **catalog 目前固定了 runtime 打不开的容器**。迁移后的容器只存在于模型目录之外，仓库里也没有 Rust 版的容器迁移工具。

## 3. 剩余工作（不再是迁移，而是资格验证）

1. STARS 的 AMD 780M 端到端 worker 运行；FireRed 的 AMD 780M 完整转写运行。
2. 仍缺的参考对照：STARS/ROSVOT note evidence、PolarFormer/Denoise/Dereverb/Harmony 的旧实现向量、FCPE layerwise 向量。
3. RMVPE 低置信 onset 差异的中间 tensor 对比。
4. 新晋级的 note / speech 模型的长音频性能与更广素材覆盖。
5. 以上设备覆盖闭合后，再做 Intel B580 最终真实冒泡。
6. 最后统一评估 `production_ready`，并执行显式 release pass（whole-workspace 检查、产品构建、Nix 打包、identity scan、模型子进程扫描）。

## 4. 运行与记录规则

- Rust/native 命令使用 `bash dev.sh`；
- build、check、test、experiment 和 model run 前使用 `tools/record-operation.py`；
- 改动后的代码执行前先提交对应独立改动；
- model run 前分别记录 host load 与 GPU snapshot；
- missing completion record 只表示结果 unknown；
- 不把进程退出成功当作退出后 host stability 证据。

见 `docs/ROFORMER_OPERATION_RECORDING.md`。

## 5. 工作树保护

以下修改是既有用户/其他工作，不属于模型迁移提交，不得覆盖、还原或误提交：

```text
AGENTS.md
```
