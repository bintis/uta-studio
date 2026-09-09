# B580 / Leap XE90 单次全曲测速（2026-09-07）

## 授权与执行边界

用户明确授权：“跑一次B580 全曲，用现在的，走XE90模型，看看速度”。本次只执行一次当前 Rust `uta-roformer-worker` 的 Leap XE90 全曲；不运行额外预热、上传探针、GGML参照、其他模型、自动重试或压力循环。既有 batch=1、no-async、serial-pipeline、模型默认chunk/overlap均保留，源音频与模型只读。

此前B580整机断电问题未确认解决；这次授权不改变生产路由或READY/production_ready状态。进程完成、数值通过和有限日志观察不代表整机长期安全。

## 输入与程序准备

- 使用此前同一完整输入：354.880秒、44.1kHz、双声道F32 PCM WAV；CPU `ffprobe`已核实，不截片。
- 模型：`bs_roformer_leap_xe90_vocals` 的原始 `bs_leap_xe_voc-F32.gguf`，不更换精度或权重。
- 工作/证据目录：`test-artifacts/roformer-b580-xe90-full-once-20260907/`。准确输入路径、大小、mtime、请求在该目录的 `prepared-inputs.json` / `request.ndjson`。
- 当前已提交的RoFormer/WGPU/GGUF源码与 `6a759cf` 无差异；相关未提交改动仅为先前保留的格式调整。两类diff分开保存，不把HEAD当作整个dirty工作树身份。
- 将定向构建当前Release GPU worker并单独保存二进制与构建记录；不在推理时并行构建。
- 推理前查看 `observe-host-load.py` 和 `nvtop --snapshot`，运行中由现有观测器保留CPU/两卡DRM状态；不终止用户的其他测试。
- 推理进程显式限制Intel ICD `/run/opengl-driver/share/vulkan/icd.d/intel_icd.x86_64.json`，请求 `discrete_gpu`。实际库/设备以执行记录为准；不回退到AMD或CPU。
- 开启既有CPU阶段profile和编码前浮点统计，关闭额外GPU timestamp / shader dump。所有观测开销保留在实际进程墙钟里，不扣除估计值。

准备记录：`operations/20260907T144803-686def895543`；音频探测：`operations/20260907T144803-234b494bda66`。

## 结果：整机稳定性失败，无全曲耗时

用户报告本次黑屏、整机掉电，只能拔电重启。已停止GPU执行，不重试；后续授权是只读对比RoFormer/RMVPE和GGML三个参数，不是新的推理授权。

- Release构建成功，记录 `operations/20260907T144927-a5856255bccc`；执行记录为 `operations/20260907T145143-1572d2f3be51`，commit `970bafa188840f210abc70d9917ea4b6cbc18795`，PID 405995。保存的二进制、请求、dirty差异与原始日志不改写。
- 目标实际于 **2026-09-07 14:51:44.821726 UTC** 创建；stderr确认 **Intel Arc B580 / BMG G21、coopmat_f16_f32、batch=1、vulkan_no_async=true、serial_pipeline=true**。采样库为Mesa 26.2.2 `libvulkan_intel.so`，DRM为xe / `0000:07:00.0`。
- 第 **1/38** 个chunk完整调用耗时 **12.591737732秒**，随后进入第2个chunk的Transformer。最后保留的完整细粒度行是 `resident.freq.ff1: 33.454799ms`；它不确定故障算子，不能认定下一步GELU触发了断电。
- 96条进程采样、20条主机采样，最后时间 **14:52:05.129958 UTC**，距进程创建约20.31秒。这是留存观测范围，不是断电精确时刻。stderr尾部有3184个零字节，目标缓冲与断电可能丢失更多尾部记录。
- 外层和观察器均无 `result.json`，无协议 `done`，输出目录为空。**不能将首chunk时间外推为本次全曲成绩，也没有完整输出的有限性/质量结论。**
- boot从 `a4acf780-23bb-453d-9a68-5bf560b86a55` 变为 `92db7338-5952-489d-979c-cae339072568`。sudo可读journal仅保留当前boot；指定事故boot报无记录。`/sys/fs/pstore`不存在，归档仅有5月目录；本次无可用的故障时内核日志，不是“内核无错误”。当前启动记录有FAT卷未正常卸载提示，未执行修复/删除。
- 运行前CPU约3.3%，nvtop B580约1%、47°C、40W。运行期间可见CPU总占用9.38–42.46%；其他可见GPU客户端主要是桌面，非目标引擎活动峰值约1.86%（Intel）、1.60%（AMD）。不据此宣称整机隔离，也没有故障时温度/功耗曲线。
- 目标DRM客户端采样峰值 `resident-vram0=5502520 KiB`（约 **5.25 GiB**），GTT另列222564 KiB；不是全部客户端总显存，也不是瞬时峰值上界。首chunk结束池中146个buffer共3132.0 MiB。没有证据把此次故障直接归因于12 GiB显存耗尽。

提取结果：上述证据根的 `incident.json`；只读提取操作 `operations/20260907T145758-d090e152f76c`，内核可见性检查 `operations/20260907T145759-8539fd5e6982`。这两项没有创建GPU设备。

## 源码研究：三个GGML参数到底阻断什么

审查固定GGML提交 `8c63e70982c95ceb862e3a1073a2c1beef75d60a`，不是当前上游主分支。事故重启后旧临时源码不存在；只下载研究副本至证据根 `source-audit/`，操作 `20260907T150211-7bdd54c17776` / `20260907T150300-c0b2abbfd76a`，未构建/执行GGML。

| 参数 | 代码真正改变的路径 | 不覆盖的风险 |
| --- | --- | --- |
| `--batch-size 1` | [CLI](../native-inference/roformer/cli/main.cpp#L310)及[Process](../native-inference/roformer/src/roformer_runtime.cpp#L399)拒绝其他值；`RunInferenceBatch`以states数量扩展图张量第三维，位置、输入和输出也乘batch。单chunk避免多个音频chunk合入同一大图。 | 不把一首曲子缩成一个chunk；不限制单chunk的frames、bands、heads、GEMM rows、单次提交工作量、功耗或温度。当前默认本来就是1，旧手动事故也记录batch=1。 |
| `--serial-pipeline` | [Process](../native-inference/roformer/src/roformer_runtime.cpp#L403)改走 `ProcessOverlapAdd → ProcessChunk → PreProcessChunk → RunInference → PostProcessChunk`。否则[三级流水](../native-inference/roformer/src/roformer_runtime.cpp#L823)使用容量各3的输入/输出队列、前处理线程、主线程GPU计算、后处理线程，允许不同chunk的STFT/GPU/iSTFT重叠。 | 不等于CPU单线程；不改变图内shader、Vulkan的计算/传输队列分配或每次提交大小。串行路径减少跨阶段并发和在途chunk内存，但不是硬件限功率。 |
| `--vulkan-no-async` | [EnableVulkanWithoutAsync](../native-inference/roformer/cli/main.cpp#L66)设置 `GGML_VK_DISABLE_ASYNC=1`。固定后端将 `support_async=false`，图末 `ggml_vk_synchronize`，初始化时只将接口的 `get_tensor_async`置空。 | **不等于每次提交后等fence**；没有关闭所有名字带async的接口，也不按此值关闭独立transfer queue、coopmat/F16或fusion。 |

### 关键修正：本项目调用链本来就会图末同步

[GGML `ggml-backend.cpp:444–448`](https://github.com/ggml-org/ggml/blob/8c63e70982c95ceb862e3a1073a2c1beef75d60a/src/ggml-backend.cpp#L444)明确为：

```cpp
enum ggml_status err = ggml_backend_graph_compute_async(backend, cgraph);
ggml_backend_synchronize(backend);
return err;
```

我们的[C++ RoFormer](../native-inference/roformer/src/roformer_runtime.cpp#L547)调用的就是这个同步包装，而非GGML scheduler或直接异步graph接口；输入/位置和输出分别用同步 `ggml_backend_tensor_set/get`。因此在**这条固定调用链**中，no-async增加的图末同步与外层同步重叠，并没有新增“上一个chunk未结束就不准下一个chunk计算”的核心边界。不能据此撤销现有参数，也不能把历史成功归因于它独立防住黑屏。

真正逐次提交等待的是另一个 `GGML_VK_SERIALIZE_SUBMISSIONS`：固定[Vulkan后端的 `submit_after`](https://github.com/ggml-org/ggml/blob/8c63e70982c95ceb862e3a1073a2c1beef75d60a/src/ggml-vulkan/ggml-vulkan.cpp#L17045)才逐次 `waitForFences/resetFences`。CLI的no-async路径先调用 `DisableVulkanDiagnostics()`，**明确清除这个开关**；只有独立的诊断模式设置它。不能混淆 `--serial-pipeline` 与Vulkan submission serialization。

固定GGML图内部仍按节点/FLOP阈值拆提交，让CPU编码和GPU运行重叠（[17025起](https://github.com/ggml-org/ggml/blob/8c63e70982c95ceb862e3a1073a2c1beef75d60a/src/ggml-vulkan/ggml-vulkan.cpp#L17025)）。200 GFLOP是初始估算阈值的一部分，随后还会调整，单个大节点也不由此拆开；它不是严格的每提交200 GFLOP安全上限。

**有无三个参数的历史记录不是三个独立A/B实验。** 当前能直接确认的是它们的代码作用、旧GGML事故确实未开阶段串行，以及此次Rust在单chunk/串行提交/串行阶段下仍故障；不能推断它们中的某一个已经被证明是故障开关。

## 结合RMVPE：已确认的区别与排查优先级

比较当前Rust RMVPE，不把它以前的OpenVINO/GGML证据混入。RMVPE源码仍有既存未提交内容；这是当前文件审查，不把HEAD单独当作它完整的实现身份。

| 层次 | 当前Rust RMVPE | 本次Rust RoFormer XE90 |
| --- | --- | --- |
| 设备与算子 | [lib.rs](../native-inference/rmvpe-native/src/lib.rs#L149)使用普通 `GpuDevice::new`；WGSL F32 Conv/ReLU/Add、GRU和输出linear。 | [lib.rs](../native-inference/roformer-native/src/lib.rs#L133)使用 `new_prefer_coopmat`；启用F16/subgroup/experimental cooperative-matrix等能力。主干GEMM为F16操作数/F32累加，attention仍是FP32 subgroup实现，**不是实验coopmat attention**。 |
| Vulkan资源交接 | 使用普通WGPU拥有/跟踪的buffer和pass，不调用raw coopmat接口。 | [gpu_linear.rs](../native-inference/roformer-native/src/engine/gpu_linear.rs#L26)对N可被256整除的主干linear选raw coopmat：手动VkBuffer/VkDeviceMemory/descriptor，`as_hal_mut`编码，再导入WGPU；并非全部迁移到tracked pooled kernel。 |
| 提交粒度 | [conv.rs](../native-inference/wgpu-runtime/src/gpu/conv.rs#L631)每个range调用 `run_kernel`，其[submit后立即wait](../native-inference/wgpu-runtime/src/gpu.rs#L1367)。残差块保持激活驻留，但**不是把整块合入一个GpuBatch**；块末下载，权重跨窗口缓存。 | [engine.rs](../native-inference/roformer-native/src/engine.rs#L924)把gate、各group的gather/完整attention/scatter装入同一个attention batch，最后一次 `finish`才提交/等待。分group/dispatch不等于分submission。 |
| 输入与工作集 | [engine.rs](../native-inference/rmvpe-native/src/engine.rs#L38)固定256帧、64帧重叠，10ms mel网格，窗口约2.56秒；DeepUnet + 双向GRU，没有轴向全上下文attention。 | 单chunk约20秒，1722 frames × 90 bands，8 heads、head dim64。单个QKV逻辑输出 `154980×1536×4=952197120`字节（908.1 MiB），此外还有F16打包、中间激活与池。两者不是同等强度负载。 |
| 串行是否真实 | 每窗口顺序执行，GPU计算调用同步。 | [separate_track](../native-inference/roformer-native/src/engine.rs#L1855)逐chunk同步调用完整DSP/推理；[GpuBatch::finish](../native-inference/wgpu-runtime/src/gpu/batch.rs#L68)和raw GEMM均submit后poll等待，没有发现漏实现阶段串行或忘记提交后等待。 |

### 1. 优先审查“每次提交装了多少工作”，不是继续增加同名布尔参数

RMVPE每段conv等待；RoFormer多个dispatch可能一起等待。`RESIDENT_MAX_SERIAL_INVOCATIONS=1048576`是相关dispatch的元素/调用切分尺度，不是全command buffer总计算量或运行时间上限，raw GEMM还有自己的tile/grid规则。

XE90一次time attention仅QK/PV乘加按 `4×90×8×1722²×64` 估算已约 **546.6 GFLOP**（忽略softmax等；不是性能测量），各group仍编码进同一个attention batch。留存的 `time.attn.finish`约0.44–0.49秒，是提交和等待的主机墙钟，不是独立GPU timestamp。它说明等待粒度与RMVPE不同，**没有证明超过驱动超时阈值或就是此次触发点**。GGML本身也可能有很大的单节点，不能说它的图阈值保证避免此风险。

### 2. 独立审查raw coopmat的分配/释放/导入边界

[coopmat.rs](../native-inference/wgpu-runtime/src/gpu/coopmat.rs#L747)仍为每次raw GEMM分配A/C大buffer，而权重缓存；清零、F16打包、raw GEMM、可选bias分别有提交等待。C在完整写入且等待后才导入，A清零后才导入；descriptor在等待后释放，raw编码有显式barrier，且**提交仍经WGPU queue，并不是另起一条不受控的raw queue**。此前初始化/feature/池增长修复仍存在，本轮没有发现可以据此认定的新同步缺陷。

相对RMVPE，多出来的是大内存分配/释放频率、手工Vulkan与WGPU跟踪边界，以及XMX/DPAS负载；这些是具体代码审查对象，而不是“Rust语言不稳定”。原池泄漏修复不能撤回，也不能把正常留存池直接当作泄漏。

### 3. 系统层仍需保留，但不能凭现象定责

两者共享ANV/xe不代表执行了同样的指令、分配模式或提交长度。RMVPE完成过全曲不排除共享驱动仅在RoFormer负载下触发的问题。GGML以及用户报告的OpenVINO也曾掉电，意味着Rust专属raw交接错误即使存在，也不足以单独解释所有历史事故。需要拔AC的恢复方式必须保留为板卡/供电保护/固件状态线索，但1200W额定功率、启动前47°C或此次5.25 GiB采样都不能确定或排除这些原因。

本轮结论是**三个参数不提供黑屏保证，Rust并非只打印标志；提交工作量与特殊资源/算子路径是更具体的差异**，不是已找到唯一根因。下一步可继续离线审查这两处的资源寿命与边界；任何后续隔离变量的GPU验证都需重新授权。不得同时改精度、模型chunk、等待和内存策略来“试修”。本轮未修改应用/驱动/参数，未运行构建或GPU测试。
