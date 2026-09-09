# Rust RoFormer / Intel Arc B580 性能独立审查与修改交接

## CPU 权重布局优化：不创建设备的接续结果（2026-09-07 20:20 JST）

本轮按用户要求继续优化，同时避免重新暴露于已知的 GPU 断电风险：只执行 CPU 权重加载、布局转换与测试。**没有创建 GPU 设备、启动微型上传、执行模型推理或全曲；整机断电根因及 GPU 稳定性仍未解决。** 本节结果覆盖当前目录中的 Leap、Denoise、Dereverb、Harmony 四个 RoFormer，Harmony 使用既有 karaoke GGUF 资产；不恢复已移除的 Inst V2。

源码先按相关模块提交保存：共享 GGUF `73041bf`、WGPU `8bd5b82`、RoFormer `a4cd5ea`。这些提交记录已有工作，不代表新增 GPU 验收。随后 `d24c282` 单独撤下未验证的偶数长度 F16 `queue.write_buffer` 上传分支，沿用之前的 mapped 初始化实现；这是撤回候选，**不是确认它导致断电，也不是证明回退后安全**。事故时源码与二进制继续保留。

`38120af` 的性能变更只影响 CPU 布局准备：F32 K-major 副本由 `OnceLock` 按需生成；F16 消费者从原 [N,K] F32 权重分块转置并按原 RNE 规则打包，避免完整的中间 F32 副本。F32/raw-coopmat 消费者仍取得原布局；首次 F32 准备时间单列为 `f32_layout`。没有修改 GPU dispatch、提交等待、上传完成前临时数据的生命周期、scratch 释放或失败传播，也没有调整精度、chunk、overlap 或产品串行控制。

### 同条件 CPU 测量

对照是独立 Git worktree 的真实旧 eager loader（`aa706e1`，基于测量工具提交 `9658833`，仅修正一条过时的锁文件依赖）；新版是 `38120af`。两者均 Release、关闭 GPU feature、4 个 Rayon 线程，按旧／新／新／旧顺序，每个模型每个版本执行两次。下表是平均值，计时包含加载与全部选定布局准备；`b580` 参数只模拟当前 raw/pooled-coopmat 布局选择，不访问或探测设备。

| 模型 | 旧加载＋布局 | 新加载＋布局 | 耗时变化 | 旧／新 CPU 进程峰值 RSS |
| --- | ---: | ---: | ---: | ---: |
| Denoise F16 | 347.05 ms | 282.13 ms | 减少18.71% | 2185.0／1467.8 MiB |
| Dereverb F16 | 351.47 ms | 287.35 ms | 减少18.24% | 2185.1／1467.7 MiB |
| Harmony F16 | 337.71 ms | 292.95 ms | 减少13.25% | 2184.8／1467.7 MiB |
| Leap F32 | 175.71 ms | 175.29 ms | 波动范围内，无显著提速 | 771.2／668.8 MiB |

三个 F16 模型峰值 RSS 减少约32.8%，Leap 减少约13.3%；这是 CPU 进程常驻内存，不是显存。测量前已完成模型只读数值校验，模型处于操作系统缓存中，没有清缓存。测量期间没有本轮并行构建，但保留正常桌面背景活动。GPU 初始化、所有上传（含 raw-coopmat 上传内部打包）和推理都不在计时内，不能将这些比例外推为整模型加速或“不再黑屏”的证据。Leap 的小时间差不作性能收益。

### 验证与可追溯记录

- 14 项 CPU 单元测试通过，包括全部65536种 half 模式、F32 边界／NaN payload、RNE、非整 tile、单维／空形状及并发 OnceLock 读取。
- 四个真实模型共1330个线性层，750463488个值的 F32 布局和 F16 打包分别逐位一致。每层以独立索引转置及原 half slice 转换作比较；不改变模型文件。校验单独运行，其额外分配不计入性能／RSS。
- GPU feature 的 all-targets 编译检查及 Release worker 构建通过，**没有执行 GPU 测试或 worker**。构建提交 `74a113a` 仅在性能提交之后修正 CPU 测量 JSON 的 mode 显示；不改变布局算法。实际未执行的 worker 与构建信息保存在证据目录。
- 每项实现、构建和实验均先经 [操作记录器](ROFORMER_OPERATION_RECORDING.md) 保存命令、提交、dirty 状态和启动意图；改动先提交后执行。旧／新程序分开保留，对照记录的 Git 提交取各自工作树。保留 unrelated 工作树内容，不把 HEAD 声称为整个 dirty 工作树的完整描述。
- CPU 证据：[`summary.json`](../test-artifacts/roformer-cpu-layouts-20260907/summary.json)、[`b580-cpu-comparison.json`](../test-artifacts/roformer-cpu-layouts-20260907/b580-cpu-comparison.json)、[`gpu-worker-build.json`](../test-artifacts/roformer-cpu-layouts-20260907/gpu-worker-build.json)。每条结果指向独立操作记录，正常完成与执行意图分开保存。

“整模型超越 GGML”仍待验证。后续可继续测量 CPU 前后处理与分配成本；GPU 端到端速度和黑屏根因保持未确认，不用既有微型成功记录替代稳定性证据。

## 最新更新：微型上传用例后仍整机断电（2026-09-07）

用户报告瞬间整机断电、必须拔 AC 恢复，随后指出也可能是微型用例已通过、下一步启动才断电；确切触发步骤未确认，故障时点没有日志。19:28:28 用例数值通过且退出0，随后19:35:17只读检查取得新 boot ID `a4acf780-23bb-453d-9a68-5bf560b86a55`（此前 `5fe2e889-c892-411a-9765-7e49deaf46ea`），实际仍启动同一修补内核。**稳定性再次失败，不能用微型回读成功评为安全。**

最后一个有完整记录的微型用例涉及 Vulkan 设备／普通 pipeline 创建、三个旧 mapped-init 与两个新 queue 写入、五次复制回读及销毁，没有计算 dispatch、FA、coopmat 或模型推理。五个目的 buffer 合计544字节，但采样显存约214.605 MiB；小数据量不代表没有设备／驱动工作。新上传路径与“先排队五个上传再回读”的测试顺序都是差异点；创建、复制同步、map/unmap及退出后的驱动清理也须记录，尚不能认定某个API就是根因。同期另一任务的 `make -j15` 内核编译是已保存的混杂因素。

优化后的 Denoise 只有准备请求，未见可核实的启动／结果记录，不能仅凭缺失排除后续步骤中断。CPU并行解码的数值及真实权重加载结果保留，GPU上传优化未获模型性能／稳定性验证。相关源码与实际测试二进制已保存，该次故障记录时后续GPU执行已停止。随后 CPU 阶段撤下未验证的新队列上传分支，见上节；没有据此认定任何 GPU 路径稳定。

详见 [本次代码路径与故障记录](ROFORMER_B580_UPLOAD_BLACKOUT_2026-09-07.md)；证据目录 `test-artifacts/roformer-upload-blackout-20260907T193517/`。

日期：2026-09-07（Asia/Tokyo）  
审查项目：`/home/bintis/Code/uta-studio`（用户给出的 `/code/uta-studio` 在当前 Friday 主机不存在）  
审查方式：实际工作树、保留的基准日志、实验代码、设备能力快照，以及构建配方固定版本的 GGML 源码交叉核对。

## 最新故障更新：修补内核下仍发生整机断电（2026-09-07）

用户明确报告：**手动运行 GGML 后再次整机断电，仍需拔 AC 才能恢复开机**。随后核对发现，`test-artifacts/roformer-ggml-fullsong-defaults-20260907T190724/diagnostic.log` 已记录实际 GGML 执行：Leap F32、354.88 s／44.1 kHz 双声道输入、B580／Vulkan0、batch=1、serial_pipeline=false、`GGML_VK_DISABLE_ASYNC=<unset>`、chunk881559／overlap2，共38个 chunk。按用户说明将本次执行归属为手动运行；此前“只有准备、模型／输入／进度均未记录”的表述已过时。

日志记录 chunk1 计算成功、下载和后处理完成，chunk2 开始计算；最后一条是19:07:47.130 JST 的 chunk1 后处理结束。`command.json`、请求和启动脚本等旁路文件当前均为0字节，因此精确 argv 仍无法恢复。没有完整运行结果；日志截断不能证明断电恰好由 chunk2 或最后一条日志所对应的调用触发。

19:12:47（Asia/Tokyo）只读检查取得新 boot ID `5fe2e889-c892-411a-9765-7e49deaf46ea`，此前两次 Denoise 为 `2880e489-0ed9-4add-a5fb-e9668af1d3b6`；运行与启动 kernel 仍为修补后的 `kgr3m4y9…-linux-7.2.3/bzImage`。当前未发现 RoFormer runtime／worker 进程。本次内核日志因文件权限不可读，未据此声称日志正常，也不要求复现故障来补日志。

**实际稳定性验证失败：这轮内核／驱动修复没有解决所报告的断电故障，根因仍未确认。** 既有 GGTT、PCODE、probe 和 NEO 源码缺陷的审计／修复事实保留，但不能据此声称消除了本机故障，也不据此次失败自动回滚这些修复。先前两次短 Denoise 的数值与耗时仍有效，首次桌面正常反馈和有限退出观察不覆盖后续断电。故障后未自动重试 GGML 全曲，产品路由未改。

**最新接续授权：** 用户随后明确要求继续优化 RoFormer 系列的 Rust 性能，同时关注稳定性与代码安全边界。本轮从 CPU 实现、准备成本测量和相关测试继续推进，保留既有提交等待、资源生命周期、错误传播及产品串行参数；CPU 结果不能证明 GPU 端到端加速或整机稳定。此前故障后的临时暂停不冻结性能优化工作；本轮未重新启动 GPU，也不把上游问题视为已经确认的根因。

证据：`test-artifacts/roformer-b580-incident-20260907T191247/incident.json` 保留首次只读检查；新增 `evidence-review.json` 纠正其当时对命令／进度未记录的解释，并指向实际日志。

用户已手动 `nrs` 并重启，修补内核／NEO 已核对启用；后续状态见 [切换后验证交接](ROFORMER_B580_REBOOT_HANDOFF_2026-09-07.md)。用户的一次直接 GGML 全曲例外授权已执行并报告断电，不改变产品默认，也不是自动重试授权。

> **当前结论：修补内核已启用，随后手动 GGML 仍整机断电，安全目标失败。驱动审计和修复事实保留；本轮继续已授权的 Rust 性能实现、CPU 测量与安全边界审查，未启动新 GPU 负载。既有 CPU 权重加载平均减少约45%，整模型性能仍未持平 GGML。**
>
> 最初的只读审查没有修改运行时源码、启动 Vulkan/GPU 测试或重新跑全曲，其耗时来自当时保留的日志。后续实施结果与证据边界见紧接的“实施复核”；既有 batch=1、串行、no-async、安全退出和产品路由语义继续保留。

## 重启后验证更新

当前源码定向 Release 构建通过（Cargo 10.75 s）。随后逐项运行两次 Denoise 1.2 s，沿用同一模型／输入、chunk352800／overlap4、batch=1／no_async／serial_pipeline。两次均正常完成，墙钟 **3.565／3.143 s**，加载 **0.542／0.452 s**，chunk **2.275／2.256 s**。两次105840个解码音频值完全一致，对已有 GGML 参考 SNR **61.259 dB**、最大绝对误差0.0002071。进程记录确认 B580 与实际 Mesa ANV 库；枚举时也加载了其他 ICD，不能将库列表当成多设备推理。

用户在首次结束后确认两屏和交互正常并授权继续。第二次结果后45.6 s 的只读检查中 boot ID 未变，测试起点以来可读内核日志无条目；尚无第二次后的独立人类桌面反馈。这只覆盖两次短用例及有限观察窗口，不撤销历史整机断电事实，也不是稳定性保证。仍慢于历史 GGML 2.089 s；跨重启及缓存／主机条件变化，不能单独归因于驱动或某项优化。

分层计时指向后续准备／上传优化：第二次 mask 主机670.820 ms，其中 F16打包54.764 ms、F16上传328.103 ms、F32上传87.971 ms；首次 mask GPU 批次合计72.747 ms。原始逐批次时间及两次汇总在 `measurement-summary.json`。下一步优先审查这些准备／上传成本和未计入chunk的设备初始化成本，保留既有同步和池生命周期；未自动运行更大模型、全曲或完整 FA。产品路由和 READY 状态未改。

证据目录：`test-artifacts/roformer-b580-postboot-20260907T185920/`。两次短用例发生在随后 GGML 断电之前；下方原始结论及停止状态仅保留其历史范围。

## 实施复核：2026-09-07 后续工作树

以下编号章节保留最初只读审查的时间范围。本节记录其后对实际工作树的复核和修复，不能把早先的源码行号、内核选择或耗时当成修改后实测结果。

### 前一次断电记录：用户确认整机断电，随后暂停 GPU 实测

用例49之后，用户明确报告最近的测试造成整机突然断电，并手动重启；还说明 Intel OpenVINO 及完全未修改的内核／驱动也发生同样故障。用户随后确认1200W电源、显卡双8PIN供电，故障后必须拔AC电源线等20秒以上，否则整机拒绝开机。重启后只读检查取得新 boot ID `ffefbd5c-fb17-4908-ab72-8bf8f7fb453a`，此前用例记录的是 `73b4df92-8f9b-44c3-a33c-306819382886`。本轮系统安全结果为**失败**。用例中的正常退出、有限数值和命令执行期间 boot ID 不变，只覆盖各自的采样时间，不能证明退出后的整机安全。具体触发调用尚未确定，不能仅凭最后一个结果文件认定唯一根因；事实记录见 `test-artifacts/roformer-b580-followup-20260907T124128/reported-poweroff-after-recent-tests.json`。

最近两个完整形状 cooperative FA 用例45/49分别耗时 GPU 1159.586 / 1168.890 ms，均慢于现有 FP32 FA 的432.246 ms。静态展开累加器后，编译器报告的 spills/fills 从75/184降至40/142，但没有性能收益，也没有解决用户报告的断电。该候选未被模型 worker 选择；不能用数值通过或减少 spill 将它评为安全。用例47 Dereverb 完成了短片段数值输出，但同样不构成系统稳定性通过。

当时转入源码审计、修复准备和 CPU 验证，审计范围包括 ANV、NEO 共用的 xe/GuC、显存和电源路径，以及本机实际 Nix 内核的自定义补丁。后续重启验证、GGML 例外实验和最新继续优化授权见上方更新。产品三个安全参数仍保留；当前既没有完成“不黑屏”的目标，也没有完成整模型性能持平 GGML 的目标。

### 断电报告前的串行实测（截至用例44，历史执行证据）

用户说明 B580 驱动一个 4K 日常屏幕，AMD 核显驱动另一屏，并明确授权继续测试。证据目录为 `test-artifacts/roformer-b580-followup-20260907T124128/`；本轮 GPU 用例逐项执行，没有并行压测，也没有修改驱动调度参数。后续主机进程检查观察到另一会话在编译、Chrome 占用较多 CPU；没有成功保存当时的进程快照。用例44前保存的`host-load-before44.json`显示此前的编译／Chrome 高负载已消退。下面分别报告进程／调用墙钟与同次提交内的 GPU timestamp，不能把主机调度波动解释为内核时间。

- 编码失败不提交、旧 attention 回归、新 attention 的 seq35 与 seq1722 数值测试通过；普通 WGSL、新 attention、coopmat 三类合成 time/freq 子块均通过三轮数值和池复用检查。
- 首次 coopmat 子块参考失败已定位：本机 `8086:e20b` 的当前 DPAS shader 将 FP16 subnormal **操作数**当作零，而打包读回的位值仍与 CPU RNE 一致。参考按已测设备对齐此行为，保留 `1e-4` 与原 CPU 图误差界；不是放宽容差。实际对未量化 CPU 图最大偏差约 `2.79e-4`。不能将该结论泛化为所有 Intel GPU 的固定行为。
- 同一个原 gemm8 shader 的完整 QKV（M154980/K256/N1536）单次同步调用 **61.181 ms / 1.99 TFLOP/s**；全部 238049280 输出有限，59499 个参考位置最大偏差 `7.61e-7`。上传、参考、回读在调用计时外，无隐含 warmup。

| 单 chunk，同模型／输入／chunk／overlap | GGML 进程墙钟 | Rust 进程墙钟及 chunk 时间 | 数值证据 |
| --- | ---: | ---: | --- |
| Leap，原全曲前 6 s，chunk881559 / overlap2 | 11.319 s（内部处理10.885 s，用例12） | 15.762 s（chunk14.618 s，用例16；后续候选尚未重测此模型） | 解码音频 SNR60.577 dB，最大绝对误差0.0012984 |
| Denoise，新 Leap 输出前1.2 s，chunk352800 / overlap4 | 2.089 s（用例17） | 用例19：5.499 s / chunk3.674 s；用例28：4.541 s / chunk2.674 s；用例38：4.195 s / chunk2.581 s；用例44：**3.672 s / chunk2.154 s** | 用例19解码 SNR61.189 dB；用例28与44解码比较均为61.259 dB、最大绝对误差0.0002071。用例38与44编码前105840个浮点值全部有限，peak0.16708748、RMS0.01793820 |

**当前整模型速度仍未持平 GGML。** Denoise 的384通道 out/FF2 已接入由 wgpu 跟踪资源依赖的 pooled cooperative-matrix 内核，保留既有提交等待。相同12次调用的 GPU 平均时间，out 从用例19的18.457 ms 降至用例28的1.745 ms、用例38的1.301 ms；FF2 从55.356 ms 降至5.830 / 5.405 ms。Mask MLP 五个批次的 GPU 总时间从326.407 ms 降至77.710 / 74.291 ms，主机端首次权重准备仍占较大时间。

权重转置改为32×32分块，FP16打包改用 half 库的批量转换；原权重位值／布局与 RNE 转换语义保留，新增的少量 profile 只统计首次准备。用例38加载总计1.100 s，其中张量读取561.899 ms、转置536.978 ms。Mask 的主机时间915.142 ms 中，首次 F16 打包111.145 ms、F16上传493.893 ms，另有 F32上传86.174 ms；这些是主机墙钟，不能与74.291 ms的 GPU 时间混用。

用例44进一步直接上传已有 FP16 字节，去掉完整`u16→u32`复制，并让 GELU 复用池分配；空／奇数长度上传边界和三轮子块／池复用分别由用例39/41验证。此次 blocks主机时间1.260 s、mask主机时间628.255 ms、加载1.026 s（读取527.187 ms／转置497.483 ms），mask首次F16打包43.520 ms、上传306.011 ms。同期mask GPU总时间87.332 ms，未低于用例38的74.291 ms；端到端进展不能全归因于GPU算子，主机负载与准备路径也在变化。

Pooled GEMM 的 C 缓冲区现在分别记录逻辑长度与按 tile 补齐的物理容量，直接复用完整写入的 C，省去另一份逻辑输出和复制。用例33的非整行／384通道小图连续三轮通过数值和池复用检查；完整 QKV 用例32→35的池容量从1,983,930,368降至1,031,733,248字节，消除 **952,197,120字节（908.086 MiB）** 重复存储。用例35两轮池容量相同，238049280个输出均有限、59499个参考位置最大误差均为`7.61e-7`；GPU批次时间15.948 / 13.359 ms，而调用墙钟为115.374 / 49.473 ms。该差异再次说明调用墙钟不能冒充内核计时。

Leap 用例16的16个 time attention **批次**合计 GPU 时间8.017 s（含 gate、gather/RoPE、FA、scatter），freq合计0.668 s。完整 time FA 独立探针使用同一 `groups90 / seq1722 / heads8 / head64` 输入形状，结果如下：

| FA 候选 | GPU 批次时间 | 调用墙钟 | 截至用例44的选择 |
| --- | ---: | ---: | --- |
| 原 FP32 FA（20） | 462.519 ms | 539.500 ms | 比较起点；编译统计无 spill |
| FP32 Q缓存／展开（23） | 432.246 ms | 505.354 ms | 保留；精度、完整上下文、提交方式不变 |
| F16 K/V存储（29） | 423.989 ms | 495.851 ms | 比Q缓存仅约1.9% GPU收益，主干尚未启用；Q、softmax、累加与输出仍为F32 |
| row-state 改写（36） | 534.942 ms | 776.287 ms | 数值通过但变慢，已撤回 |

以上完整 FA 探针均检查79349760个输出有限性，并以13个完整 query row 的全部 keys 作 f64 参考。F16 K/V 的用例26/31曾在独立 Q 参考处失败；诊断中旧／新 Q 位值一致，K/V打包一致，差异来自共同的三角函数数值路径。用例40保留逐位比较，并在 WGSL 规定的三角函数精度范围内使用传播后的 f64 误差界，通过有限性、布局与参考检查；范围外大角度只提供旧／新位值比较。Cooperative-matrix FA 的小图／seq1722 已通过对应混合精度参考（42/43），但对原 dense F32 仍有额外误差。后续完整形状用例45/49数值通过但 GPU 耗时超过1.15 s，慢于原路径；整模型数值未验证，因此仍未接入 worker。

以上成功用例正常结束，记录的 boot ID 保持`73b4df92-8f9b-44c3-a33c-306819382886`；已知数值失败没有被记为通过。完整全曲、多模型 Rust/GGML 比较和重复稳定性仍未完成；这一阶段 Dereverb/Harmony 仅取得 GGML 参照。后续用例47的 Dereverb 进程墙钟4.037 s，对照 GGML 1.582 s；1.2 s 极低电平输入的解码 SNR23.508 dB，不能外推整曲分离质量。这里不构成“绝不会黑屏”的证明，也没有改变产品路由或 READY 状态。

### 重启后的 CPU 权重加载实测

重启后的新实现将 FP16 解码改为批量 SIMD 转换，并使用 Rayon 并行转置互不重叠的32行输出块；转换保留共享 GGUF reader 的全部65536种半精度位模式语义，包括原有 exponent-31 行为，转置只重排位值。RoFormer worker 的16项 CPU 测试通过，3项 GPU 子块测试保持 ignored。

`examples/profile_weights.rs` 只打开模型并调用相同的权重 loader，不创建设备或执行推理。对照程序仅恢复用例44的共享 FP16 解码和串行32×32转置，其余加载路径相同；两者使用相同依赖版本和 Release 配置、关闭默认 GPU feature。这不是重建整个历史用例44 worker。

| Denoise FP16，同一672张量／228202788个值 | 权重加载 | 其中张量读取 | 其中转置 |
| --- | ---: | ---: | ---: |
| 串行对照，用例50 | 1.084 s | 543.265 ms | 539.896 ms |
| SIMD／并行，用例51 | 0.605 s | 409.815 ms | 193.546 ms |
| SIMD／并行，用例52 | 0.547 s | 388.614 ms | 156.927 ms |
| 串行对照，用例53 | 1.003 s | 467.569 ms | 534.722 ms |

按旧／新／新／旧顺序各执行两次，平均加载1.044→0.576 s，耗时减少44.85%。期间存在 NEO／内核后台 CPU 编译，不是隔离主机或冷缓存测试。四次均正常结束，仅只读访问用户模型；未执行 GPU 上传／推理，不能从该比例推导整模型加速或系统 GPU 稳定性。原始输出及汇总见 `test-artifacts/roformer-b580-followup-20260907T124128/cpu-weight-loading-comparison.json`。

### GGML 实际 attention 精度

四个已测 GGML 模型使用同一 B580／固定二进制，诊断均为`fp16: 1`、`KHR_coopmat`。结合固定 GGML 的选核条件和本机 Mesa 26.2.2 报告的`8×16×16`形状，可推导它们的 attention 都走 **FA_SCALAR**；GGML 的 FA_CM1 要求`16×16×16`，GEMM 可用 cooperative matrix 不能证明 FA 也使用它。该结论来自源码与设备能力交叉核对，日志没有直接打印每次 FA 的 pipeline 名称。

Leap 的 public schema 在`native-inference/roformer/src/graph.cpp:643–647`显式把 K/V 转成 F16 并调用`set_prec(F32)`；Denoise、Dereverb、Harmony 使用816/990行的 legacy 路径，没有该覆盖。`set_prec(F32)`只保证此 scalar shader 的 QK/Sf 累加类型，不能据此称整份 FA 都以 F32 累加：

| scalar FA 内部值 | Leap public schema | Denoise／Dereverb／Harmony legacy |
| --- | --- | --- |
| Q输入／`Q*scale`后的`Qf`工作组缓存 | F32／F16 | F32／F16 |
| K/V输入／shader工作值 | F16／F16 | F32／转为F16 |
| QK累加与`Sf` | F32 | F16 |
| softmax最大值与归一化分母（`Mf/Lf`） | F32 | F32 |
| 概率`Pf`、PV与输出累加`Of` | F16 | F16 |
| 最终输出 | F16归一化后转为F32存储 | F16归一化后转为F32存储 |

类型依据为固定提交的[`vulkan-shaders-gen.cpp`](https://github.com/ggml-org/ggml/blob/8c63e70982c95ceb862e3a1073a2c1beef75d60a/src/ggml-vulkan/vulkan-shaders/vulkan-shaders-gen.cpp#L667)中`FLOAT_TYPE`与`ACC_TYPE`的独立选择，以及[`flash_attn.comp`](https://github.com/ggml-org/ggml/blob/8c63e70982c95ceb862e3a1073a2c1beef75d60a/src/ggml-vulkan/vulkan-shaders/flash_attn.comp#L118)的`Qf`、`Sf`、`Pf`和`Of`定义／写回。Rust 当前 FP32 FA 和仅 K/V F16 的候选都不能宣称已逐步复现这份混合精度路径。

### 三个参数的准确语义

| 参数 | 固定 GGML 实现 | Rust 实现要求 |
| --- | --- | --- |
| `batch_size=1` | 每次一个音频 chunk；不是禁止 GEMM rows 或独立 attention heads 批量计算 | 保留单音频 chunk，完整上下文不变 |
| `vulkan_no_async=true` | 设置 `GGML_VK_DISABLE_ASYNC=1`，图末同步；CLI 明确清除另一个 `GGML_VK_SERIALIZE_SUBMISSIONS` 开关 | 保留已有每次提交后的同步等待；不能将参数理解为每个算子一次等待 |
| `serial_pipeline=true` | 前处理、完整推理、后处理顺序执行，关闭阶段流水重叠 | chunk 与前处理／推理／后处理保持串行 |

依据是仓库 `native-inference/roformer/cli/main.cpp`、`src/roformer_runtime.cpp` 和 [固定 GGML Vulkan 源码](https://github.com/ggml-org/ggml/blob/8c63e70982c95ceb862e3a1073a2c1beef75d60a/src/ggml-vulkan/ggml-vulkan.cpp)。GGML 图内仍会按节点和 FLOP 切分提交。Rust 的 invocation 常量约束单次 dispatch，不能据此声称整份 command buffer 的总工作量也受同一个数值约束。三个参数没有消除驱动或整机故障的能力。

### 已修复的具体缺陷

- 独立 linear/GELU 的输出没有从 `GpuBufferPool` 取得，却不断被放回长期池。Leap 单个 QKV 输出是 `154980 × 1536 × 4 = 952197120` 字节；32 个 time/freq 子块可向无人取用的 bucket 累积约 **28.4 GiB**。这五类 standalone 输出现在在最后使用后释放；普通 WGSL 主干重新使用真正的 pooled 输出。
- 原 coopmat SPIR-V 声明 `VulkanMemoryModel`，设备却没有启用对应 feature。构造现通过锁定 wgpu 29 的原生 cooperative-matrix feature 协商完整依赖，并核对所需 `8×16×16 f16→f32` 形状与 32-lane subgroup。
- 原始 A/C 缓冲区未初始化就导入 wgpu，违反 `create_buffer_from_hal` 的既有要求。现在 A 在清零完成后导入，C 在 GEMM 完整写入并同步后导入；失败分配、descriptor 和部分 pipeline 构造得到清理。
- 设备创建失败后无条件再试普通设备会吞掉 OOM/validation 等错误。现在只在创建设备前能力不足时选择普通 WGSL；真实 GPU 错误向上返回。
- 编码出错时，`run_encoded` 不再提交部分操作。普通 `cargo test` 中意外创建 GPU 的测试均改为显式执行，防止默认并行跑全尺寸 GPU 测试。

这些是代码中可以直接验证的问题，不是对无日志黑屏的唯一根因认定。上述应用修改阶段没有修改驱动、系统调度参数、模型、chunk、overlap 或产品模型路由；后续经用户授权的系统 flake 驱动修复见下文。

### 性能实现与验证边界

普通 WGSL 的四个主干线性层、GELU 和相邻运算复用每子块的 command buffer 与缓冲池。新增的 RoFormer 专用 subgroup attention 候选保持完整非因果上下文与 FP32 运算，借鉴固定 GGML 的 `Br=4 / Bc=32 / D_split=8` 划分，用在线 softmax 去掉二次大小的 scores/probabilities；不支持的设备或 head width 继续使用原有内核。其他模型的普通设备构造不启用该候选。

`UTA_STUDIO_WGPU_TIMING=1` 可记录普通 WGSL 操作／batch 的 GPU timestamp；query resolve 放在既有提交里，回读不新增推理提交。`cpu_encode` 与 `finish` 的主机墙钟计时分开。raw coopmat 尚没有独立 GPU timestamp，不能用它的主机调用时间充当内核时间。

`bench_wgsl_linear` 现在显式选择形状、内核、布局和次数，默认只做一次小形状，参考会按实际操作数精度舍入并检查有限性及尾块。它不再用旧 `[N,K]` 布局代表当前 `[K,N]` 生产调用，也没有隐含重复压测。

历史 ggml 对照更正为 **368.481 s**：同 Leap F32 GGUF、354.88 s 输入、38 个 chunk、1722 frames、overlap=2，历史工具输出记录内部 `audio.process.end=368.480929 s`、进程总计 `368.996 s`。原 `/tmp` 文件已消失，工具输出仍在当时会话记录中；这是历史执行证据，**本轮没有重测**。961 s 的早先 Rust F16 运行对应约 2.61 倍差距；126 s 及由它计算的比例不再作为该输入的比较依据。

第一阶段先完成 CPU 参考、WGSL/Naga 验证和定向编译，并准备显式 GPU 测试；下面的初始验证表记录这个阶段。第一次默认库测试意外触发了原先未标记的 GPU 测试，已中断（exit 130），不作为稳定性或性能验收。随后单独授权的 GPU 实测见上面的双屏机器小节，当前**没有获得“持平或超过 GGML”或“绝不会黑屏”的结论**。

已执行的定向验证：

| 验证 | 实际结果 |
| --- | --- |
| `uta-wgpu-runtime --features gpu --lib` | 18 项 CPU 测试通过，5 项 GPU 测试 ignored；最终 subgroup 修订后再次运行其 6 项 CPU 测试通过 |
| `uta-roformer-worker --features gpu --lib` | 7 项 CPU 测试通过，3 项 GPU 子块测试 ignored |
| `uta-gpu-probes --features gpu --bin bench_wgsl_linear` | 4 项 CPU 测试通过 |
| 原 coopmat SPIR-V | `spirv-val --target-env vulkan1.3` 通过 |
| 新 attention WGSL | Naga 解析、完整验证、SPIR-V 1.6 lowering 通过；GPU 数值测试未执行 |
| 定向编译与优化构建 | worker／probe 全 targets 检查及两个 release 二进制构建通过 |
| 仓库检查 | 修改文件格式、diff 空白、产品名称扫描通过；相关 62 个源码文件均不超过 2000 行 |

第一阶段准备的优化二进制为 `target/release/uta-roformer-worker` 和 `target/release/bench_wgsl_linear`，后续候选分别重建后再执行；同一二进制路径不代表同一实现版本。原先待执行的编码失败、非整行 attention 和三个子块／池复用用例已逐项完成，结果见上面的后续实测。本次断电报告后，后续 GPU 测试已暂停。原显式用例的运行方式为逐个 `--exact --ignored --test-threads=1`；它不是对整个 ignored 集合的授权，其中仍有完整 QKV 吞吐实验。重启后的新权重 SIMD 解码／并行转置及混合精度参考通过16项 CPU 测试，3项 GPU 子块测试保持 ignored；新增实现尚未进入模型 GPU 实测结果。

### 驱动源码复核

本机为 Linux 7.2.3、Mesa ANV 26.2.2、Intel Compute Runtime 26.31.39395.13。WGPU/GGML Vulkan 经过 ANV，再进入 `xe`/GuC；Level Zero/OpenCL/SYCL 使用另一条用户态路径，也共用 `xe`。

NEO 的 `ioctl_helper_xe.cpp` 使用 `DRM_XE_VM_CREATE_FLAG_LR_MODE`；本机 Linux v7.2 的 `xe_guc_submit.c` 对该模式设置 `MAX_SCHEDULE_TIMEOUT`。但 [Mesa 26.2.2 的 ANV VM 创建](https://gitlab.freedesktop.org/mesa/mesa/-/blob/mesa-26.2.2/src/intel/vulkan/xe/anv_device.c#L44)没有使用 LR 模式，所以不能直接将该机制认定为当前 WGPU 的故障原因。共用内核、显存迁移、VM_BIND、TLB 和复位路径仍需区分。对运行内核 Nix 派生及实际 vmlinux 的进一步检查发现，自定义 `xe-ggtt-bulk-clear.patch` 将64位 `writeq` 换成 x86 `rep movsl`，每PTE对应两次32位CPU写入，却仍只计一次，破坏BMG每1100次写入的规避措施。清理GPU队列、释放LRC/BO时同样可达这条GGTT路径。

另一份 `xe-exclusive-async-probe.patch` 让异步probe脱离 `driver_detach` 的全局等待，存在注册失败清理／卸载的生命周期缺陷；尚无证据说明本次触发了该条件。Intel NEO官方 `3d7a21dca960c81ead70412d6b8cbacd290a3905` 则修复私有内存复用后的驻留遗漏，只影响Level Zero，不能解释ANV路径。

原版 Linux 6.18.49／7.2.3 的 PCODE 功率 RMW 还有一个与本地补丁无关的缺陷：读取失败后继续写入，初始零值可能请求清除另一功率限制的 enable 位。新补丁让失败立即返回原 errno 并禁止本次写入，同时传播平均窗口写入的错误。两版真实源码在 CPU 模拟中均复现原缺陷，修补后均通过读写错误、成功位值保留及锁／PM 引用释放测试。本机是否曾满足“读失败、后续写成功、固件接受且没有恢复”的条件仍不明；初始化调用层忽略返回值的行为尚未完整重构，不能称作完整初始化修复。

用户授权后，`/home/bintis/Code/xirin-nix/hosts/btspc01h/kernel-local.nix` 从既有输入移除上述两条自定义内核补丁，并让普通6.18.49／优化7.2.3各接入一次 PCODE 修复；`modules/system/gpu.nix` 的两个启动项均接入本地NEO回移。补丁临时实际应用、正常Git flake及整套系统最终派生求值通过。NEO普通／诊断两包已完成CPU编译；既有构建跳过单元测试，因此四个上游单元测试未执行。最终优化7.2.3内核也已编译成功，普通6.18.49内核目前只验证补丁应用和实际函数的CPU编译执行。尚未切换系统或重启。用户已明确确认完全未修改的内核和驱动也会断电，本地bulk补丁不是故障的必要条件，不能将这些修复认定为断电根因的解决。完整源码依据、修复文件与边界见 [驱动修复说明](../native-inference/gpu-probes/driver-fixes/README.md)。

最终新内核的 `vmlinux` 反汇编确认64位 GGTT 写入及1100次刷新、PCODE 读错误直接返回，以及 interval 错误路径释放锁和 PM 引用。独立复核确认优化固件也依赖新内核，没有遗漏的旧内核引用。构建及产物证据汇总为 `test-artifacts/roformer-b580-followup-20260907T124128/system-driver-final-verification.json`；未构建或激活整套系统，也没有新的 GPU 执行。当前安全与性能目标仍未完成。

## 1. 已确认的结果与证据边界

当前 HEAD 为 `7176107`，但运行时包含大量未提交修改；**该提交号不能代表被审查的完整源码**。修改前请保存相关文件 diff/摘要，不能 reset、覆盖或回滚其他工作。

现有实验目录统一记为：

```text
S=/tmp/claude-1000/-home-bintis-Code-uta-studio/c8e6418f-0859-4ddc-a527-8812d3fca14c/scratchpad
```

本次重新读取 `timing-result.txt` 并用 CPU 解析两次运行的 `stderr.txt`，得到：

| 同一 Leap 全曲实验 | 墙钟时间 | time 子块 finish 合计 / 次数 / 平均 | freq 子块 finish 合计 / 次数 / 平均 |
|---|---:|---:|---:|
| `S/roformer-fullsong-merged-v1` | 1386 s，exit=0 | 1020.261 s / 608 / 1678.061 ms | 303.894 s / 608 / 499.825 ms |
| `S/roformer-fullsong-softmax-fix-v1` | 1054 s，exit=0 | 694.121 s / 608 / 1141.647 ms | 296.150 s / 608 / 487.090 ms |

后者请求文件明确使用 `bs_roformer_leap_xe90_vocals`、原有 `bs_leap_xe_voc-F32.gguf`、`device=gpu`、`batch_size=1`、`vulkan_no_async=true`、`serial_pipeline=true`。608 次 / depth 16 对应 38 个 chunk 的每轴子块执行。

历史笔记给出的 GGML 对照约为 126 s，因此历史参考比例是 `1054 / 126 = 8.365`，不是当前仍固定为 10 或 12 倍。**本次没有重新验证 126 s 的二进制、精度、chunk/overlap、计时边界和 GPU 选核；不能把它当作已经重新完成的严格 A/B。**

现有笔记还记录了：批次合并约 1568→1386 s；softmax 并行化后 1386→1054 s；两个朴素 WGSL FlashAttention 原型在短片段约 176/189 s，慢于三内核路径的约 137–141 s。这些是此前实验，不是本次新测量。不要重复实现已修复的 `workgroup_size(1)` softmax，也不要重复原样的 TILE=16 FlashAttention。

## 2. 优先级最高的新发现：coopmat 基准不足以支持“硬件上限”结论

主要证据：`S/probe-coopmat/src/bin/bench_gemm9.rs`；同类问题也存在于 `bench_flash_coopmat5.rs`。这是**独立实验程序的问题**，不能直接说生产 wgpu 路径同样存在这些 Vulkan 用法错误。

### 2.1 命令缓冲区生命周期不合法：必须先修

`bench_gemm9.rs:115` 创建 command pool 时没有 `RESET_COMMAND_BUFFER`；126–136 行录制、提交并等待一次 correctness pass；165–177 行的 timed loop 只重置 fence，然后再次 `begin_command_buffer`，既没有 reset pool，也没有 reset command buffer。

`bench_flash_coopmat5.rs:159,173,208–211` 使用相同模式。

这违反 Vulkan 的 `VUID-vkBeginCommandBuffer-commandBuffer-00050`：未以 `VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT` 创建的 pool，其 command buffer 在 begin 时必须处于 initial 状态。等待 fence **不等于** reset command buffer。[V1]

建议选择一种合法方案，而不是混搭：

- pool 加 `RESET_COMMAND_BUFFER`，等待前次 fence 后显式 reset command buffer，再 begin/end/submit；或者正确 reset 整个 pool。
- 对纯内核稳态测试，预录可重复提交的 command buffer，不使用 `ONE_TIME_SUBMIT`，确保上次完成后再提交，并正确处理每轮 query reset、内存依赖与结果读取。

先用验证层检查一个有界正确性用例，再在同配置、关闭验证层的性能测量中对比。不要再用不合法生命周期程序的 GFLOP/s 宣布某条硬件路线无效。

### 2.2 输入和输出没有要求 device-local 显存

`bench_gemm9.rs:15–35` 选择第一个满足标志的 memory type；73–75 行的 A/B/C 全部传 `host_visible=true`，要求的只是 `HOST_VISIBLE | HOST_COHERENT`，没有要求 `DEVICE_LOCAL`。

`bench_flash_coopmat5.rs:27–35,110–113` 对 Q/K/V/O 也一样。

**目前不能断言实际一定分配到了系统 RAM**：有的设备内存类型同时具备 host-visible 和 device-local。能确认的问题是，实验没有记录选中的 memoryTypeIndex、flags、heapIndex，也没有证明它与 wgpu 的 GPU 工作缓冲区具有相同的显存驻留条件。

修改要求：A/B/C 或 Q/K/V/O 使用 device-local 工作缓冲区；必要时用独立 staging 上传和回读。将分配类型、heap 和字节数打印进实验记录。一次性上传与性能循环分开；结果检查放在计时外，但保留端到端含传输的独立指标。不能把 PCIe/主机内存访问开销解释为 XMX 指令吞吐上限。

### 2.3 测试形状、dispatch 和占用率不能代表真实模型

`bench_gemm9.rs:6–9` 固定 `M=1728,K=256,N=1536`。生产 `engine.rs:395,544–545` 的 QKV 线性层使用整个网格的 `rows`，Leap 的实际语义 M 是 `90×1722=154980`。

当前 `gpu.rs:471–475` 又把它分成每 dispatch 672 行：`floor(floor(1048576/1536)/16)×16=672`，整次 QKV 约 231 个 dispatch。**这些 dispatch 同属批次，不等于 231 次 submit/wait。**

因此需要分别对比：单个真实 dispatch、完整逻辑 QKV 调用、完整子块。`M=1728` 可保留为辅助用例，不能充当整条生产路径的“相同形状证明”。

`gemm9.comp:26–35` 每 subgroup 持有 `8×4=32` 个 accumulator matrix；该版本 host dispatch 只有 `27×3=81` 个 workgroup。大寄存器块带来的占用率、spill 和可调度任务数量必须检查；它的吞吐平台不自动等于芯片上限。

### 2.4 subgroup 假设、正确性和计时也要补齐

`gemm9.comp:14,31` 的 256-thread workgroup 按 8 个 subgroup 编址；host pipeline 未显式要求 subgroup size。设备快照同时给出 min=16、max=32。应请求并验证该 pipeline 所需的 subgroup size，或使用实际 subgroup 数编址，不能仅依赖全局默认值。这里是需要排除的风险，**不是已经证明本机实际运行错用了 subgroup size**。

现有 GEMM 只抽检 7 个元素且允许绝对误差 0.5（152–160 行），不足以验收新布局与尾块；必须检查 `is_finite`、更多边界和非整除维度。NaN 不能被误差统计掩盖。

`Instant` 包围 host 录制/提交/等待，反映外部调用延迟，但不是内核自身 GPU 时间。保留这个指标，同时补 timestamp query。所有候选用同一 harness、相同驻留、精度、warmup、重复次数和 readback 范围，避免两套微基准各自计时。

**处理旧笔记：**给 `project_roformer_hardware_ceiling_finding.md` 的“约 257 GFLOP/s 是硬件上限 / 该方向已排除”结论追加更正。保留原始测量，不删除失败历史，但将该解释标为“基准存在待修问题，不能据此排除”。

## 3. 生产代码真正做了什么

### 3.1 已完成的优化应保留

`roformer-native/src/engine.rs:384–440,453–605` 的普通 RoPE 主干已经保持网格和主要权重在 GPU；每个 time/freq 子块共享一个 batch，在 597 行结束。不存在“每个主干线性层都下载到 CPU”这个当前根因。

保留 `GpuBufferPool` 已修好的生命周期与早释放逻辑。gamma 的 `queue.write_buffer` 上传发生在 batch 编码之前（479–485 行），和 encoder 内的 GPU 写入不同；继续合并整图时必须重新设计上传、资源别名与同步，不能只删 `finish()`。

### 3.2 当前 profile 无法把 GPU 耗时分到具体算子

`engine.rs:552–558` 的 `attention` timer 包住命令编码；597 行 `finish` 才汇总等待该子块的 GPU 工作。日志里 attention 几毫秒，不能说明 GPU attention 很便宜；`time.finish` 也不能当作纯 attention 时间，因为它包括 QKV、gate、out projection、FFN、norm、残差等。

在 1054 s 运行中，两轴 finish 合计约 990.272 s，占墙钟约 94.0%；说明应重点分析这些子块，但仍不能直接区分算力、带宽、barrier、分配、提交等待或驱动成本。

修改 `wgpu-runtime/src/gpu.rs` 的 device feature 协商和 `encode_kernel_pass`，以及 batch 的 query resolve：可选开启 `TIMESTAMP_QUERY`，利用 compute pass 的 beginning/end timestamps，批末统一 resolve/readback。[W1][W2] 不要为每个算子额外 `poll`，否则测量本身会改变执行方式。unsupported 时输出“GPU timing unavailable”，不能用 CPU encode time 冒充。

计时记录至少包含：kernel/axis/layer、M/N/K 或 groups/seq/head_dim、dtype、布局、dispatch 数、submit 数、GPU ns、CPU encode ns、传输字节、pool 新分配/峰值。短期按类别采样即可，避免海量查询扰动结果。

### 3.3 F32 标量 GEMM 仍是通用小 tile 实现

`shaders/linear.wgsl` 使用 16×16 workgroup tile、每线程一个 F32 accumulator，每轮 K tile 两次 workgroup barrier。没有寄存器微块或协作矩阵计算。

RoFormer 的 `linear_pooled` 调用传 `weight_transposed=true`；`linear.wgsl` 的 `col*K+k` 权重寻址使相邻输出列的取数跨 K 步长。实际合并访存效果要看编译器与 lane 映射，但这是可直接做 A/B 的改进点：**模型加载时只做一次 `[N,K]→[K,N]` 预打包，或修改协作加载后在 shared memory 转置**。不要在每次推理中反复 CPU 转置。

`shaders/linear_f16.wgsl` 只是读取 packed half 权重、`unpack2x16float` 后做 F32 标量循环；不是 XMX，不是混合精度 tiled GEMM，也不是当前主干的等效高性能替代。不要直接切过去期待自动提速。

### 3.4 注意力仍物化完整的分数与概率矩阵

`gpu/attention.rs:710–843` 的 resident 路径执行 gather/RoPE → scores → softmax → context → scatter/gate，按 group 分块；它不是在线 softmax 的 FlashAttention。

Leap 的 `bands=90, heads=8, T=1722, head_dim=64` 给出以下算术量：

| 量 | 结果 |
|---|---:|
| 全部 time attention 的 score 元素数 | 2,135,004,480 |
| 一份 F32 score 的逻辑总量 | 8.540 GB / 7.954 GiB |
| score + probability 的逻辑总量 | 15.907 GiB |
| 完整 F32 QKV 输出 | 908.086 MiB |
| 单个 time 子块的 QK 与 PV 总 FLOPs | 546.561 GFLOP |
| 单个 freq 子块的 QK 与 PV 总 FLOPs | 28.566 GFLOP |

这些是整个逻辑调用的总量；实现已分块，**不表示一次申请了 15.9 GiB**。分块预算仍应按同时存活的 scores、probabilities、Q/K/V、残差、FFN 和 pool 缓存总和计算，而不是只比较单个 buffer 与 binding 上限。

按 B580 官方 456 GB/s 峰值，仅四次完整 score 大小的逻辑读写就对应约 74.9 ms 的理想带宽算术量，而不是此前笔记写的“多次读写只有 13–17 ms”。[I1] 这不是实测 DRAM 时间：缓存、重复扫描、有效带宽与调度都会改变结果。它既不能证明带宽解释了全部差距，也不能用来排除带宽问题。

### 3.5 不同模型系列不能直接共用 Leap 结论

`engine.rs:392–394` 遇到 PoPE 会退出这条 resident 主干路径。PolarFormer/PoPE 的其他路径应独立标记 CPU/GPU 算子归属、Q/K 扩维、位置相位和精度，不要只优化 Leap 后宣布整个系列 GPU 化完成。

`docs/KEY_CONCLUSIONS.md` 还记录了 PolarFormer 全 FP16 出现非有限 mask 的历史。RMSNorm 的 `epsilon=1e-12` 也不能转成 half 后继续指望同样的数值行为。保留 FP32 reduction、norm、softmax 状态、必要的 residual/输出；混合精度和严格 F32 必须作为不同实验条件验收。

## 4. GGML 的真实实现提供了哪些可用对照

构建脚本 `native-inference/ggml-worker/build-ggml-runtime.sh:5` 固定：

```text
ggml commit: 8c63e70982c95ceb862e3a1073a2c1beef75d60a
```

CMakeCache 中原来的临时 checkout 已不存在。本次只下载上述 commit 的 `src/ggml-vulkan/ggml-vulkan.cpp` 作为研究副本：

```text
/tmp/uta-b580-review.bwddLF/ggml-vulkan.cpp
sha256 c73e8f7980cd416f7fd30860c43f85e4ecacb7db84c0bf057cbb505aaf90142f
```

以下行号指这个固定版本，不是随时间变化的 master；仍需核对历史 126 s 二进制是否确实匹配此配方与环境。

**第一，单次 graph_compute 不是一次 Vulkan dispatch，也不能推导为一次 submit。** `ggml_backend_vk_graph_compute` 在 17027–17073 行按 FLOPs、节点数量等组织提交；3159–3233 行负责提交一组 command buffers。之前“GGML 一图一提交，所以 launch tax 解释全部差距”的推断不成立。

**第二，GGML 对 GEMM 和 FA 分别选核。** 4168 行的 Intel 最小 subgroup size 选择属于 `mul_mat_id` 路径；普通 cooperative-matrix GEMM 仍按报告的 subgroup size 专门化，不能泛化为全部 matmul。4295 行以后有 Xe2/Xe3 coopmat warptile 调优；4734 行构建包含 F32 输入类型的 matmul 变体。不能只看 GGUF 是 F32 就推断底层每条乘法或存储类型；应记录真正 pipeline、specialization constants 与 shader 内部精度。

**第三，B580 不一定通过 coopmat1 执行 FlashAttention。** `get_fa_tuning_params` 的 3858–3865 行要求 FA coopmat1 有 16×16×16 的目标类型形状，不满足则选 scalar。此前设备实验记录的 8×16×16 GEMM 形状，不能自动满足这个 FA 条件。设备快照暴露 `VK_NV_cooperative_matrix2` 扩展也不代表完整所需 features、flexible dimensions 和编译支持都满足。实际 FA 路径必须记录，不能猜。

后续复核已将本轮四个 GGML 模型与固定源码、实际 B580 设备能力对应，结论为 FA_SCALAR；完整依据与精度区别见上面的“GGML 实际 attention 精度”。这更正了最初审查时尚未完成的选核判断。

**第四，有可以直接复现的 Intel scalar-FA 对照参数。** 3708–3780 行明确给 Intel 设置 `disable_subgroups=true`，注释说明强制 subgroup size 曾有性能问题。对 head_dim=64 且多 query rows，代码推导的初始参数是 workgroup=128、Br=4、Bc=32、row_split=1、d_split=8、shmem_staging=0。它和之前“一块 16×16，逐 K tile 多次 barrier”的原型不是同一种工作划分。

因此第一版 FA 对照应**复现这条实际参数化的成熟实现或移植它的工作划分**，再测试新方案。不要断言“增大 K tile/加 subgroup 就必定更快”；先与这个 Intel 对照比较。[F1] 的价值是减少非 matmul 工作与跨线程通信的原则，其 A100 数字不能搬到 B580。

**第五，图级精度参数不能代表所有内部存储和累加类型。** 本项目 `roformer/src/graph.cpp:643–647` 的 public-schema 路径显式把 K/V 转 F16，并将 FA precision 设为 F32；其他 schema 的 FA 在 816、990 行。本轮选中的 scalar shader 内，Leap 的 QK/Sf 为 F32，另外三个 legacy 模型为 F16；但四者的缩放后 Q缓存、K/V工作值、概率 P、PV 和输出累加均为 F16，`Mf/Lf`保留 F32。应逐项核对这些类型；不能将 Leap 的`set_prec(F32)`表述为全程 F32 累加，也不能将全部 activation 改 half。

## 5. 让 Rust 路径有机会超过 GGML 的实施路线

### P0：先让优化决策基于有效证据

修复第 2 节基准生命周期与驻留；把 phase timer 明确改名为 CPU encode timer；增加可选 GPU timestamp。为同一输入保存 GGML 和 Rust 的模型摘要、构建信息、GPU/driver、实际形状、chunk 数、overlap、padding、dtype/精度、kernel 路径、墙钟边界。

首先得到一张同条件表：QKV、out、FF1、FF2、time FA、freq FA、norm/gate/residual、layout copy。不能用整个子块的 FLOPs 除整个 finish 时间，作为某个 GEMM 的吞吐。

验收：验证层的有界正确性检查通过；分配与形状可审计；同一候选的 GPU 时间、调用墙钟和整体墙钟分别报告；没有 benchmark 专用偷减工作量。

### P1：GEMM 两条候选路线，不再从“单一小 tile”盲调

**保留严格 F32 的低侵入路线：**先一次性预打包权重或修正 shared-memory 加载布局；再用 128/256 个线程的 register microtile 计算多个输出，独立选择 BM/BN/BK，测试例如 32×64、64×64 的输出块，而不是把 workgroup 直接扩大到 32×32。这些是待测候选，不是推荐固定最优值。小 N 的 gate/band 投影要有独立策略。

**B580 专用混合精度路线：**以 GGML 真实 matmul 变体、subgroup 和 tile 策略为性能参考，使用设备实际支持的 cooperative-matrix 形状与 FP32 accumulation，先验证 single-op，再在完整子块对比。修复旧 harness 后才允许解释 XMX 的实测结果。

当前 `wgpu-runtime/Cargo.toml` 锁定 wgpu 29.0.4，`gpu.rs:192` 的 `required_features` 为空。硬件支持 shaderFloat16/cooperativeMatrix 不等于当前 WGSL 路径已经获得这些能力；应检查这个锁定版本的 Naga/WGSL/hal 能力，不能照抄其他版本的实验 feature 名。

若所需的 F16 cooperative-matrix 操作无法通过当前 WGSL 合法表达，可将**Rust 主机端 + 预编译 GLSL/SPIR-V 的受限 Vulkan 实验后端**作为备选。Rust 重写不要求 GPU shader 也必须是 Rust，但该方案不能悄悄替换已有生产 wgpu 路由。需要清晰的设备/队列所有权、buffer 生命周期、barrier 与错误处理；不得把另一个 VkDevice 的 buffer handle 当成本设备可用资源，也不得绕过 wgpu 跟踪直接写共享 buffer。

晋级条件：真实形状上的 kernel 与完整子块均有收益，且数值验收通过；全曲尚未验证前只保留实验状态。单靠 packed-half 存储或 feature 开关不能晋级。

### P2：分别为长 time attention 和短 freq attention 选核

保留现有三内核实现作为正确性和性能对照。第一候选复现第 4 节的 GGML Intel scalar-FA 划分及相同精度，测 `seq=1722` 和 `seq=90`，不要只有 `1728` 的整除用例。

第二候选才是 B580 的 8×16×16 cooperative-matrix 专用在线 softmax。这里存在一个具体研究机会：GGML 的 coopmat1 FA 形状门槛可能未覆盖该设备形状；专用实现若保持足够 query/head/group 并行度、控制寄存器压力并减少中间矩阵，可能优于其 scalar 路径。**这是带有代码依据的待证假设，不是已经获得的提速。**

长序列按 query tile、head、group 并行，在线 softmax 的 max/sum/output accumulator 保持 FP32；分段 K 的状态合并必须使用正确的 rescaling。只在并行度不足时测试 split-K，不给已经足够多的 90×8 个 head/group 无条件增加归并开销。逐候选检查寄存器和 shared-memory预算。

短频率轴单独对照 compact noncausal attention、GGML scalar-FA 和原三内核；它可能不适合长序列的同一配置。禁用 causal 假设；正确处理 90、1722、尾块、RoPE 位置、padding、非有限值，以及 PoPE 的不同 Q/K 维度。

不能为了提速缩短 chunk、减少 overlap、去掉频率轴、跳层或改成局部 attention；那些会改变算法与输出，不能算同一任务胜出。

### P3：追平算子之后，做 GGML 通用图之外的模型专用融合

RoFormer 固定模型和固定 chunk 形状提供以下可验证机会：

1. **融合 epilogue：**FF1+bias+原有 erf-GELU；FF2+bias+residual；out projection+residual。保持现有 GELU 公式与精度，不擅自换 tanh 近似。先测单个融合对，而不是一次重写整块。
2. **融合布局处理：**让 QKV 投影直接产生 attention 所需布局，在合法的成对通道处理下融合 RoPE；将 context+gate+scatter 与后续投影布局相协调。目标是减少约 908 MiB 的大 QKV 临时张量及重复 gather/scatter，而不是增加新的全尺寸转置。attention 输出投影融合受跨 head 依赖限制，不应想当然塞进单 head kernel。
3. **缓存静态计划：**缓存 shape/precision 对应的 pipeline、bind group、uniform 模板和模型常量；按 liveness 规划 arena 与总峰值。当前 wgpu command buffer 不能被当作可任意重复提交的 CUDA graph；复用的是计划和资源，或在合法原生路径中管理可重录/可重复执行的命令。
4. **边缘流水线：**再把 band split 与 mask estimator 的散碎传输合并；`engine.rs:837–913` 的 mask batch 后逐 band download 可以改为一次 staging 中的多段 copy/统一等待。其优先级由新 profile 决定，不以它作为剩余约 8 倍差距的默认解释。

保持有界串行提交，不强求“一整首/一整图一个 command buffer”；参照 GGML 的工作量预算和本项目硬件安全历史。不得全局增大共享 runtime 的 dispatch 上限来让其他已验证模型承担风险。

### P4：以模型系列和最终输出验收，不只看一个矩阵

数值测试覆盖随机、零/近零、较大幅度、真实中间张量、奇数和尾块、所有支持的 head/seq、RoPE 与 PoPE。输入量化误差与计算误差分开；FP16 候选用相同量化后的输入作 CPU 高精度参考，同时比较对原模型输出的总体影响。

逐层记录绝对误差、相对误差、均方误差、finite 检查；音频端比较解码 PCM、能量/误差和 chunk 拼接边界，不以两次 GPU 运行 byte-identical 代替与参考实现的数学对齐。阈值沿用项目已有要求，新增阈值应解释依据，不能随意以 0.5 的 spot check 放行。

性能按 microkernel → 完整子块 → 同配置片段 → 原全曲递进，GPU 测试只在既有明确授权和安全约束内串行进行。报告 cold-start 与 warm steady-state、GPU kernel 时间与包含 I/O 的墙钟、峰值显存及设备错误；共享 runtime 改动还要跑对应其他模型的有界回归。

如果剩余未优化部分占比为 p，那么把其余部分无限加速也只能达到 `1/p` 倍；因此单靠注意力或单靠 submit 合并不能预先承诺超过 GGML。最终胜出条件是**同模型、同工作量、相当输出质量、同硬件条件下稳定低于 GGML 墙钟时间**，而非局部 TFLOP/s 好看。

## 6. 给 Claude 的直接修改任务

请先阅读本报告并核对你的最新工作树，避免覆盖正在进行的实验。

- **第一批修改：**修正 `probe-coopmat` 的 command-pool/reset 与 memory-type/staging；给实验添加有限性、尾块和实际形状检查；更正“XMX 硬件上限”旧结论。把可复现的 harness 放入项目约定的开发测试位置，不仅留在 `/tmp`。
- **第二批修改：**在 wgpu runtime 增加可选 GPU timestamps 与按类别统计，修正 `attention`/`finish` 计时名称和文档；不改变生产精度、安全 profile 或其他模型默认行为。
- **第三批修改：**以真实选核的 GGML 为对照，先提交 GEMM 权重布局/寄存器分块和 scalar-FA 对照的独立候选，再根据同条件测量决定 cooperative-matrix 或进一步融合；失败候选留记录，不默认启用。

每批留下“改了什么、源码身份、测试范围、数值差异、GPU/墙钟时间、显存、是否通过”的记录。没有跑到的模型和全曲直接标注未验证，不更改 production-ready 声明。本次审查只添加报告，运行时源码由你按上述顺序修改。

## 7. 证据与外部原始资料

本地证据优先：

- `native-inference/roformer-native/src/engine.rs:384–605,837–913`：resident 主干、计时、PoPE 分支与 mask readback。
- `native-inference/wgpu-runtime/src/gpu.rs:142–194,457–499,1144–1191`：安全约束、feature、dispatch 切分与 pass 编码。
- `native-inference/wgpu-runtime/src/gpu/attention.rs:710–843`；`shaders/linear.wgsl`、`linear_f16.wgsl`、`softmax_rows.wgsl`：实际 GPU 算法。
- `native-inference/roformer/src/graph.cpp:643–647,816,990`：本项目 GGML 图的 FA 与精度。
- `VP_VULKANINFO_Intel(R)_Arc(tm)_B580_Graphics_(BMG_G21)_26_2_2.json:314–316,657–660,811–821,1389,1427–1429,1501–1505`：设备快照。它不是本次重新查询的当前驱动状态。
- `S/roformer-fullsong-{merged-v1,softmax-fix-v1}/{request.ndjson,stderr.txt,timing-result.txt}`：本次重新解析的实验资料。
- `S/probe-coopmat/src/bin/bench_gemm9.rs`、`bench_flash_coopmat5.rs`、`gemm9.comp`：本次发现的实验漏洞。
- Claude 项目记忆中的 `project_roformer_batch_merge_result.md`、`project_roformer_attention_bottleneck_and_flash_attn_gap.md`、`project_roformer_hardware_ceiling_finding.md`：历史解释与失败实验；不能用旧解释覆盖实际代码证据。

外部资料均为原始规范、项目源码、作者论文或厂商规格：

[V1] Vulkan `vkBeginCommandBuffer`，特别是 VUID 00050：  
`https://docs.vulkan.org/refpages/latest/refpages/source/vkBeginCommandBuffer.html`

[V2] Vulkan command buffer 生命周期与 reset：  
`https://docs.vulkan.org/spec/latest/chapters/cmdbuffers.html`

[W1] wgpu 官方 timestamp queries 示例（实现时核对项目锁定的 29.0.4 API，而不是盲用文档最新版本）：  
`https://wgpu.rs/doc/wgpu_examples/timestamp_queries/index.html`

[W2] wgpu ComputePassTimestampWrites：  
`https://docs.rs/wgpu/latest/wgpu/struct.ComputePassTimestampWrites.html`

[I1] Intel Arc B580 官方规格，12 GB GDDR6、456 GB/s：  
`https://www.intel.com/content/www/us/en/products/sku/241598/intel-arc-b580-graphics/specifications.html`

[F1] Tri Dao，FlashAttention-2，工作划分与减少同步/非矩阵乘开销的原始论文摘要；其 GPU 性能数字不作为 B580 预测：  
`https://arxiv.org/abs/2307.08691`

[G1] 固定版本 GGML Vulkan 后端源文件；第 4 节行号据此：  
`https://github.com/ggml-org/ggml/blob/8c63e70982c95ceb862e3a1073a2c1beef75d60a/src/ggml-vulkan/ggml-vulkan.cpp`
