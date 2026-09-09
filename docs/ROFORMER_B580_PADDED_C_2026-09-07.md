# B580 XE90：保留padded C、去掉完整输出副本（2026-09-07）

用户根据[已确认的冗余分配](ROFORMER_B580_GROUP_SUBMISSIONS_2026-09-07.md)授权修复并再执行一次Rust全曲。范围：同一354.880秒输入、原始XE90 F32 GGUF、B580、chunk881559/overlap2、batch1/no-async/serial-pipeline，不加预热/探针/重试。

## 独立改动与边界

1. 记录器修复单独提交 `cd27bfc`：在既有采样循环同步stdout/stderr，不再仅在退出后同步。CPU子进程测试覆盖活进程日志同步和同步失败不重启目标；观察I/O开销纳入本次耗时。用户态缓冲和最后一个未同步区间仍可能丢失，不能承诺硬掉电日志绝不丢失。
2. 推理只修改 `wgpu-runtime/src/gpu/coopmat.rs`：在M-padding存在时保留已初始化、已导入的C缓冲，令 `GpuBuffer.len=m*n`，去掉第二个完整输出分配和前缀copy提交。原raw GEMM、可选bias和函数末完成/错误检查保留；descriptor、A/C初始化、feature协商和原有group提交候选不同时改变。

XE90 QKV：物理155008×1536，逻辑154980×1536。多出的28行仅168 KiB；不再为去掉它们分配/复制908.09 MiB。此项是确定的冗余工作修复，不预先认定为黑屏根因。

## 已审查的消费者

- `GpuDevice::download`使用 `buffer.len*4`，不会回读padding。
- raw后续F16打包按 `m*k`、逻辑count和显式padding边界处理；gather/attention、GELU、add和RMS按显式rows/dims/total处理，不用 `arrayLength`推断逻辑shape。
- `validate_buffer_len(c_len)`在原raw分配前已覆盖整个padded容量的buffer/binding限制，不新增限制。
- 原raw输出仍经 `release_linear_output`正常释放，不放入普通池的无人取用bucket。WGPU保留后续编码所需的原始buffer引用；存储容量不会因为修改逻辑长度而缩小。
- 新增CPU tile/prefix布局测试；既有显式ignored GPU形状测试增加物理padded容量和逻辑长度断言。本次不额外运行这些GPU测试，避免超出一次全曲授权。

## 参数启用的实时诊断

用户随后明确要求加参数启用，并且日志必须先于实际操作写入。新增 `uta-roformer-worker --diagnostics`，默认关闭；stdout仍是原机器协议，诊断为stderr单行JSON，包含时间、PID、配对操作ID、label、phase和begin/complete/error。

顺序为 **begin写出并flush／Unix普通文件sync_data返回 → 实际操作 → complete/error写出并同步**，不使用异步日志队列或人为sleep。终端/管道只flush，文件描述符为普通文件才同步落盘；本次观察器将stderr直接接到独立文件。若I/O失败会明确报错，不把失败当作已保存，也不跳过已有GPU等待/清理或添加模型重试。

覆盖task、权重/音频、设备初始化/pipeline等待、resident子阶段、raw buffer创建/分配/绑定/导入、F16打包、队列submit及GPU wait。`gpu.submit complete`只表示提交调用返回，必须结合后续wait complete才表示该等待成功。缺少结束事件表示完成未知，不能单凭最后一行认定故障根因。

该诊断改动与padded-C改动分开提交。本次全曲带 `--diagnostics`，同步日志的I/O开销计入测速，不能将差异全部归因于显存副本修复。

## 执行计划与状态

独立提交推理改动后，执行CPU测试、定向all-targets检查和Release构建；Rust/原生命令经 `bash dev.sh`，记录commit及dirty输入。再记录CPU/两GPU/nvtop负载，使用单独保存的worker和唯一输出目录执行一次全曲。原输入/模型只读，保留所有历史事故证据，不同时运行本任务的构建。

推理操作意图：`test-artifacts/operations/20260907T155226-fa1907232c2f`；日志修复意图：`20260907T155044-6dcb3c27241c`。padded-C修复已提交 `c04fa18`，参数诊断单独提交 `1ab2f47`。70项Rust CPU测试通过（worker23/runtime47，18项GPU测试ignored），记录器4项CPU测试通过；定向all-targets检查与Release构建通过。默认/`--diagnostics`两个纯quit启动用例保持stdout只有Ready，诊断模式stderr有配对begin/complete，未创建GPU设备。

进一步用真实worker的纯quit用例执行strace：`begin`的stderr写出后 **fdatasync返回0 → stdout Ready写入 → complete的fdatasync返回0**，验证顺序不是异步日志或sleep推测。此CPU证据在 `test-artifacts/roformer-b580-xe90-padded-c-once-20260907/diagnostics-order.strace`。

Rust测试/检查/构建：`20260907T160836-62258f5e5215` / `20260907T160844-dfa9aceb1773` / `20260907T160847-1fd1c8529eb8`；记录器测试 `20260907T155148-57716e829914`；协议smoke `20260907T161020-76efaf552ea4`；顺序验证 `20260907T161105-4a80fb4fad97`。

全曲证据目录 `test-artifacts/roformer-b580-xe90-padded-c-once-20260907/`，独立保存当前worker、`build-and-inputs.json`、请求与committed/dirty差异。运行前CPU约19.56%（其他CPU/编译任务）、B580约2%、47°C、40W，AMD可见桌面引擎约1.34%；前检 `20260907T161107-9d13a661b4af` / `20260907T161108-fce1595a1246`。不终止用户任务，不把此次视为隔离测速。

上面的前检是准备期记录；最终启动前另采CPU13.63%、B5802%/47°C/39W、AMD桌面gfx1.40%，见 `20260907T161413-193c5c9dcf97` / `20260907T161415-3687b2fe1b22`。

## 实际执行：再次黑屏，日志有效

- 执行操作 **`20260907T161443-83ff8c033621`**，源码/启动HEAD `2037cc1`（构建代码 `1ab2f47` 加继承dirty输入），worker PID **585556**，实际启动 **16:14:45.493720 UTC**。用户随后报告“好 黑屏了 分析log来修复”。这是已启动并执行GPU的事故，不是未启动。
- 原boot `17f6b158-8dbc-4a53-be30-2604edc93846`，复核boot `69cbc1cc-f99c-4022-a74b-306e761dea36`。无observer result、无输出文件。首chunk完成尚未记录。
- stderr **1,004,911 bytes**，**5749条有效诊断**，NUL/损坏JSON/ID配对错误/错误事件均0；没有记录日志同步失败警告。唯一未配对操作是整个task。
- 前9组time/freq子块的所有已标记resident阶段完成；第10组time QKV也完成。最后事件 **16:14:57.382489 UTC，ID2855 `roformer.time.qkv / resident.stage / complete`**。其前的raw C创建/分配/绑定、submit、wait、import、最终wait都有complete，**没有卡在begin的GPU wait**。
- 下一段batch创建、gate编码及normed释放当时没有细分阶段日志。不能把最后QKV complete说成“QKV正在执行时崩溃”，也不能把缺少后续事件当作所有未记录代码都未执行的证明；记录写出与落盘自身同样存在时间区间。此前padded-C修复确认减少冗余，但没有解决本次整机失败。
- **57个进程样本、12个主机样本**。部分首chunk期间目标峰值VRAM **4,275,668 KiB / 4.08 GiB**、RSS **898,224 KiB / 877.17 MiB**，CPU12.84–18.28%。这不是同阶段完整chunk内存对照，不证明OOM/泄漏或其排除。最后进程样本16:14:57.328860，主线程D/`jbd2_log_wait_commit`，只能说明该采样点在日志同步等待。
- 复核 `20260907T161748-7e7c1c8dae09`，被动内核采集 `20260907T161748-ca9e54def419`；先前boot journal不可用，`/sys/fs/pstore`不存在。没有可归因的driver故障报告。标准化KiB/MiB/GiB重解析及最终 `incident.json`：`20260907T163833-77965d63290e`。原始日志与旧worker不改动。

## 后续候选：跟踪缓冲的串行coopmat

用户先授权分析/修复，随后明确追加“打好补丁后继续GPU全曲测试”。下一次仍只执行同song/model/control的一次全曲，不加GPU探针/预热/自动重试。

- **`a2fd165`**：主干原N%256 raw GEMM在现有tracked能力可用时优先使用WGPU拥有的普通缓冲、已有SPIR-V coopmat compute pass和buffer pool，避开这条主干的手工VkDeviceMemory分配、descriptor与buffer导入。执行与输出返池用同一个路由选择；无tracked能力时原raw/WGSL可用性保留，无执行失败后的自动替代后端。
- 新增 `linear_coopmat_pooled_serial`，pack后submit/wait、GEMM后submit/wait、bias/最终段由原调用者finish后再返回；scratch在final finish后才返池。原384宽度及Mask的同batch调度不变。新路径不再需要raw分配的预清零提交/导入；不宣称每个内部调用都与旧实现一模一样。
- F16 operands/F32 accumulation、源F32权重、音频、attention全序列及已有group提交不变。但使用**已有64×128 tracked tile替代64×256 raw tile**，资源归属、复用和shader工作分组发生变化；不能事先宣称数值bit-exact、显存下降或黑屏已修复。池保留大张量以复用，需观察实际内存。
- **`e3f1d36`**：独立补齐 `gpu.batch.begin`、`gate.encode`、`normed.release`、`buffer.allocate`、`pipeline.pooled`，沿用先同步begin再执行的参数诊断。无新异步日志、模型gate或重试。
- **`53ac4ad`**：复核发现原 `supports_pooled_coopmat`只看feature，pipeline本来已要求Vulkan1.3/SPIR-V1.6。能力选择现匹配既有完整feature/API条件，避免优先路由让仍能运行raw的Vulkan1.2设备新近失败；只读取已创建设备元数据，不执行探针。原pipeline检查保留。
- **`8d39bcd`**：CPU测试中的Vulkan1.4常量不在当前ash头文件里，改用同值 `make_api_version(0,1,4,0)`。失败编译 `20260907T163630-6ab65968cb09`保留，不冒称该步测试通过。
- 修正后 **74项CPU测试通过（worker26/runtime48），19项GPU测试ignored**；含路由/归属、旧384批处理及pipeline能力匹配，新增串行GPU数值/复用用例仅编译未运行。测试 `20260907T163717-075ce2a63cab`；定向all-targets检查 `20260907T163720-bba01e9f9f49`、Release构建 `20260907T163722-4c6d2b9e1a20`均成功。

新证据目录 **`test-artifacts/roformer-b580-xe90-tracked-serial-once-20260907/`**，已保存源码身份 `8d39bcd`、独立worker、输入元数据、请求及committed/dirty差异。准备期完成后实际运行如下。仍无确认根因，不调整生产route/readiness；GGML受支持的串行配置仍是用户反复验证稳定、且已完成38/38的参照。

## tracked串行候选实际结果：38/38，退出0

- 唯一执行 **`20260907T164322-10f8d19efd7c`**，worker **PID25486**，启动HEAD `aacbd33`（构建代码 `8d39bcd`及已保存dirty差异）。**16:43:23.856470 UTC**启动，**16:53:45.888705 UTC**观察器结果。目标墙钟 **621.995629秒**，外层记录器623.507413秒，38个chunk计时合计619.690472秒。原模型/输入/控制、Intel ICD、`--diagnostics`与profile参数保留。
- **38/38，worker done=ok，退出0**，boot前后均 `69cbc1cc-f99c-4022-a74b-306e761dea36`。未追加预热/探针/GPU单测/重试。此次授权已执行完，不再自动启动模型。
- 完整日志 **273010条事件**，全部begin/end配对，未配对/错误/损坏JSON/NUL均0，**raw阶段事件0**，tracked pipeline实际初始化完成。不能仅凭代码选择宣称已走tracked；这里有运行记录。
- **38次池快照完全一致：26 buckets、152 buffers、5,585,728,176 bytes / 5327.0 MiB**。目标采样VRAM峰值 **5,997,956 KiB / 5.72 GiB**，目标树RSS峰值1,198,320 KiB；大张量池保留导致显存高于部分旧失败区间，不能宣称此次靠降低峰值显存修复。
- 2893次进程、583次主机采样。211个读取错误均ENOENT（206个fdinfo、5个其他路径消失），采样不完整；observer errors为空。CPU总忙碌 **2.15–100%**，不是隔离测速。GGML同曲观察值402.623654秒仍更短；额外同步日志I/O、背景负载、shader tile与资源归属同时不同，不能据此拆分性能收益或证明哪个变化消除了事故。
- 输出 **`output/guide-vocals.flac`**：真实FLAC/24-bit、44.1kHz双声道、15,650,208 frames / **354.880秒**。编码前profile：31,300,416 samples、**non_finite=0**、peak1.1223633、RMS0.1847233758。输出解码完成，无损坏/截断。
- 既有整数FLAC转换有 **468个满幅样本**；GGML F32参考也有468个样本超过FLAC24范围。对同长度、同位置且裁到FLAC24范围的GGML参考：RMSE **0.00017901154**、最大绝对差0.0062219501、相关系数 **0.9999995307**、信号/误差60.27dB。未裁参考RMSE0.00022075759。这不是原始Rust浮点bit-exact证明，不加入数值阈值gate，也不在此改动继承的音频编码文件。
- 输出probe `20260907T165607-05f9c15fe1c2`；输出/参考只读解码 `20260907T165611-e8c2d92b827e` / `20260907T165613-372e97108c0b`；指标 `20260907T165825-e99db3ab1c9a`；最终 `summary.json`/配对与内存重解析 `20260907T170052-0052be3670eb`。
- **退出后428秒（约7分8秒）**仍是同boot，主机可响应。内核复核 `20260907T170054-596378941d87`的本次运行至复核窗口无GPU hang/reset；但 **17:00:10 UTC有 `FAT-fs (nvme2n1p1): Volume was not properly unmounted` 警告**，所以不能说内核日志为空。其与先前断电/其他操作的关系未核实，未执行fsck、卸载或修改磁盘。后检nvtop `20260907T170054-bf8f0fefb2ff`：B5803%/49°C/47W、整卡1.41GiB。

**结论：这一候选完成了一次真实B580全曲并观察到退出后主机响应，未复现之前的整机失败；不是确认唯一根因或普遍生产稳定性的证明。** 保留 `c04fa18`已确认的冗余副本修复和现tracked候选，不重新跑已知失败raw路径来追求归因；后续实验范围待用户新授权。源码identity扫描通过；全仓扫描仍被保留的历史raw GGUF元数据命中，证据不改写。
