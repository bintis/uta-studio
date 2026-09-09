# B580 XE90：显式throughput单次全曲实验（2026-09-07）

## 授权与参照

用户在[tracked串行路径完成38/38](ROFORMER_B580_PADDED_C_2026-09-07.md)后明确允许：一次忽略batch/no-async/serial-pipeline三个旧参数的性能优化方案，再执行一次同曲GPU全曲。随后明确允许本次**不开诊断**，若再次失败由用户重启后再指示开启。此新授权替代前文“无更多GPU运行授权”，但不是批量探针、压力测试、自动重试或生产默认变更授权。

参照：已完成的Rust tracked串行621.995629秒（逐事件同步诊断开启），受支持GGML同曲402.623654秒（用户反复验证稳定）。Rust观察值约多54.5%；这些非隔离记录有不同日志I/O与背景负载，不是纯kernel性能对照，也不保证本次能超过GGML。

## 实现

- **`9b6fc6d`**：worker专用显式参数 `--experimental-throughput`，默认关闭且不由Studio设置隐式启用。单独解析入口忽略三个旧字段的值，保留已有device/profile/device-class验证；显式coopmat构造器允许实验调度，普通解析和构造器的行为不放宽。
- 有效调度是**一次一个音频chunk、CPU前后处理仍串行、一个time/frequency子块内统一GPU命令批次**。norm、QKV pack/GEMM/bias、gate、attention、out projection、FF/residual在同一个跟踪命令流内编码；去掉逐GEMM阶段和逐attention group的host等待，子块末仍submit/wait并处理全部错误。原分组范围及全序列不裁剪，原F16 operands/F32 accumulation、64×128 shader tile、权重和音频不变。
- 不声称音频batch2或CPU/GPU多stage重叠：前一候选的池约5.2GiB、目标峰值5.72GiB，复制两套激活不是本次选择。请求故意设置旧字段batch2/no_async=false/serial=false以验证显式入口不受它们控制；实际拓扑会单独打印，不把请求值冒充执行行为。
- 复用既有普通WGSL单批次resident分支；主干linear在能力/shape匹配时使用既有tracked coopmat，否则保留原F16 WGSL可用性。不在错误后切换设备/后端，不返回手工raw分配/导入路径。
- 持久权重、gamma在首次编码前上传；gamma保留至final finish。pack scratch的最后GPU读取已编码后才允许后续GPU写复用，WGPU跟踪依赖；不允许host上传覆盖未提交的池内中间量。
- **`14716ac`**：最终生命周期复核发现attention的小型全1 mask仍在编码期间从通用池host上传。将其独立上传为不可变普通buffer，由WGPU保留至GPU使用完成，不再向池捐赠该独立分配。避免依赖shape巧合来排除host写与待执行中间量复用，不宣称这就是旧黑屏根因。
- 运行不传 `--diagnostics`，因此不输出逐操作同步JSON；保留 `UTA_ROFORMER_PROFILE=1` 的阶段/最终浮点统计，以及独立观察器的启动、实时输出同步和CPU/GPU采样。用户明确授权关闭的是运行时诊断，不是伪造或取消启动记录。

## 提交前记录与验证

规划 `20260907T171444-1bef00d26e3d`原提出提取resident模块；后续用小型公共encoder避免复制，保持engine.rs 1989行，所以未执行提取。一次记录器调用因重复input参数写法错误在argparse阶段退出，没有启动实现或测试；修正后的实现意图 `20260907T171633-85e3e470cd55`。

性能提交 `20260907T172247-c45483fe2f88`；mask修复意图/提交 `20260907T172605-5a66e2910019` / `20260907T172628-43bd7b7c3290`。每项先记录，单独提交后才执行变化代码。

- 最终 **77项CPU测试通过**（worker28/runtime49），**20项GPU测试ignored**。含实验解析/旧参数忽略、普通模式仍拒绝同一请求、已有profile/device验证、tracked/ordinary形状选择。新增完整合批子块GPU数值/池复用用例只编译，不额外执行。
- 测试 `20260907T172629-43d502cb317c`，定向GPU all-targets检查 `20260907T172633-8b5d6de4dbcc`，Release构建 `20260907T172635-12ace7e3acf5`均成功；CPU-only feature编译 `20260907T172301-b2c5eaf651f8`通过。
- 默认与实验参数的CPU-only quit协议smoke `20260907T172803-182146827a35`通过：stdout仅Ready，stderr为空，未创建GPU设备，也没有意外启用诊断。

## 当前证据与执行边界

**`test-artifacts/roformer-b580-xe90-throughput-once-20260907/`** 保存独立worker、请求、`build-and-inputs.json`、committed/dirty补丁及Cargo.toml/lock快照；构建代码 `14716ac`，继承的其他工作树变更不覆盖。准备记录 `20260907T172803-867266352bd9`。

仍是原354.880秒F32输入、原XE90 F32 GGUF、chunk881559/overlap2、Intel ICD/B580。原音频/模型只读；唯一新输出目录。运行前检查CPU/两GPU/nvtop，不终止其他任务、不等阈值空闲；本任务构建结束后才启动模型。

准备完成后执行如下。退出后需核对原始浮点统计、FLAC格式/时长、与上次Rust及GGML参考的输出差异、主机采样与boot/kernel状态。不把一次成功提升为production_ready或唯一根因证明，不更改GGML稳定参照。

## 无诊断性能运行：17/38后再次黑屏

- 执行 **`20260907T173231-cd09975e8495`**，worker **PID54013**，启动HEAD `cb4b91b`，构建代码 `14716ac`。**17:32:35.040234 UTC**实际启动，已记录B580/ANV及 `experimental_tracked_batches` 有效拓扑。CPU/GPU前检 `20260907T173128-289fcab04e16` / `20260907T173129-097906417510`：CPU10.64%、另有rustc；B5802%/47°C/41W、AMD桌面gfx0.74%。本任务构建已结束，不终止别人的任务。
- 用户再次报告黑屏，要求记录/分析并新授权一次开启log的GPU重试。原boot `69cbc1cc-f99c-4022-a74b-306e761dea36`已变为 **`2778fb70-768e-4dc9-abb3-3b2a2a64c211`**；原worker/记录器进程不在，无result或输出。不是未启动。
- **17/38完成**，其平均chunk时间 **10.605788503秒**。此部分数据不能证明全曲完成或超过GGML，不能用外推冒充结果。
- 第18个chunk内，10组time/freq都有finish记录；第11组time的QKV、attention、projection、FF1、GELU、FF2 **CPU编码**计时均已打印，最后FF2为906.378µs，没有后续time.finish。其后的residual/可选norm/submit/wait缺少操作前日志，**不能据此说FF2 GPU shader就是故障位置，或finish尚未启动**。
- stderr284002 bytes，NUL0，按授权没有runtime_diagnostic事件。880个进程、176个host样本，最后样本 **17:35:44.627090 UTC**，主线程R/wchan0；这一个采样点不建立随后阻塞位置。CPU2.51–100%，非隔离。
- 17次池快照一致：24 buckets、150 buffers、5,585,720,928 bytes /5327MiB。目标VRAM峰值 **6,040,964 KiB /5.76GiB**，与先前成功串行5.72GiB接近，不支持“应用池持续增长”这一简单解释，但不能排除driver/内存hazard。
- 中途nvtop `20260907T173412-94ced6736be7`：B58096%、2850MHz、128W、61°C、整卡约6.80GiB。这不是故障时刻温度/功率。稳定GGML曾观察到138W/70°C，不能据此归因简单温度/功率阈值。
- 事故重解析 `20260907T174005-aced2a0570c7`，被动内核采集 `20260907T174006-8b9497abc204`：先前boot journal不可用，`/sys/fs/pstore`不存在（采集命令因此非0，不能视为内核检查通过）。进一步边界分析 `20260907T174124-4df6a38cded5`，保存至旧目录 `incident.json`，原日志/二进制不改。

### 为什么仍可能失败：当前证据边界

相对成功串行版，此次将整个子块放入一条命令流，跨算子的同步由WGPU资源追踪处理，host只在子块末等待；同时取消了大量同步诊断I/O间隙。**更长连续批次、driver命令/资源跟踪、批内scratch复用及持续负载**都是待查方向，不是已确认根因。源代码仍保留最终完成/readback、错误传播与跟踪缓冲；未发现可据此次日志直接证明的use-after-free。B580能力已满足时，构造器的prefer只影响不支持adapter的分支，feature/limit/pipeline协商相同，不是因prefer=false另换了一条feature链。

## 新授权：同一保存二进制加诊断，重试一次

本次不是自动重试：用户明确说“记录下来 然后试图分析为什么， 下次开启log，重试一次GPU”。据此准备 **`test-artifacts/roformer-b580-xe90-throughput-diag-once-20260907/`**，原样复制失败运行保存的worker及构建/dirty/manifest记录，源码仍 `14716ac`，不重建、不改算法/调度/精度。只增加 **`--diagnostics`**，任务ID/输出目录为隔离证据而改变，模型/音频/旧三字段/ICD/profile均不变。

准备 `20260907T174227-2f44dd8233a1`；双参数CPU-only quit smoke `20260907T174227-587f1b3a0efb`通过，stderr为同步Ready begin/complete，stdout协议不变，无GPU上下文。复用同一已完成77项CPU测试/定向检查/Release构建的二进制，不重复无变化的构建和GPU单测。

准备后的实际重试如下。同步日志改变时序/负载，可能改变复现概率；即使不再黑屏也不等于已修复或证明某个根因。不自动再试或切换后端。

## 同一性能版加诊断：38/38，退出0

- 执行 **`20260907T174737-a481edfe59fc`**，PID **6864**，启动HEAD `62b5fff`（复用源码 `14716ac` 的已保存二进制，不重建）。**17:47:38.563712 UTC**开始，**17:56:24.312747 UTC**结果，墙钟 **525.713031秒 /8分46秒**，chunk合计523.347539秒，**38/38、done=ok、退出0**。
- 用户也报告“居然跑完了”。启动/退出boot一致；退出后 **388秒**仍为 `2778fb70-768e-4dc9-abb3-3b2a2a64c211`，主机可响应。内核复核 `20260907T180252-d3df9b09d806`窗口无GPU hang/reset，但18:00:01 UTC再次出现FAT nvme2n1p1未正常卸载警告；不说内核日志为空，不执行磁盘修复。
- **129518条诊断全部配对**，无错误、未配对、NUL、损坏JSON或raw阶段事件。38次池快照相同，24 buckets/150 buffers/5,585,720,928 bytes；目标VRAM峰值 **6,040,964 KiB /5.76GiB**，与失败版本峰值相同。CPU1.94–100%，非隔离；2422次进程/488次host采样，91次样本读取错误，observer errors为空。
- 有效FLAC24/44.1kHz/双声道/354.880秒，解码125,201,664 bytes（15,650,208 frames）。**解码PCM与上次成功tracked串行版逐字节一致**；不是对未保存的原始浮点做bit-exact断言。编码前profile也相同：samples31,300,416、non_finite0、peak1.1223633、RMS0.1847233757956834。既有468个整数满幅样本及GGML参考差异指标随PCM一致而保持，未重新宣称这些峰值不受FLAC转换影响。
- 输出probe/解码/比较：`20260907T175952-b51ad4f3634d` / `20260907T175956-b17e7dba4be7` / `20260907T180000-4f243767e5cd`。最终分析 **`20260907T180251-389421b51a6b`**，保存 `summary.json` / `output-parity.json`。原GPU检查77项CPU测试、20项ignored GPU测试的范围不扩大。
- 途中按前13块估算495–505秒，实际更长：后续chunk17/20/21分别18.80/18.42/22.30秒，不能保持最初13秒/块的估算，也不能把CPU峰值直接当作唯一原因。实际比GGML402.623654秒长 **30.57%**，比前一带诊断串行621.995629秒短 **15.48%**。日志事件数/同步I/O和背景负载均不同，不把差值全归因GPU算子或调度。

### 新发现：计算引擎/队列并非相同

只重解析已有fdinfo，没有另起GPU探针：GGML目标client74的CCS cycle增量6,959,636,554、BCS3,177,668；成功Rust串行client92的RCS6,349,893,968；失败无诊断client201的RCS2,923,709,571；本次成功client80的RCS6,360,830,811。**GGML在CCS/BCS，Rust各次均在RCS**。这些计数说明使用哪个引擎，不是跨运行可直接比较的利用率。

源码对应：wgpu-hal29.0.4 `vulkan/adapter.rs:2232–2234`取第一个queue family并要求GRAPHICS，`:2844`默认family0；固定GGML源 `ggml-vulkan.cpp:6449–6454`默认优先非图形compute queue（另有 `GGML_VK_ALLOW_GRAPHICS_QUEUE`覆盖）。RCS也能计算、桌面也用它；CCS是计算命令引擎，但仍共享执行资源/显存/功耗预算。**这是真实差异和后续排查点，不是已确认黑屏根因；Rust两次成功也用RCS，不能说RCS必然失败或CCS必然安全/更快。**

被动检查 `20260907T175022-1817fb6a6328`，源码检索 `20260907T180001-98e4121ad047`。当前boot RCS/CCS均job_timeout5000ms、preempt_timeout640000µs、timeslice1000µs；没有修改。这不是失败boot参数或超时报告。诊断host begin/complete间最大wait约0.539秒含日志/调度，不是GPU timestamp，不拿它判定hardware timeout。wgpu源另存本次目录 `source-audit/wgpu-hal-vulkan-adapter.rs`。

**本次诊断重试授权已完成。** 同一二进制无诊断失败、有诊断成功，提示时序/持续负载/交互差异值得查，但日志不是修复，且重启/背景状态也不同；不提升production_ready，不更改GGML稳定参照。

## 用户最新追加授权：显式计算队列候选

用户询问RCS/CCS区别，并建议“继续优化一版 开log 再跑一次”。新范围为：研究并实现独立实验计算队列选择，保留tracked buffer、现有精度和合批调度，CPU验证后**一次同模型同曲B580全曲，开启诊断**；不加GPU探针/自动重试，不改生产默认。

准备审查发现wgpu-hal `open_with_callback`虽允许改queue infos的Vec，但末尾仍把原始family_info（family0）交给 `device_from_raw`；此外buffer/image shader屏障默认包含VERTEX/FRAGMENT阶段，不能直接放到纯计算队列。不能只改回调就宣称已合法切换。计划 `20260907T180724-35e18028b165`，依赖适配记录 `20260907T182145-50edec4afd52`。

### 已实现并验证的候选 `f651ae6`（全曲结果见下）

- **`81d8a9e` / `7ddccb0`**：将已解析wgpu-hal29.0.4作为项目内第三方依赖保留，维护原MIT/Apache声明，Cargo仅增加局部patch/对应锁文件来源；不编辑Cargo缓存、不引入hash校验、不把第三方模块误当应用源文件拆分。
- **`80bbfea`**：HAL使用回调后实际queue family，并保留该family的flags供shader buffer/image屏障映射。普通GRAPHICS队列映射不变，纯COMPUTE队列shader访问使用COMPUTE_SHADER，不削掉读写access mask或transfer/host/indirect同步。device、queue、command pool均用同一family。
- **`d5075c4`**：worker新增default-off `--experimental-compute-queue`，隐含既有tracked-throughput；正常调用保持family0与原legacy检查。先选有队列的非图形COMPUTE family，经同adapter/同features/limits的HAL打开再交回WGPU；记录selected/HAL/WGPU family。仍为WGPU持有buffer、encoder、提交与清理，不回到手工raw分配，不因创建错误切换另一个设备。缺少明确请求的compute queue能力则返回错误。
- 用户随后强调XMX约109TFLOPS、FP32 XVE约13TFLOPS及不走SYCL。**RCS/CCS是调度引擎，XVE/XMX是算术资源，不能混为一谈。** 主要GEMM已经用F16/F32 cooperative matrix，但没有XMX实际利用率证明。保留现有矩阵shader、tile、精度及完整attention上下文；不把理论峰值或GPU忙碌率当有效吞吐。
- **`f651ae6`**：独立加入可选矩阵shape/逻辑与padding FLOPs、嵌套pack/GEMM/bias GPU区间，复用 `UTA_STUDIO_WGPU_TIMING=operators`。marker/resolve/copy仍在原batch，完成后才map，不增加中途推理submit/wait。计时按实际queue `timestamp_valid_bits`处理回绕/无定义高位后再转浮点；CPU覆盖36/64bit回绕。诊断覆盖timestamp资源创建、resolve和report。父算子含子阶段，分析时不能重复相加；GPU区间包含调度/内存/屏障影响，不是XMX硬件active百分比。

最终验证：**82 CPU tests通过、20 GPU tests ignored** (`20260907T185217-1b89b8816500`)，all-target检查 `20260907T185231-4fdf0d2d6e55`，Release `20260907T185238-49289b22533d`，CPU-only feature检查 `20260907T185537-de0fd0e7abd2`，全flags Ready/quit `20260907T185724-41b27a7dac56`均通过，无GPU上下文。此前HAL单独测试受到非workspace package/dev-dependency规则及offline缺少mach-dxcompiler-rs阻塞（`20260907T183538-df35fca19925` / `20260907T183616-a53fdc50f2c3` / `20260907T183703-5c9c84f36864`）；应用CPU测试直接覆盖实际已修改的公开HAL barrier映射，不能把未执行的上游测试报为通过。一次误用package名的检查在解析阶段失败，已以正确 `uta-roformer-worker` 完成检查。

运行准备 **`20260907T185539-90543438ff60`**，目录 `test-artifacts/roformer-b580-xe90-compute-queue-diag-once-20260907/`：保存f651ae6二进制、committed/dirty补丁、manifests、输入身份、request与CPU smoke。待CPU/B580/AMD及nvtop检查后，按新授权启动**一次同曲全曲**：`--experimental-throughput --experimental-compute-queue --diagnostics` + `UTA_STUDIO_WGPU_TIMING=operators`。与前次相比同时增加计时，因此是带诊断的分项分析运行，不是无混杂的queue墙钟A/B或最佳吞吐成绩。不加GPU探针、预热或自动重试。

### 输出目录准备错误（不是GPU推理失败）

**`20260907T190007-717d6905d844`** 确实启动worker **PID40020 /19:00:09.772769 UTC**，但agent遗漏提前建立隔离output目录，任务以 `Output directory does not exist` 返回，进程6.6ms后退出0（协议明确error，不能以退出0报成功）。完整4条诊断为Ready begin/complete和task begin/error。`run_task`的目录检查位于model路径、权重、音频和device初始化之前；这才是**没有GPU上下文或推理**的证据，不靠空采样推断没有启动进程。

保留原 `run/` 与 `preparation-error.json`。修正准备 **`20260907T190140-263ae8e9fb11`** 仅建立本次已授权临时output目录，二进制/请求/算法不变。实际唯一GPU运行仍待完成，后续观察目录改为 **`run-gpu/`**，不覆盖错误记录；不是重试一次GPU失败，更没有加入自动重试循环。

## CCS + 算子计时全曲完成；B580暂停

实际GPU执行 **`20260907T190248-66d4c10e1865`**，worker **PID40965**，**19:02:50.239726 UTC**开始，**19:17:02.568308 UTC**结果。启动HEAD `90cde3d`，保存二进制源码 `f651ae6` 加已保存相关dirty sources。

- **38/38、done=ok、exit0、852.294405秒 /14分12秒**，chunk合计850.336446秒。第1–3块20.79/20.40/20.39秒，另有约32秒慢块。CPU2.23–100%，非隔离，不能只归因于队列选择或日志。
- **487634条诊断全部配对**，无error、raw阶段、未闭合、损坏JSON或NUL，所有矩阵shape均有对应GEMM timing。分项计时含marker/query/resolve/map、额外同步诊断；比前次525.713秒更慢**不是CCS必然更慢的证据**，也不是无日志最佳性能成绩。不能把852−GPU子块时间都算成日志开销。
- selected/HAL/WGPU均 **family1 /queue0**，flags为COMPUTE|TRANSFER|SPARSE_BINDING，timestamp_valid_bits64。target client156 **CCS 152→6,515,453,138 cycles**，RCS547保持不变；初始化其他client无增长。此次已经实测使用CCS，不只是源码推断。
- 38次activation池快照完全相同：24 buckets/150 buffers/5,585,720,928 bytes（5327MiB）。采样目标VRAM **6,028,796 KiB /5.75GiB**，tree RSS1,259,272 KiB。3914次进程/787次host样本，270次读取错误，observer errors为空；不能把采样峰值当硬上界。
- 输出 **FLAC24、44.1kHz、双声道、15,650,208 frames /354.880秒**。125,201,664 bytes解码PCM与上次成功tracked串行版 **逐字节一致**；不是对未保存原始F32的bit-exact证明。编码前31,300,416 samples、non_finite0、peak1.1223633、RMS0.1847233757956834；既有468个满幅样本及GGML差异指标不因相同PCM而消失。
- 启动/退出boot均 `2778fb70-768e-4dc9-abb3-3b2a2a64c211`；退出约259秒后仍可响应，同boot；内核19:02–19:21窗口无条目。只说明观察窗口，没有证明普遍稳定或定位原黑屏根因。

### GPU瓶颈：不是只看XMX峰值

1216个Transformer子块共 **336.662624 GPU秒**（不是全曲墙钟）：time attention **254.956211秒**、frequency attention **22.203822秒**，合计 **82.33%**；四个主要GEMM合计 **16.371976秒 /4.86%**。父子区间重叠，不能重复相加。

| GEMM | M×K×N | 次数 | GPU秒 | 逻辑FLOPs/区间，TFLOPS |
|---|---|---:|---:|---:|
| QKV | 154980×256×1536 | 1216 | 6.219113 | 23.83 |
| attention output | 154980×512×256 | 1216 | 1.976277 | 25.00 |
| FF1 | 154980×256×1024 | 1216 | 3.263727 | 30.27 |
| FF2 | 154980×1024×256 | 1216 | 4.912859 | 20.11 |
| Mask MLP | M1722，多组K/N | 3610 | 0.691338 | 4.96 |

这是 `2*M*K*N / GPU区间`，已另存padding工作量；区间包含调度、内存及屏障效果，不是硬件XMX busy百分比。109TFLOPS FP16 XMX与约13TFLOPS FP32 XVE是用户讨论的理论峰值，不是实测，也不能相加当模型吞吐。**该B580图后续首先应审查attention，不把主要GEMM提速当作全图数量级提速；该占比不能直接套到AMD。**

用户观察92/380W及间歇空档。被动读数 `20260907T191024-57b0bdf78e0a`确认 `power1_cap=190W`、`power1_crit=380W`，没有改功耗/超时。同步日志与计时确有等待开销，但不以功耗比推算算力或性能空间。用户询问异步日志：已解释吞吐与断电尾部证据的取舍，**本次没有改异步日志**，没有在运行中改设置。

证据：`run-gpu/result.json`、`summary.json`、`output-probe.json`、`output-parity.json`。最终解析 **`20260907T191838-ce35ceb77b5e`**，probe/解码 **`20260907T191843-c5a29388a4be`**，PCM比较 **`20260907T192121-b3e27c342e14`**，postexit kernel **`20260907T192121-3b43fa83c0c9`**。源码identity空匹配，完整扫描仍被保留的历史GGUF原始metadata挡住（`20260907T192121-0ac2d0962ca1` / `20260907T192121-19460bf89532`）；不删历史证据冒充通过。

## 最新夜间授权和优先级

用户去睡觉，明确要求本次结束后落地记录，**B580等待用户回来再测**。期间广泛授权AMD核显测试，并纠正顺序：**先将Qwen ASR/Forced Aligner实际模型计算接入Rust/WGPU、用AMD验证/调性能，再继续RoFormer系列AMD调试**。现有Qwen仍是Rust worker调用C++/GGML，不能把包装层算成Rust模型图。AMD/Intel可有各自适合形状/硬件的GEMM；保持native WGPU/Vulkan、不引入SYCL、不偷换精度、不修改用户模型/输入。AMD“不会黑屏”是用户经验，不代替实际观测或给出普遍稳定保证。无需逐次重复请求AMD授权，但继续记录操作/提交/负载/结果，不自动重试、不执行整仓release/Nix检查、不提升生产状态。

授权记录 **`20260907T191655-3bfc6a0fe52b`**，后续工作见 [Qwen Rust/AMD实施记录](QWEN_RUST_AMD_2026-09-07.md)。
