# AMD GPU 12 秒 RoFormer 对照验证（2026-09-07）

用户授权：“可以先在 AMD GPU 上跑12秒切片验证，先做到 AMD 上能超越 GGML”。本轮在 AMD 集显执行同输入的 Rust/GGML 对照；不启动 Intel B580 推理。现有 batch=1、no_async、serial_pipeline、完整 chunk/overlap 和错误传播保持。

设备由 sysfs 只读确认：PCI `0000:10:00.0`、vendor/device `1002:15bf`、`renderD129`。两边进程显式设置 `VK_DRIVER_FILES` 与 `VK_ICD_FILENAMES` 为 `/run/opengl-driver/share/vulkan/icd.d/radeon_icd.x86_64.json`，指向 Mesa26.2.2 的 `libvulkan_radeon.so`；Rust 选择 `integrated_gpu`，GGML 选择该 ICD 下的 index0。实际设备和已加载库以运行证据为准。

输入是已有354.88秒源音频的60–72秒，12.000秒／44.1kHz／双声道 F32 PCM。测试 WAV 是原生边界的临时无损中间文件；用户源文件只读。初始 Denoise 对照使用相同 GGUF、chunk352800、overlap4；没有将12秒解释为缩短模型上下文。

证据目录：`test-artifacts/roformer-amd-12s-20260907T1139/`。各操作由 `tools/record-operation.py` 先保存提交与完整命令；`tools/observe-roformer-run.py` 记录实际目标启动意图、PID、进程树 GPU 库／DRM 设备、显存与真实进程墙钟。观测包装器的 stdin、非零退出及 INT/TERM/HUP 单次转发已用隔离 CPU 程序验证；无超时、自动重试或备用设备。

当前 Rust 初始二进制来自 `74a113a` 的已保存 Release 构建；其中权重布局优化为 `38120af`，未验证队列上传分支已在 `d24c282` 撤下。普通／cooperative kernel 仍按原确切能力匹配在创建设备前选择，不把支持其他矩阵形状当成支持 B580 的8×16×16实现。

## 最新：考虑系统负载后恢复小优化（2026-09-07 23:38 JST）

用户指出其他CPU/GPU测试同时进行，并建议使用 `nvtop`。因此先前连续加载候选的单次结果只能说**收益未定**，不能判为无效。下节的撤回是当时的决定，本节的重新测量和 `9ba2a63` 恢复提交为当前状态；RoFormer/WGPU源代码与已构建的 `6a759cf` 一致，当前可执行文件为 `test-artifacts/roformer-cpu-stft-continuation/rust-worker-contiguous`。

### 观测工具与A/B/B/A

新增 `tools/observe-host-load.py`，模型运行前人工查看CPU总占用/主要进程、sysfs GPU busy与DRM客户端；`nvtop --snapshot` 补充本机B580统计。nvtop当前只列B580，AMD仍由sysfs/DRM覆盖。已有worker观测器现在还在运行中每秒保存全系统样本，预/后采样不计入目标墙钟。xe的原始cycle计数按同引擎total-cycle差值解释，不猜频率；缺失/权限不足保持未知。6项host单元测试及2项CPU命令集成测试通过，故障不会阻止或重试目标，不引入闲置门槛。详见 [操作记录说明](ROFORMER_OPERATION_RECORDING.md)。

同一Denoise 12秒输入、同一模型/chunk352800/overlap4及三个串行参数；两版均开启operators/profile，使用同一新版观测器。A为旧加载方式，B为连续逻辑行加载。每次运行前均检查CPU和AMD；从用户提出nvtop起额外保存该快照。

| 项目 | A平均（旧） | B平均（新） | 变化 |
| --- | ---: | ---: | ---: |
| 进程墙钟 | 24.818 s | 24.672 s | 少0.59% |
| 主GEMM GPU合计 | 12.586 s | 12.516 s | 少0.56% |
| subblock GPU合计 | 19.272 s | 19.188 s | 少0.44% |
| Mask GPU合计 | 2.518 s | 2.436 s | 少3.25% |

四次墙钟依次24.797/24.658/24.687/24.838秒，全部1058400个解码值有限、逐样本一致。CPU运行中中位占用3.95–4.36%，峰值10.48–19.34%；非目标AMD引擎采样峰值15.7–21.2%，可见所有者为桌面合成器。重新解释已保存的xe原始cycle数据还看到Intel上的终端/面板/桌面少量活动，没有将旧采样说成新增现场观测。**这仍非隔离机器或统计显著性证明，但两次B及对应算子时间均有小幅改善，故保留小优化而不宣称重大提速。** 不把原先较大的约1.9秒墙钟差全部归因于它。

证据 `test-artifacts/roformer-amd-load-aware-abba/summary.json`、`host-review-with-xe.json`；四次启动为 `operations/20260907T141249-5aa20118842d`、`20260907T141523-99e10e3ecf68`、`20260907T141628-583ebe51fba3`、`20260907T141737-f1fd6f5811bc`。B二进制仍指向其原构建提交6a759cf，不把运行时观测器HEAD当作该二进制的源码身份。

恢复后另以相同预/运行中负载观测逐项完成Dereverb/Harmony/Leap：20.749/24.729/47.312秒，四份导出音频均与上一保留版逐样本一致；这些是各一次模型回归，不是另外三组A/B。记录 `test-artifacts/roformer-amd-contiguous-continuation/family-summary.json`。

### 新GGML参照与尚未闭合的差距

同一输入与参数的新Denoise GGML运行仅 **12.443秒**，输出与原GGML逐样本一致；Rust B平均24.672秒，对统一导出钳制后的GGML SNR仍53.880dB。双方使用相同新版进程/负载观测器；Rust额外开启算子timestamp，GGML沿用原diagnostic log，因此细小差别不作归因，但**目前约2倍墙钟差距仍在**。旧GGML34.940秒的异常慢参照不能用于宣布Rust已超越。新参照也不是全系列/长期稳定证据。记录 `operations/20260907T143017-2af6aa101552`，`test-artifacts/roformer-amd-load-aware-abba/ggml-comparison.json`。

一次同模型的RADV编译诊断确认两个寄存器GEMM已含F32 FMA指令，NIR报告64-lane subgroup；不能把重复添加FMA当作优化。部分机器码指令未被当前反汇编器解码，不等于Vulkan执行无效，也不能据此声称完整寄存器/spill统计。诊断临时关闭本进程shader cache并开启dump，**其墙钟不用于性能比较**。源码/反汇编依据：`test-artifacts/roformer-amd-compiler-continuation/audit.json`及完整stderr。

当前剩余重点是主GEMM（仅这一组在Denoise已耗12.516秒GPU时间）及attention，而不是继续靠毫秒级CPU准备优化追平整模型。下一步需核对AMD实际cooperative-matrix能力/元组与GGML真正选核，并评估其与当前WGPU跟踪、FP32累加和原等待的兼容性；不能因为GGML打印KHR_coopmat就套用B580的8×16×16路径。保持目前精度/上下文/路由，任何候选仍需带系统负载的同条件数值和性能验证。

本轮仅AMD 12秒推理；B580只读占用观测不是B580推理或稳定性验证。正式路由、READY、production_ready不变，整机历史断电问题未解决。

## 前一接续阶段：CPU与Mask验证（2026-09-07 23:00 JST）

用户转交任务锁后，接续已有 AMD 12秒范围；未启动 B580、全曲、并行 GPU 或默认 ignored GPU 测试。当前保留的 RoFormer/WGPU 源码与 `54690f4` 相同（`441762d` 撤回了后续无明确收益候选）。二进制为 `test-artifacts/roformer-cpu-stft-continuation/rust-worker-mask-scratch`，构建记录 `operations/20260907T134454-fcde5dcae322`。原有工作树改动保留，HEAD 不代表全部 dirty 源码。

### 补齐上一轮断点

- `f273b6e` RMS 向量访存已有 Denoise/Dereverb/Harmony 运行：25.662/22.849/79.814秒；输出均与各自旧 Rust 逐样本一致。Harmony 79.814秒期间最后采样 DRM engine 累计仅22.839秒，不能将两者之差直接当成某个 GPU 算子、CPU 调度或驱动故障耗时。原证据 `rms-vec4-denoise.json`、`rms-vec4-dereverb-harmony-parity.json`、`rms-harmony-timing-anomaly.json` 保留。被动观测器 `8d20976` 已经通过 CPU fixture，不重做该测试。
- `afaa7a9` Mask F16 原本已通过 CPU 布局测试及 Release 构建，缺少模型执行。本轮补齐三个 F16 模型及 F32 Leap 的真实输出验证；原 F32 权重仍走 F32 Mask，不新增量化。
- 新增 `54690f4`：STFT/iSTFT 每次变换只分配一次 FFT scratch，所有帧串行复用；不缓存设备、不改变窗口、反射 padding、FFT、重建累加顺序或 GPU 同步。已有 STFT 签名格式调整一并保留。

### 四模型结果：同一12秒输入、原chunk/overlap、全部串行参数

本表均开启相同的 operators/CPU profile；GGML 栏引用上一轮关闭额外计时的历史记录，**不是本轮重新配对，也不能比较细小百分比**。

| 模型 | 先前 RMS Rust墙钟 | 当前保留 Rust墙钟 | 先前 GGML墙钟 | 输出与先前 Rust |
| --- | ---: | ---: | ---: | --- |
| Denoise | 25.662 s | 26.640 s | 34.940 s（首次29.354 s，已有异常波动） | 逐样本一致 |
| Dereverb | 22.849 s | 20.977 s | 11.092 s | 逐样本一致 |
| Harmony | 79.814 s（停顿原因未定） | 25.233 s | 14.775 s | lead及残差均逐样本一致 |
| Leap XE90 | 未跑RMS版；register64为51.962 s | 47.950 s | 48.033 s | 与register64逐样本一致 |

每个输出1058400个有限样本，协议均 `done.status=ok`；全系列目标仍未完成。不能将 Harmony 本次未出现长停顿说成已修复其根因，也不能用 Leap 的约0.08秒差称为稳定超越 GGML。

Denoise 采样进程树峰值 RSS 从 RMS 版1945788 KiB降至1172636 KiB，少755.0 MiB（39.7%）；同字段 resident-gtt 从2560476降至2429352 KiB。Mask首次准备日志确认 F32 布局值数降至0，180份权重走F16，共201265152值。RSS是采样的进程树求和，可能重复计算共享页；DRM字段不相加。这里保留的是内存改进，不是已证明的端到端提速。

运行与汇总：`test-artifacts/roformer-amd-mask-continuation/summary.json`。四个启动记录分别为 `operations/20260907T134546-34300565c2d5`、`20260907T134733-c4b2f3f8694d`、`20260907T134822-a2b44ca7d2b0`、`20260907T134941-dadf2e95232c`。Harmony 的 `--input` 元数据误写了一个不存在的 karaoke 缓存路径；启动前保存的 `stdin.bin` 正确记录实际 `Documents/.../melband_roformer_harmony/model-fp16.gguf`，执行请求未错。后续只补记当前stat，没有伪造该文件的历史启动前stat。

### CPU 前后处理与撤回的候选

CPU-only `profile_stft` 比较旧/新/新/旧两个独立保存的 Release 可执行程序，每个模型形状每版两次。MelBand 的单声道前向+逆向平均12.540→12.022 ms，Leap 23.820→22.372 ms；这是合成音频、含分配及FFT规划的小样本计时，不能外推整模型或统计显著性。18项CPU测试通过，包括全频谱/任意复数mask重建的逐位比较、短输入、静音、脉冲、非整尾部和空逆变换。证据 `test-artifacts/roformer-cpu-stft-continuation/summary.json`；首次构建因 `dev.sh -c` 参数组装错误未启动Cargo，修正显式 `bash -c` 后成功，失败记录保留。

`6a759cf` 尝试将 GEMM 共享tile按逻辑连续行加载，并将未读取的填充列移至K循环外初始化；精度、累加、边界、等待不变。4项CPU测试与构建通过，Denoise输出逐样本一致，墙钟26.640→24.743秒，但subblock GPU合计19.190→19.231秒，Mask2.489→2.433秒，**没有明确算子收益**。已在 `441762d` 单独撤回，未扩跑其他模型；不能仅按墙钟挑赢家。候选二进制、命令及 `test-artifacts/roformer-amd-contiguous-continuation/denoise-assessment.json` 保留。

本轮采样只出现 AMD PCI `0000:10:00.0`／RADV，boot ID保持 `a4acf780-23bb-453d-9a68-5bf560b86a55`。sudo只读kernel review覆盖22:45:40起至最后检查，没有条目；普通权限读取失败也保留。退出正常、有限观察及无日志不保证退出后或长期稳定，B580断电根因未解决。

定向GPU-feature all-targets检查、CPU测试、Release构建通过；未运行工作区/Nix正式发布检查。完整身份脚本因先前只读GGUF检查日志里的旧 `general.description` 元数据命中失败，保留原始模型和不可改写的操作证据；不将此失败报告为通过。

**下一步：** AMD实测主线性层仍占Denoise GPU时间大头，且Dereverb/Harmony仍慢于GGML；继续在同精度、原同步和原上下文内审查GEMM编译结果/数据复用与attention成本，勿重复已撤回的RMS64、K展开或本次连续加载候选。正式路由、READY和production_ready均不变。

## 实测结果

### Denoise 初始对照

| 项目 | GGML | Rust 初始版本 |
| --- | ---: | ---: |
| 目标进程墙钟 | 29.354 s | 56.731 s |
| 完整输出 | 12秒 / 1,058,400样本 | 12秒 / 1,058,400样本 |
| 实际 GPU | Radeon 780M / RADV PHOENIX | Radeon 780M / RADV PHOENIX |
| 进程退出 | 0 | 0，协议 done.ok |
| 采样峰值 DRM resident-gtt | 1,005,620 KiB | 2,947,564 KiB |

GGML 使用 `KHR_coopmat`；Rust 未匹配 B580 的8×16×16能力，使用普通 `wgsl_packed_f16_f32`。Rust GPU 时间戳合计：subblock43.870秒、mask estimator5.222秒、band norm+linear0.127秒。时间／频率子层耗时相近，结合共同的线性层形状，优先优化普通 GEMM 的数据复用；这是定位方向，尚非每个线性核的独立计时。

初始音频 SNR 对 GGML 为53.760dB；将 GGML 浮点 WAV 同样钳制到 Rust 整数 FLAC 的[-1,1]导出范围后为53.880dB。GGML 有354个超出该范围的样本，因此原始 max_abs=0.036785不能全解释为推理误差；两边样本均有限。当前结果不证明逐位相同或全部模型等价。后续优化应另与保存的 Rust 初始输出比较，区分既有实现差异与本次改变。

记录：GGML `operations/20260907T114635-df76b027b99f`，Rust `operations/20260907T114819-e6eb738caaf2`；各自详细输出在本实验目录的 `01-ggml-denoise/`、`02-rust-denoise-initial/`，音频对照为 `initial-denoise-parity.json`。两次实际采样只有 RADV 库与 AMD PCI `0000:10:00.0`，没有 Intel Vulkan 库／DRM计算设备；执行前后 boot ID未变。

GPU 稳定性不能由数值通过或正常退出单独保证；这两次 AMD 切片未观察到断电，不推广为长期稳定结论。B580 断电根因仍未确认。

### F32 寄存器分块 GEMM（e25f51a）

普通 AMD RoFormer 的线性层新增32×32输出块／64线程／每线程4×4累加。保留F32输入、累加、输出和原有F16权重存储；原16行对齐的调度范围、缓冲区、提交等待不变。四项 CPU 测试覆盖 Naga、共享槽及输出唯一覆盖、偏移／尾部／转置／偏置／F16打包；Release构建通过。

12秒 Denoise 实测49.892秒，比初始56.731秒减少12.1%，尚慢于GGML29.354秒。GPU subblock合计40.077秒；mask5.750秒。1,058,400个导出样本与初始Rust逐样本相同（max_abs=0）；对GGML差异仍为既有约53.88dB SNR，不把它记为新核造成的误差。

执行记录：首次 `operations/20260907T120504-6dd9d10f4f5c` 因临时输出目录未创建而协议报错，尽管进程exit0也不计为成功或测量；错误输出保留在 `03-rust-denoise-register32/`。补齐临时目录后另行记录 `operations/20260907T120643-fef8a60babc1`，成功输出在 `04-rust-denoise-register32/`。音频核对见 `register32-denoise-parity.json`；只观察到RADV／AMD设备，前后boot ID未变。

继续审阅发现RMS归一化仍是一个工作组一个活跃线程，每个子层三次相同行数的归一化。下一处改动将64条独立行放入一个工作组，各行仍按原顺序F32求和；收益需下一次完整12秒切片实测。

### 撤回独立行 RMS 分组（a94a596 → 4faff4f）

64条独立行共享工作组的候选通过四项CPU测试与Release构建，AMD 12秒 Denoise输出也与初始Rust逐样本相同，但墙钟由49.892秒退化至83.815秒，subblock GPU合计72.302秒。结果没有支持最初的利用率推测，因此在 `4faff4f` 单独撤回该候选，恢复原RMS核；不以理论利用率替代实测结果。

运行 `operations/20260907T121727-a7d7154b768b`、详细日志 `05-rust-denoise-rms64/`、`rms64-denoise-parity.json` 和 `rust-worker-rms64` 均保留。执行正常结束，只观察到AMD/RADV，boot ID未变。退化的具体GPU成本分布尚待逐算子timestamp，不能把工作组大小、某个API或内存访问推测写成确认根因。

### 逐算子计时与64×64分块（c484833、6a377ea）

在已有GPU提交内增加可选的算子时间戳（环境变量值为 operators 时启用），沿用原有提交后等待；默认运行不启用额外查询。CPU测试确认原批次计时开关仍保留。寄存器32×32版本的实测主线性层合计29.060秒，RMS合计约3.23秒，因此后续优先改善GEMM。该次运行墙钟46.801秒，记录为 operations/20260907T122931-40735f5b7401，细分数据在 operator-timing-denoise.json。

6a377ea 将普通AMD RoFormer GEMM输出块扩为64×64，K块仍16，256线程每线程4×4 F32累加，共享内存8512字节。原16行对齐的分段、缓冲区生命周期、串行提交等待和F32运算顺序保持；合作矩阵路径保持原选择。四项CPU测试覆盖Naga验证、共享/输出槽唯一覆盖、尾部/偏移/转置/偏置/打包F16独立数值比较和选择范围；Release构建通过。

带算子计时的12秒Denoise墙钟31.817秒，subblock GPU合计21.185秒、mask2.505秒；主线性层约12.37秒。输出与初始Rust的1,058,400个解码样本完全相同。记录 operations/20260907T123658-87b5b9a2c819，证据 07-rust-denoise-register64/、register64-denoise.json；二进制 rust-worker-register64 及其build.json保留。

### 撤回K循环展开（7743a34、057b222 → 4c2d716）

显式展开K16的候选在CPU验证后运行：墙钟29.948秒，但subblock GPU合计23.040秒、mask2.983秒，均慢于循环版本。墙钟变动未得到GPU算子收益支持，因此4c2d716撤回展开及其专用测试，保留较简单的64×64循环版本。两crate源码与6a377ea相同。运行 operations/20260907T124912-e6ee2a1a61d9、08-rust-denoise-unrolled/、unrolled-denoise.json和候选二进制保留；音频仍与初始Rust逐样本相同。CPU专用测试最初因只遍历顶层Naga IR导致断言失败，在任何GPU执行前修正递归遍历并通过；它不属于GPU执行故障。

### Denoise关闭额外计时的配对结果

保留版使用6a377ea保存的Release二进制，运行时HEAD为4c2d716，两crate源码经Git比较相同。关闭额外GPU/CPU性能计时，仍使用相同观察包装器及原有全部串行参数。

| 同一12秒输入 | GGML | Rust保留版 |
| --- | ---: | ---: |
| 目标进程墙钟 | 34.940 s | 28.065 s |
| chunk / overlap / 完成chunk | 352800 / 4 / 6 | 352800 / 4 / 6 |
| 退出与输出 | exit0，12秒 | exit0，done.ok，12秒 |

本次配对Rust耗时减少19.7%。GGML此前为29.354秒，说明主机/运行有波动；不能仅凭一组近距离结果宣布全部模型或稳定超越。Rust对初始Rust输出逐样本一致，GGML对初始GGML也逐样本一致；对GGML钳制后SNR仍53.880dB，未产生新的数值变化。

记录 operations/20260907T125319-4378803e563a（GGML）、operations/20260907T125858-9de5c1c15359（Rust），详细数据09-ggml-denoise-paired/、10-rust-denoise-paired/、paired-denoise.json。实际只观察到AMD/RADV，进程正常结束、boot ID未变；保留版Rust采样峰值resident-gtt约2.56GiB，高于GGML约0.96GiB，性能进展不等于显存或长期稳定性已达标。

### 四个模型首轮配对及Denoise波动解释

以下均为同一12秒输入、同一真实GGUF、相同默认chunk/overlap、batch1/no_async/serial；Rust为6a377ea保存二进制且关闭额外计时。新结果不能作为全部模型胜出的证据。

| 模型 | chunk / overlap / 完成chunk | GGML墙钟 | Rust墙钟 | 对GGML音频SNR |
| --- | --- | ---: | ---: | ---: |
| Denoise | 352800 / 4 / 6 | 34.940 s | 28.065 s | 53.880 dB（相同导出范围） |
| Dereverb | 352800 / 2 / 5 | 11.092 s | 25.046 s | 49.730 dB |
| Harmony | 352800 / 4 / 6 | 14.775 s | 27.784 s | lead 56.183 dB；残差69.446 dB |
| Leap XE90 | 881559 / 2 / 2 | 48.033 s | 51.962 s | 42.157 dB |

输出均为12秒、1,058,400个有限样本。Harmony Rust额外计算/导出输入减lead残差FLAC，直接GGML只输出单神经stem WAV；残差参考为输入减GGML lead，并统一导出钳制。输入是现有音频切片，并非已确认孤立的全人声，故本实验比较引擎而不证明所有语义场景的听感质量。Leap仍完整1722帧上下文，没有因输入12秒而缩短模型默认chunk。

CPU实读三份MelBand GGUF后，687个tensor名称/shape/dtype完全相同；每份300个F16张量含227,934,720个值，384个F32张量含268,452个值。配置仅模型名称和Dereverb overlap不同，GGML图均801帧/1834节点。逐chunk的GGML batch.compute主机区间显示：Denoise第二次首chunk18.293秒、后续4.143/2.370/2.391/2.604/2.487秒；Dereverb每chunk2.008–2.147秒，Harmony2.238–2.344秒。因此Denoise的那次总墙钟领先包含未解释的运行波动，不能推导Rust稳态吞吐领先。启动/编译/频率/其他负载的具体影响未确证；这些主机区间也不是纯GPU时间戳。

新增配对记录：Dereverb operations/20260907T130029-3f1e4ddcff8d、20260907T130201-34e17a5e71b7；Harmony 20260907T130405-879ba75ed68a、20260907T130522-a23c3f62176b；Leap 20260907T130732-4e7e3380f3e3、20260907T130847-713611d86ca0。详细日志依次为11–16号目录；音频比较为paired-dereverb.json、paired-harmony.json、paired-leap.json；GGUF与GGML逐chunk对照为ggml-model-cost-comparison.json（CPU读取操作20260907T130723-efb7f13b7c6b）。

所有目标进程正常结束，只观察到AMD/RADV，boot ID未变。Leap Rust采样峰值resident-gtt 5,529,212KiB，GGML 2,366,444KiB；这是相同计数字段的进程观测，不能把不同DRM字段相加当作物理显存。当前结果没有满足全系列稳定超越GGML的目标；Rust仍为实验验证对象，生产路由及production_ready不提升。
