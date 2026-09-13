# 独立 LibTorch XPU 硬件潜力实验

## 范围与执行方案（2026-09-10）

本实验只生成合成张量、执行原生 C++ LibTorch/ATen 算子，不加载模型，不改变 GGML、已安装运行库、媒体、模型、设置或其他人的未提交探针。独立源码：`tools/xpu-capacity-study/`；证据：`test-artifacts/xpu-capacity-study/`。现有 `tools/libtorch-xpu-probe/` 保持不动。

历史依据：`ROFORMER_B580_ATTENTION.md`、`ROFORMER_B580_INSTALLED_THROUGHPUT.md`、`ROFORMER_B580_DISJOINT_KV.md`、`PERFORMANCE_DIRECTION_2026-09-09.md`。最新 GGML 实际模型时间注意力 36.3168 ms / 15.0498 TFLOPS；最佳独立样本 34.644 ms / 15.777 TFLOPS，并非稳定承诺。历史 GPU 时间不可直接与新的主机同步时间做严格加速比。

1. 记录环境、Git/dirty、boot ID、主机与 GPU 负载。复用已下载官方 `torch 2.13.0+xpu` 的 C++ 头文件/库；只在隔离证据目录补齐该 wheel 声明的原生依赖。记录来源和版本，不安装到系统或生产环境。
2. 编译原生 C++ 基准：GPU event 间隔与同步主机时间分别记录；先小形状数值验证，再固定预热和重复次数。保留所有原始样本、错误和不支持路径。
3. 计算上限：FP16/BF16/IEEE FP32 方阵 GEMM 尺寸扫描；访存上限：大于缓存的设备内复制；实际形状：时间 `[90,8,1722,64]` 与频率 `[1722,8,90,64]` 全上下文 SDPA、布局对照、转换成本对照。禁用静默 CPU/数学 SDPA 回退。另测形状匹配的 QK/PV 与 softmax，不能冒充融合注意力。
4. 区分纯同类型输入的原生 SDPA 和 GGML 的 F32 Q/F16 K/V/F32 输出接口。若 LibTorch 不支持混合类型，明确记录不支持；单独测 Q 转换、输出转换/布局恢复的桥接成本及数值误差，不宣称同精度无损替换。
5. 分离计时与采样：尝试 Level Zero/PTI 硬件计数器，枚举实际 metric groups，记录 XMX、向量 ALU、扩展数学、send/control、线程占用、stall、SLM/L3/DRAM 及频率。不可读取的计数器用 N/A 和实际 API 错误说明；不把 busy 或 TFLOPS 比值冒充各单元的计数器利用率。不更改系统安全权限、时钟或功率设置，不终止其他任务。
6. 快配置做独立确认，保留负例和漂移；根据当前实际频率和明确的数据类型/稠密 FLOP 定义计算百分比，同时给固定标称频率分母。最终报告区分理论峰值比例、实测算子上限比例、硬件计数器比例、历史对照与本轮直接对照。

FLOP 定义：FMA=2 FLOPs；SDPA 只计 QK+PV 的有效 GEMM 工作，`4*B*H*S*S*D`。时间轴 546,561,146,880，频率轴 28,565,913,600。此比值不是把 exp/reduction 算作免费，而是将整个融合执行时间用有效矩阵工作归一化。B580 233 INT8 TOPS 不能直接作为 FP16 注意力分母。RT、光栅、媒体单元不是注意力可任意借用的计算资源。

操作规则：每次实现意图先用 `tools/record-operation.py` 持久化；独立改动提交后执行；原生构建/运行经 `bash dev.sh -c ...`；每个执行记录启动/完成。主机观测不证明独占。缺少完成记录不表示从未启动。

## 执行结果

### 验收状态与异常（2026-09-10，日本时间）

**性能容量测试已有有效结果；各 Xe2 执行单元的工作负载计数器百分比尚未完成验收。本报告不能宣称已经挖满硬件，也不能宣称已经交付了完整的分单元利用率实测。**

17:23:45 出现新启动 `eacf1481-aa6d-4d60-b768-1b4d41acb175`。此前的计数器联动运行与连接中断相邻，之后发现 Git HEAD 对象 `.git/objects/4b/007a1be9def32ef6100f22a9b328399f8cce56` 为空，`git status` / `git log` 报 `bad object HEAD`。不能仅凭时序判断是 GPU 驱动、采样器、供电、内核或其他任务导致重启。journal 只保留当前启动，上一启动内核日志不可用。

已停止新增 GPU 压力测试；未执行 reset、clean、gc、Git 对象修复、系统安全设置修改或时钟/功率修改。其他并行工作的改动未主动覆盖。报告编辑单独记录了意图，但没有在损坏的 HEAD 上继续提交。后续采样须先处理仓库完整性、日志持久化和主机稳定性问题。

### 1. 有效主结果

原生基准为 C++ LibTorch/ATen，运行中没有 Python 解释器参与算子执行。主结果来自 `test-artifacts/xpu-capacity-study/confirm-ylay9t44/series.json`，总操作 `test-artifacts/operations/20260910T080816-50b98e078147/`。确认组预热 12 次，重复 20 组；每组注意力 4 次、GEMM 8 次、复制 16 次。GPU event 包围完整提交区间，可能包含队列空隙，不等同逐 kernel 时间之和；同步主机时间另外列出。分配/初始上传、首次编译/预热和数值验证不计入表中。

| 测试 | GPU event 均值 ± 样本标准差 ms | 同步主机均值 ms | 有效吞吐 | 2850 MHz 容量模型比例 | 2670 MHz 标称模型比例 |
|---|---:|---:|---:|---:|---:|
| 时间 SDPA，FP16，interleaved | 13.1579 ± 0.6982 | 13.3061 | 41.5386 TFLOPS | 35.58% | 37.98% |
| 时间 SDPA，FP16，contiguous | 13.1937 ± 0.6514 | 13.3336 | 41.4259 TFLOPS | 35.49% | 37.88% |
| 时间 SDPA，FP16，含接口桥接 | 16.2576 ± 0.8124 | 16.3823 | 33.6187 TFLOPS | 28.80% | 30.74% |
| 时间 SDPA，返回 interleaved | 13.1760 ± 0.6074 | 13.3103 | 41.4817 TFLOPS | 35.53% | 37.93% |
| GEMM，FP16，8192 方阵 | 13.0629 ± 0.3430 | 13.1942 | 84.1708 TFLOPS | 72.10% | 76.96% |
| GEMM，BF16，8192 方阵 | 12.7331 ± 0.5990 | 12.8939 | 86.3507 TFLOPS | 73.97% | 78.96% |
| GEMM，IEEE FP32，8192 方阵* | 80.9017 ± 0.1047 | 81.0376 | 13.5907 TFLOPS | 93.14% | 99.42% |
| 设备内复制，FP32，512 MiB 输入 | 3.1872 ± 0.1438 | 3.2275 | 336.8884 GB/s | 73.88% of 456 GB/s | 同左 |

*FP32 行来自较早 `gemm-2kkbmf_9`，预热 8、重复 12、每组 8 次，总测量区间约 7.78 秒；不是本次确认组中重新运行的项目。其完整原始数据与 23 个测量区间内 nvtop 快照仍保留。

复制按一次读加一次写共 1 GiB / 耗时计算，73.88% 是**有效复制字节吞吐与标称显存带宽之比**，不是 DRAM 硬件总线计数器占用率。表中所有 TFLOPS 百分比均为算术工作量/时间/容量模型，**不是 XMX、ALU 或 EU active 计数器**。

### 2. 分母、硬件与计算口径

Intel 官方 B580 规格给出 20 Xe-core、160 XMX、160 Vector Engine、2670 MHz 图形频率及 456 GB/s 显存带宽；官方架构指南给出每 Vector Engine 8 个硬件线程，合计 1280。Level Zero 本机枚举为 5 slices × 4 subslices × 8 EUs，device ID `0xe20b`，报告 core clock 2850 MHz。确认组落在测量区间内的快照均读到 2850 MHz；这只是低频快照佐证，不是逐周期频率测量。

本报告采用以下**明确的架构容量模型参数**：每 XMX 每周期 256 个 FP16/BF16 FLOPs；每 Vector Engine 每周期 32 个 FP32 FLOPs，FMA=2。由此：

- FP16/BF16 XMX：`160 × 256 × 2.85 GHz = 116.736 TFLOPS`；2670 MHz 时为 `109.3632 TFLOPS`。
- IEEE FP32 向量：`160 × 32 × 2.85 GHz = 14.592 TFLOPS`；2670 MHz 时为 `13.6704 TFLOPS`。
- 有效利用比例：`本轮有效吞吐 / 相同精度的建模峰值 × 100%`。

每周期吞吐参数在本轮**没有用独立 ISA 峰值微内核和工作负载计数器交叉验收**，因此表中按此模型计算的百分比须与原始 TFLOPS 分开理解。它们不是 Intel 针对此次执行的性能保证。不能把 233 INT8 TOPS、FP16 向量峰值和 FP16 XMX 峰值混作同一分母，也不能把不同功能单元峰值简单相加。

环境：官方 `torch 2.13.0+xpu` wheel 内原生 C++ 库；oneDNN 3.12.0；Intel compute runtime 26.31.39395.13；NixOS Linux 7.2.3。运行库/依赖留在 `test-artifacts/libtorch-xpu-isolated/` 与本实验目录，未安装进生产运行时。C++ 显式 IEEE FP32，禁止 SDPA math 回退；选择标记为 `sdpa_choice=4` 不单独证明所有形状都用了同一种融合 kernel。

### 3. 相对 15T 挖掘了多少

与历史实际模型注意力 `15.0498 TFLOPS` 作**容量对照**：

| 口径 | 吞吐比 | 吞吐相对增加 | 2850 MHz 模型峰值比例 |
|---|---:|---:|---:|
| 历史 GGML 实际模型 | 1.0000× | 基准 | 12.89% |
| 原生 FP16 时间 SDPA | 2.7601× | +176.01% | 35.58% |
| 含 Q 类型转换和输出恢复的桥接 SDPA | 2.2338× | +123.38% | 28.80% |

原生注意力的容量比例比历史高 **22.69 个百分点**。但历史 GGML 的 F32 Q / F16 K/V / F32 输出与原生同类型 FP16 SDPA 不是同精度计算，历史与本次计时边界也不完全相同；所以这些不是已经实现的整模型/整应用加速比。

相对本轮确认 FP16 GEMM 的经验上限 `84.1708 TFLOPS`，原生注意力达到 **49.35%**，含桥接时达到 **39.94%**。大方阵 GEMM 不是此注意力形状可保证达到的上限：softmax、归约、数据移动、同步和 tile 边缘都占时间，因此不能把剩余差额直接承诺为下一步可获得的加速。

布局对照没有显示 contiguous 的收益：两种均值差仅约 0.27%，小于组内波动；返回 interleaved 的时间漂移仅 0.137%。桥接增加约 **3.0997 ms / +23.56%**，使有效吞吐减少约 **19.07%**。因此融合转换、保持布局、避免反复转换，比盲目增加一次 contiguous 更值得优先验证。

### 4. 已保存的实际遥测，不冒充执行单元计数器

从 `CAPACITY_PHASE` 的开始/结束时间过滤 nvtop 快照，未把上传/验证阶段混入。快照的 GPU busy 本身可能覆盖前一个采样窗口；短区间内样本少、有边界滞后，以下只作运行环境证据，不作精确分单元占用推断。

| 工作负载 | 区间内快照数 | GPU busy 均值（范围） | 频率 MHz | 温度均值 °C | 功率均值 W |
|---|---:|---:|---:|---:|---:|
| 时间 SDPA，首轮 interleaved | 3 | 97.67%（95–100） | 2850 | 58.00 | 183.33 |
| 时间 SDPA，contiguous | 3 | 99.00%（98–100） | 2850 | 58.33 | 181.33 |
| 时间 SDPA，桥接 | 4 | 96.00%（93–98） | 2850 | 58.50 | 178.00 |
| FP16 GEMM 确认 | 6 | 86.50%（40–97） | 2850 | 58.33 | 174.00 |
| BF16 GEMM 确认 | 6 | 98.17%（95–100） | 2850 | 60.50 | 193.50 |
| IEEE FP32 GEMM 扫描 | 23 | 97.43%（92–100） | 2850 | 63.43 | 174.22 |
| 设备内复制确认 | 3 | 72.33%（23–97） | 2850 | 53.67 | 93.67 |

确认组采样中未发现另一组独立 compute 基准同时占卡，但 Hyprland 等桌面客户端仍有活动，部分快照桌面进程最高 15%。不能宣称 GPU 完全独占。GPU busy 接近 100% 并不意味着 XMX 或所有 ALU 已经接近 100%。所有这些区间内 encode/decode 快照为 0%，也不能据此声称逐个媒体单元的精确占用已测完。

### 5. Xe2 分单元硬件计数器：具体做到哪里

私有编译 Intel Metrics Discovery（提交 `b798d05c3c535d3840eccbba23670584c26dd1a9`）和 Metrics Library 后，完整 metric group 枚举成功。最初缺库/权限时的错误、补齐依赖过程与成功枚举均保留，没有把失败解释为硬件不存在。

- 普通用户加载完整 Discovery 后仍返回 `0x7ffffffe`，没有 metric groups。
- 使用机器原已具备的 `sudo -n` 权限进行只读采集、但尚缺 `libigdml.so.1` 时，只出现 `EuStallSampling`；strace 明确给出该库 ENOENT。
- 在隔离目录补齐 `libigdml.so.1` 后出现 `ComputeBasic`、`VectorEngineProfile`、`VectorEngineStalls`、`MemoryProfile`、`DeviceCacheProfile` 等组，stream open 成功。`xe.observation_paranoid` 保持 1，`perf_event_paranoid` 保持 2。

| 需要回答的单元/资源 | 本机确实枚举到的硬件指标 | 本次工作负载有效实测值 |
|---|---|---|
| XMX FP16/BF16 | `XVE_INST_EXECUTED_XMX_FP16`、`...XMX_BF16`，单位为执行 slots/events | **N/A，数据未持久保留** |
| ALU0 / ALU1 / ALU2 | `XVE_INST_EXECUTED_ALU0_ALL_UTILIZATION`、ALU1、ALU2，驱动给出 percent | **N/A** |
| 扩展数学管线 | `XVE_INST_EXECUTED_MATH`，单位 events | **N/A；不能用指令占比冒充时间利用率** |
| SEND / 控制 JEU | `XVE_INST_EXECUTED_SEND_ALL`、`...CONTROL_ALL`，单位 events | **N/A** |
| XVE 活跃/停顿/线程占用 | `XVE_ACTIVE`、`XVE_STALL`、`XVE_THREADS_OCCUPANCY_ALL` | **N/A** |
| 同步、记分牌、取指等停顿 | `XVE_STALL_BARRIER`、SBID、INSTFETCH、SENDWR、ALUWR 等 | **N/A** |
| SLM | `SLM_BYTE_READ/WRITE`、`SLM_BANK_CONFLICT_COUNT` | **N/A** |
| Load/Store cache 与 L3 | cache hit/access、L3 hit/miss/stall/read/write | **N/A** |
| 显存实际总线流量 | `GPU_MEMORY_BYTE_READ/WRITE` 及 RATE | **N/A；复制吞吐不能替代此值** |
| Compute / Copy / Render 引擎 | `COMMAND_PARSER_*_ENGINE_BUSY` | **N/A；nvtop GPU busy 是另一层级** |

本机 Xe2 的 ALU2 命名不能未经验证就改写为“XMX 利用率”。多个 pipe 可并发，多种 stall 原因可同周期成立；相关百分比不能相加成一个必须等于 100% 的饼图。RT、光栅、纹理、媒体不是注意力 kernel 可任意借用的矩阵计算资源，也不是本次算子验收对象。

新采样器 `sample.cpp` 会保留 driver raw report、decoded 数值、metric 名称/单位/描述，并通过 Level Zero 的 metric timestamp resolution 与全局时间戳 API 建立时间映射。实际 smoke 读取到 timer 19.2 MHz、56 timestamp bits，349 个报告且 dropped-data warning=0；但是那一次工作负载因目录冲突没有启动，所以这些是空闲/桌面数据，不能填进上表。

修正观察器子目录后，`profile-_ilxa8tr/ComputeBasic-time/workload/` 保存了成功完成的注意力输出（39.6306 TFLOPS，属于带采样扰动的运行，不替代主结果）。随后开始 GEMM 时段附近发生连接中断和主机重启。重启后该轮 `metrics.ndjson`、`raw.bin`、collector 结果与 series 文件均为空；GEMM 无完成记录。**不能恢复出不存在的单元百分比，也不能把空文件解释为利用率为零。**

该轮新采样器只做流刷新，没有为所有新证据实现逐批 fsync 与目录持久化；这不足以保证突发重启后数据保留。正式继续前需改为耐重启的追加写入/持久化、分别验证校准及 ROI 覆盖、校验丢报告/不确定性，再逐组、逐负载执行。只有只读 collector 需要已有特权，benchmark 应继续以普通用户运行。

### 6. 扫描、数值与负例

GEMM 扫描均值如下；这些是扫描样本，不等同全部完成了稳定性确认。

| 方阵边长 | FP16 TFLOPS | BF16 TFLOPS | IEEE FP32 TFLOPS |
|---:|---:|---:|---:|
| 1024 | 58.9436 | 46.0148 | 13.3537 |
| 2048 | 77.3171 | 39.3214 | 13.0417 |
| 4096 | 92.2643 | 57.4041 | 13.2195 |
| 8192 | 86.1568 | 87.9194 | 13.5907 |

FP16 4096 短扫描出现 92.2643 TFLOPS，但测量区间只有约 0.149 秒，没有落入区间的 nvtop 快照，也没有单独长确认，故不把它取代 84.1708 TFLOPS 的确认值。BF16 小/中尺寸扫描存在桌面活动和较大波动，不应据此断言硬件 BF16 算力只有 FP16 的一半。

早期 `attention-uthippon` 的时间/频率和布局扫描存在 Rain UI 及另一组 `torch-probe` / `ggml-probe` 同时占卡，部分 Rain UI 快照 GPU 使用达 71–84%；相应注意力性能**不作为硬件上限或布局优劣证据**。干净条件下的频率轴确认与形状匹配 QK/PV 分解尚未完成，不借用其他实验的数字填空。

完整 F32 时间注意力尝试分配 7.96 GiB 临时空间时 OOM；保留为失败，不减小形状后冒充完整案例。F32 Q / F16 K/V 混合类型路径失败，未使用静默 CPU 回退补齐。接口桥接每次将 Q 转到 FP16，执行 SDPA，并恢复输出布局和 F32 输出，因此只是在接口形状上接近 GGML，不是同精度无损等价。

内存/非矩阵扫描：copy 370.8782 GB/s（短扫描，确认值为 336.8884）；vector addcmul 556.1240 GB/s 算法字节吞吐；exp 369.3567 GB/s 算法字节吞吐、约 46.1696 G elements/s；1722 列 softmax 的 FP16/F32 时间分别为 3.8034/6.6178 ms。vector 的两个输入为常量张量，缓存/压缩可使算法字节吞吐与物理总线流量不同；超过 456 GB/s 不代表物理带宽利用率超过 100%。exp 也未建立特殊函数单元峰值分母，所以不报告“数学单元利用率”。

主确认组均通过 finite 与数值检查。GEMM/SDPA 使用 128 个确定性抽样输出位置，每个位置执行完整 FP64 参考 contraction；不是完整输出逐元素误差验收。时间 FP16 SDPA 原始 F32 输入参考 NMSE `1.16258715154e-7`，已舍入输入参考 NMSE `8.90498119063e-8`，抽样最大绝对误差 `6.8200570184e-6`。FP16 GEMM 原始输入参考 NMSE `8.99281729764e-8`；BF16 GEMM 为 `7.35881356118e-6`；IEEE FP32 大 GEMM 为 `2.14169991018e-12`。没有执行整模型、音频质量或生产运行时替换验收。

### 7. 后续执行顺序与交付物

**当前不要直接重跑带计数器的压力组。** 先保全仓库/有效证据，核查重启及空 Git 对象，再补齐耐重启日志。随后先短时单负载验证，再增长重复数；禁止多套 GPU 基准同时运行。先采 ComputeBasic，再采 VectorEngineProfile 和 VectorEngineStalls，必要时补 Memory/Cache；每组分别保存数据，不能将不同执行时段的数据冒充同时采集。

采样汇总应按真实 `GpuTime` 加权百分比，按累计 bytes / 区间求带宽，以 group 的单位/归一化公式或 maximum metric values 解释执行 slots；记录 timer 校准误差、ROI 覆盖、ResultUncertainty、丢报告和桌面污染。严禁简单平均不同长度报告、用 XMX 指令数占全部指令数替代 XMX 峰值占用，或把 N/A 置零。

性能方向只列待验证假设：优先消除桥接转换/布局恢复成本；分别测完整键长的 QK 与 PV 及 softmax，以确认 tile 利用、矩阵/向量重叠、访存/同步瓶颈；最后才移植最有证据的优化到 GGML 并做同精度、同计时边界的 ABBA 与整模型验收。当前数据足以证明 15T 不是 B580 原生 FP16 注意力算子的能力上限，但不足以断言是哪一个 Xe2 单元卡住了余下性能。

源码：`tools/xpu-capacity-study/{benchmark.cpp,common.hpp,build.sh,run.py,metrics.cpp,sample.cpp,profile.py}`。计时与采样相互独立。以下是执行过的命令形式存档，不是当前重跑指令：

```sh
python3 tools/record-operation.py LABEL --input tools/xpu-capacity-study/benchmark.cpp -- \
  bash dev.sh -c bash tools/xpu-capacity-study/build.sh
python3 tools/record-operation.py LABEL -- \
  bash dev.sh -c python3 tools/xpu-capacity-study/run.py confirm
python3 tools/record-operation.py LABEL -- \
  bash dev.sh -c python3 tools/xpu-capacity-study/profile.py \
    --groups ComputeBasic VectorEngineProfile --cases time gemm-half
```

关键证据索引（全部相对仓库根目录）：

| 内容 | 路径 |
|---|---|
| 主确认 7 项及原始时间样本 | `test-artifacts/xpu-capacity-study/confirm-ylay9t44/series.json` |
| GEMM 扫描 | `test-artifacts/xpu-capacity-study/gemm-2kkbmf_9/series.json` |
| 受竞争影响的注意力及负例 | `test-artifacts/xpu-capacity-study/attention-uthippon/series.json` |
| 内存与 softmax 扫描 | `test-artifacts/xpu-capacity-study/memory-atxblkk3/series.json` |
| 完整 metric groups / 定义 | `test-artifacts/operations/20260910T081531-61ec6d612228/stdout.txt` |
| 采样器实现与编译记录 | `test-artifacts/operations/20260910T081952-93c4d3a30979/` |
| 空闲 smoke 报告，不属于工作负载 | `test-artifacts/xpu-capacity-study/profile-ciurw32g/` |
| 重启打断且计数器文件为空的运行 | `test-artifacts/xpu-capacity-study/profile-_ilxa8tr/` |
| 重启后只读取证 / Git 错误 | `test-artifacts/operations/20260910T082546-73be6157abfa/` |
| 已保存结果和 ROI 遥测计算摘要 | `test-artifacts/operations/20260910T082800-f7d2e9571bfe/stdout.txt` |
| 本报告编辑意图 | `test-artifacts/operations/20260910T083124-803e31f8eff1/` |

原生基准已有提交 `a3c44b5`、`d28d279`、`c17b2a0`。采样器新增提交 `19db16a` 在重启前返回成功，编译、Python 语法检查、build.sh shell 语法检查也返回成功。之后目录修正的提交/执行调用连接中断，当前 HEAD 对象为空，因此不宣称当前仓库提交完整性已验收，也不把未提交的报告说成已提交。

### 8. 外部依据与局限

外部文档用于核对 API、硬件拓扑及标称规格，不替代上述本机原始数据。

1. Intel Arc B580 官方规格：`https://www.intel.com/content/www/us/en/products/sku/241598/intel-arc-b580-graphics/specifications.html`。
2. Intel oneAPI GPU Optimization Guide，Xe GPU Architecture（2025.2）：`https://www.intel.com/content/www/us/en/docs/oneapi/optimization-guide-gpu/2025-2/intel-xe-gpu-architecture.html`。
3. Level Zero Metric Group Experimental，时间戳映射及多组数据计算：`https://oneapi-src.github.io/level-zero-spec/level-zero/latest/tools/api/experimental/metric_group.html`。
4. Intel Metrics Library：`https://github.com/intel/metrics-library`；Intel PTI 的 Level Zero metrics collection 与 unitrace 文档：`https://github.com/intel/pti-gpu`。

**最终结论：确认原生注意力约 41.5T，含接口桥接约 33.6T；确认低精度 GEMM 约 84–86T，IEEE FP32 GEMM 约 13.6T。按本报告容量模型分别达到约 35.6%、28.8%、72–74%、93.1%。各执行单元的工作负载计数器利用率仍为 N/A，不能将这些吞吐比例或 GPU busy 冒充那部分尚未完成的数据。**
