# Uta! Studio — B580 修复切换后验证交接

**后续状态（2026-09-07 20:20 JST）：** 修补内核后的整机断电问题仍未解决。本轮已完成不创建设备的 CPU 权重布局优化、四模型逐位校验与测量；三个 F16 模型 CPU 加载＋布局减少13%–19%、峰值 RSS 减少约33%，Leap 主要减少内存。未验证的队列上传候选已在 `d24c282` 撤下，不能据此认定根因或稳定性。最新源码提交、测量边界及下一步以 [当前审查的 CPU 章节](ROFORMER_RUST_B580_REVIEW_2026-09-07.md) 为准；本交接下方的重启和短用例步骤保留历史范围。

## 最新更新：微型上传用例后仍整机断电（2026-09-07）

用户报告瞬间整机断电、必须拔 AC 恢复，随后指出也可能是微型用例已通过、下一步启动才断电；确切触发步骤未确认，故障时点没有日志。19:28:28 用例数值通过且退出0，随后19:35:17只读检查取得新 boot ID `a4acf780-23bb-453d-9a68-5bf560b86a55`（此前 `5fe2e889-c892-411a-9765-7e49deaf46ea`），实际仍启动同一修补内核。**稳定性再次失败，不能用微型回读成功评为安全。**

最后一个有完整记录的微型用例涉及 Vulkan 设备／普通 pipeline 创建、三个旧 mapped-init 与两个新 queue 写入、五次复制回读及销毁，没有计算 dispatch、FA、coopmat 或模型推理。五个目的 buffer 合计544字节，但采样显存约214.605 MiB；小数据量不代表没有设备／驱动工作。新上传路径与“先排队五个上传再回读”的测试顺序都是差异点；创建、复制同步、map/unmap及退出后的驱动清理也须记录，尚不能认定某个API就是根因。同期另一任务的 `make -j15` 内核编译是已保存的混杂因素。

优化后的 Denoise 只有准备请求，未见可核实的启动／结果记录，不能仅凭缺失排除后续步骤中断。CPU并行解码的数值及真实权重加载结果保留，GPU上传优化未获模型性能／稳定性验证。相关源码与实际测试二进制已保存，后续GPU执行已停止；没有自动回滚、晋级路由或新增限制机制。

详见 [本次代码路径与故障记录](ROFORMER_B580_UPLOAD_BLACKOUT_2026-09-07.md)；证据目录 `test-artifacts/roformer-upload-blackout-20260907T193517/`。

记录时间：2026-09-07，Asia/Tokyo。此文件供用户手动执行 `nrs`、重启后，在新的 Codex 窗口恢复工作。

**原始交接时的结论（下方重启后接续更新其状态）：驱动修复已写入系统 flake，目标内核和两个 NEO 包已编译；尚未切换或启动新系统。整机断电根因未确认，安全目标未完成；Rust 整模型推理仍未持平 GGML。最近一次断电后没有恢复 GPU 测试。**

## 最新故障更新：修补内核下仍发生整机断电（2026-09-07）

用户明确报告：**手动运行 GGML 后再次整机断电，仍需拔 AC 才能恢复开机**。随后核对发现，`test-artifacts/roformer-ggml-fullsong-defaults-20260907T190724/diagnostic.log` 已记录实际 GGML 执行：Leap F32、354.88 s／44.1 kHz 双声道输入、B580／Vulkan0、batch=1、serial_pipeline=false、`GGML_VK_DISABLE_ASYNC=<unset>`、chunk881559／overlap2，共38个 chunk。按用户说明将本次执行归属为手动运行；此前“只有准备、模型／输入／进度均未记录”的表述已过时。

日志记录 chunk1 计算成功、下载和后处理完成，chunk2 开始计算；最后一条是19:07:47.130 JST 的 chunk1 后处理结束。`command.json`、请求和启动脚本等旁路文件当前均为0字节，因此精确 argv 仍无法恢复。没有完整运行结果；日志截断不能证明断电恰好由 chunk2 或最后一条日志所对应的调用触发。

19:12:47（Asia/Tokyo）只读检查取得新 boot ID `5fe2e889-c892-411a-9765-7e49deaf46ea`，此前两次 Denoise 为 `2880e489-0ed9-4add-a5fb-e9668af1d3b6`；运行与启动 kernel 仍为修补后的 `kgr3m4y9…-linux-7.2.3/bzImage`。当前未发现 RoFormer runtime／worker 进程。本次内核日志因文件权限不可读，未据此声称日志正常，也不要求复现故障来补日志。

**实际稳定性验证失败：这轮内核／驱动修复没有解决所报告的断电故障，根因仍未确认。** 既有 GGTT、PCODE、probe 和 NEO 源码缺陷的审计／修复事实保留，但不能据此声称消除了本机故障，也不据此次失败自动回滚这些修复。先前两次短 Denoise 的数值与耗时仍有效，首次桌面正常反馈和有限退出观察不覆盖后续断电。故障后未自动重试 GGML 全曲，产品路由未改。

**最新接续授权：** 用户随后明确要求继续优化 RoFormer 系列的 Rust 性能，同时关注稳定性与代码安全边界。本轮从 CPU 实现、准备成本测量和相关测试继续推进，保留既有提交等待、资源生命周期、错误传播及产品串行参数；CPU 结果不能证明 GPU 端到端加速或整机稳定。此前故障后的临时暂停不冻结性能优化工作；本轮未重新启动 GPU，也不把上游问题视为已经确认的根因。

证据：`test-artifacts/roformer-b580-incident-20260907T191247/incident.json` 保留首次只读检查；新增 `evidence-review.json` 纠正其当时对命令／进度未记录的解释，并指向实际日志。

## 2026-09-07 重启后接续（两次短用例已完成）

用户已完成 `nrs` 并重启。新证据目录为 `test-artifacts/roformer-b580-postboot-20260907T185920/`。
实际 boot ID 为 `2880e489-0ed9-4add-a5fb-e9668af1d3b6`；`current-system` 与 `booted-system` 均为 `y163a1h9…`，两者 kernel 均指向已修补的 `kgr3m4y9…-linux-7.2.3/bzImage`。
Level Zero 与 OpenCL 链接分别指向记录中的新优化 NEO drivers／out；Intel Vulkan ICD 指向 Mesa 26.2.2。B580 仍为 PCI `0000:07:00.0`、`8086:e20b`、`renderD128`。
当前源码定向 Release 构建通过（Cargo 10.75 s）。随后逐项运行两次 Denoise 1.2 s，沿用同一模型／输入、chunk352800／overlap4、batch=1／no_async／serial_pipeline。两次均正常完成，墙钟 **3.565／3.143 s**，加载 **0.542／0.452 s**，chunk **2.275／2.256 s**。两次105840个解码音频值完全一致，对已有 GGML 参考 SNR **61.259 dB**、最大绝对误差0.0002071。进程记录确认 B580 与实际 Mesa ANV 库；枚举时也加载了其他 ICD，不能将库列表当成多设备推理。

用户在首次结束后确认两屏和交互正常并授权继续。第二次结果后45.6 s 的只读检查中 boot ID 未变，测试起点以来可读内核日志无条目；尚无第二次后的独立人类桌面反馈。这只覆盖两次短用例及有限观察窗口，不撤销历史整机断电事实，也不是稳定性保证。仍慢于历史 GGML 2.089 s；跨重启及缓存／主机条件变化，不能单独归因于驱动或某项优化。

分层计时指向后续准备／上传优化：第二次 mask 主机670.820 ms，其中 F16打包54.764 ms、F16上传328.103 ms、F32上传87.971 ms；首次 mask GPU 批次合计72.747 ms。原始逐批次时间及两次汇总在 `measurement-summary.json`。下一步优先审查这些准备／上传成本和未计入chunk的设备初始化成本，保留既有同步和池生命周期；未自动运行更大模型、全曲或完整 FA。产品路由和 READY 状态未改。



**2026-09-07 单次例外授权与结果：** 用户明确要求直接 GGML、不传三个安全参数、跑一个全曲，随后说明已手动运行并发生整机断电／需拔 AC。保留日志确认 Leap F32／354.88 s 输入／chunk881559／overlap2 已在 B580 上开始执行，batch=1、serial_pipeline=false、禁用异步环境变量未设置；精确 argv 未落盘，不能据此补写命令。证据目录 `test-artifacts/roformer-ggml-fullsong-defaults-20260907T190724/`。本次没有完整输出和成功耗时，稳定性结果为失败；单次实验授权不更改产品默认，也不自动重试。

## 给新窗口的启动提示

可直接粘贴以下内容，并补充实际是否已重启：

> 请读取 `/home/bintis/Code/uta-studio/AGENTS.md` 和 `/home/bintis/Code/uta-studio/docs/ROFORMER_B580_REBOOT_HANDOFF_2026-09-07.md`，继续 RoFormer 的 Rust/WGPU 迁移与 B580 修复验证。我已手动执行 nrs；请先核对现在实际启动的内核、系统和 NEO，再按已授权范围推进。保留本文中的故障事实、既有修复和未完成目标，不要从头重复排查或自动重放旧 GPU 压测集合。

## 1. 必须保留的用户事实与授权

- 日常双屏机器：Intel Arc B580 连接一个 4K 显示器，AMD 8700G 核显连接另一个；主板 ASUS TUF GAMING B650M-PLUS。已识别 B580 为 PCI `0000:07:00.0`、`8086:e20b`。此前 renderD128 是 B580、renderD129 是 AMD；重启后须重新识别，编号不是设备身份。
- 故障是**整机突然断电**，不是仅窗口黑屏。之后必须拔掉 AC 电源线等 **20 秒以上**，否则整机拒绝开机。
- 用户明确说明：**完全未修改的内核／驱动也发生同样故障，Intel OpenVINO 也发生过。** 自定义 GGTT 补丁不是历史故障的必要条件。
- 电源为 **1200 W，显卡双 8PIN 供电**。不要无证据重复归因为额定功率不足。需要断电复位提示保护锁存或主板／显卡／固件状态，但目前不能确定部件或触发源，也未排除软件负载间接触发。
- 用户已反复尝试留日志，掉电时日志会直接截断。不要要求再冒掉电风险，只为获取同一种本地日志。
- 用户先前授权继续验证，随后授权把修复直接写入系统 flake；本次决定**自行运行 `nrs` 和重启**。本交接窗口只整理文档，不执行切换、重启或 GPU 负载。下个窗口沿用已有授权范围，先确认实际系统状态；本文不是批量重放历史 GPU 用例的指令。
- 目标继续保持：保留 batch=1、no_async、serial_pipeline 及既有安全措施，修复具体缺陷，在系统稳定性有实际依据的前提下，使 Rust 整模型推理持平或超过 GGML。不通过缩减音频上下文、改 chunk／overlap 或降低比较标准冒充加速。

## 2. 从哪里接着看

| 内容 | 路径 |
| --- | --- |
| 工作仓库 | `/home/bintis/Code/uta-studio` |
| 当前任务状态 | [tasks/remaining-models/STATE.md](../tasks/remaining-models/STATE.md)，`Native Rust WGPU follow-up` |
| 当前结论 | [KEY_CONCLUSIONS.md](KEY_CONCLUSIONS.md)，RoFormer 安全／迁移条目 |
| 源码、精度与性能详细复核 | [ROFORMER_RUST_B580_REVIEW_2026-09-07.md](ROFORMER_RUST_B580_REVIEW_2026-09-07.md)，先读顶部“实施复核”，下面编号章节含更早历史 |
| 驱动修复总说明 | [driver-fixes/README.md](../native-inference/gpu-probes/driver-fixes/README.md) |
| PCODE 触发条件与 CPU 测试 | [README-xe-hwmon-pcode.md](../native-inference/gpu-probes/driver-fixes/README-xe-hwmon-pcode.md) |
| NEO 官方回移与验证边界 | [README-neo-reused-private-residency.md](../native-inference/gpu-probes/driver-fixes/README-neo-reused-private-residency.md) |
| 本轮持久证据目录 | `test-artifacts/roformer-b580-followup-20260907T124128/`，下文简称“证据目录” |

两个仓库都有大量未提交的用户改动；Uta! Studio 的 `roformer-native/`、`gpu-probes/` 等新目录也未提交。先读 `git status --short --branch`，不要 reset、clean、按 HEAD 恢复文件、删除证据或擅自 commit。代码状态以工作树为准，不能用 HEAD 或旧二进制路径代替。

系统修复在 `/home/bintis/Code/xirin-nix`；**不是**直接修改 `/home/bintis/Code/Xiryn-nixos-kernel`。保留既有 `xiryn-kernel` flake 输入及锁文件。xirin-nix 中原先的 `xpu-smi` 删除和其他已暂存修改不是本轮引入，不能当作本轮修复回滚。

## 3. 已经写入系统的四个文件

| 实际文件（相对 xirin-nix） | 本轮改动及范围 |
| --- | --- |
| `hosts/btspc01h/kernel-local.nix` | 从既有优化内核包中过滤掉 `xe-ggtt-bulk-clear`、`xe-exclusive-async-probe`；保留 builtin display、probe milestones 等其他补丁。普通 6.18.49 和优化 7.2.3 内核各追加一次 PCODE 补丁。固件生成引用最终修补后的优化内核。 |
| `hosts/btspc01h/xe-hwmon-pcode-read-error.patch` | PCODE 功率读取失败立即返回原 errno，不继续写回初始零值；平均窗口设置传播错误，保留两版各自的锁及 PM 引用释放。 |
| `modules/system/gpu.nix` | 普通与诊断 NEO 共用官方驻留修复；`XE-Optimismed` 保留原 `RELEASE_WITH_REGKEYS` 构建及原环境变量。 |
| `modules/system/patches/neo-reused-private-residency.patch` | Intel 官方提交 `3d7a21dca960c81ead70412d6b8cbacd290a3905` 的回移，包含生产修正和四个上游测试。 |

新补丁文件已用 `git add -N` 纳入本地 Git flake 的源集合，没有暂存其文件内容。精确本轮差异在证据目录 `system-flake-driver-fixes.patch`，其起点是本轮开始前工作树，**不是整个仓库 HEAD**。

本轮保留既有功耗、频率、显示、内核参数、NEO 调试变量及缓存策略，没有新增功率限制。普通和诊断 NEO 的 flags 仅相差原有 `RELEASE_WITH_REGKEYS=TRUE`；既有 `NEO_DISABLE_MITIGATIONS=TRUE` 未在本轮改动。

修复依据及不能越过的结论：

- GGTT：原自定义 bulk 路径使用两次32位 CPU 写入写一个64位 PTE，却只计一次；破坏 BMG 每1100次写入刷新的对应关系。上游64位 `writeq` 已恢复。队列／LRC／BO 退出清理同样可达此路径，进程退出成功不覆盖稍后的异步清理。
- exclusive probe：自定义异步域脱离全局等待，注册失败清理／卸载可能漏等尚在排队的 probe；恢复上游等待可能增加启动耗时。本机是否触发过该条件未知。
- PCODE：原版驱动读失败仍写，可能请求清掉另一功率限制的 enable 位。需要“读失败、后续写成功、固件接受且无后续恢复”等条件才能造成实际设置丢失；本机未证实。初始化调用层仍存在忽略返回值的行为，补丁不是完整初始化状态重构。
- NEO：复用私有分配后遗漏驻留声明。修复只覆盖 NEO 的 Level Zero 路径，**Mesa ANV／Vulkan 不执行该代码**；不能用它单独解释 WGPU 故障。

## 4. 交接时已经执行的验证

| 验证 | 实际结果 | 证据目录中的文件 |
| --- | --- | --- |
| 最终正常 Git flake 与系统派生求值 | 通过；两个内核各带一次 PCODE 修复，优化固件依赖新内核，NEO 两包进入系统依赖 | `system-kernel-final-evaluation.json`、`system-flake-toplevel-final.drv` |
| 最终优化 Linux 7.2.3 | out／modules／dev 三个输出编译成功 | `system-kernel-build.json` |
| 普通、诊断 NEO 26.31.39395.13 | 两包各自 out／drivers 输出编译成功 | `system-neo-build.json` |
| 两版 PCODE 真实 C 函数 | 6.18.49／7.2.3 原版均复现错误写入及吞错；补丁后错误传播、位值保留、锁／PM 释放验证均通过 | `xe-hwmon-pcode-cpu-validation.json` |
| 新内核机器码 | 64位 GGTT store、BMG 1100次刷新、读错误绕过写命令、interval 错误先释放锁及 PM 再返回，均实际复核 | `kernel-final-symbols.txt`、`kernel-final-*.asm` |
| Rust RoFormer CPU 测试 | 16 passed，3 个 GPU 测试 ignored；包含新权重与混合精度参考 | 详细复核文档中的最新 CPU 验证记录 |
| CPU 真实模型权重加载 | 新旧各两次，均成功；见下一节 | `cpu-weight-loading-comparison.json` |

总汇为 `system-driver-final-verification.json`。普通 **6.18.49 没有在本轮构建整份内核**，只做了补丁应用和实际函数的 CPU 编译执行。NEO 保留既有 `SKIP_UNIT_TESTS=TRUE`，**四个上游单元测试没有执行**。整套系统只求值、未完整构建或激活；`nrs` 仍可能构建其他系统组件。

`system-flake-driver-evaluation.json` 是较早 GGTT／NEO 阶段的接线证据，其中内核尚未加 PCODE；最终内核以 `system-kernel-final-evaluation.json` 为准。`system-driver-build.json` 来自主动中断的旧组合构建，不代表最终失败；原因在 `system-driver-build-interrupted.json`，最终成功分别记录在 kernel／NEO build JSON。

## 5. 手动 nrs 后，先确认实际启动了什么

本次已读取真实的 `hosts/btspc01h/home.nix` 和 `scripts/nrs.sh`：`nrs` 先执行现有 `scripts/update-rain.sh`，然后执行 `sudo nixos-rebuild switch --specialisation XE-Optimismed --flake /home/bintis/Code/xirin-nix#btspc01h`。前一个步骤失败时还没有走到系统切换，不应把它判成 Xe 修复构建失败。

**`switch` 不能替换正在运行、内建 Xe 的内核。只有切换成功还不够，须区分 `/run/current-system`、`/run/booted-system` 与重启后的实际状态。新旧优化内核版本号都是 7.2.3，单看 `uname -r` 无法区分。**

| 本次记录的对象 | 路径／值 |
| --- | --- |
| 交接时旧 kernel | `/nix/store/42mji8wm7lcqi6ixhgngzbbcb4day9dq-linux-7.2.3/bzImage` |
| 已构建的新优化 kernel | `/nix/store/kgr3m4y9vc3b4li9783d65mil9618g7c-linux-7.2.3/bzImage` |
| 新 kernel dev／vmlinux | `/nix/store/zvmzihhni593giq5ay1kh3ig99q0y8j8-linux-7.2.3-dev/vmlinux` |
| 新优化 kernel 派生 | `/nix/store/fg26mh8brd58fzvyz2dwc4nxv64yq0z3-linux-7.2.3.drv` |
| 最终系统求值派生 | `/nix/store/0fz7jxnz8g4n8ms7dvyf1v3382sbass4-nixos-system-btspc01h-26.11.20260905.c043004.drv` |
| 新优化 NEO out | `/nix/store/h9kr3pi2j0ykg9d99mwwpf8jkqj976pk-intel-compute-runtime-26.31.39395.13` |
| 新优化 NEO drivers | `/nix/store/izk74hm75fwwphjapma0bfhrx66hz1kr-intel-compute-runtime-26.31.39395.13-drivers` |
| 新普通 NEO out／drivers | 见 `system-neo-build.json` 中普通派生 `1ayb5css…` 对应的两个输出 |
| 最近掉电前 boot ID | `73b4df92-8f9b-44c3-a33c-306819382886` |
| 用户恢复开机后、本交接时 boot ID | `ffefbd5c-fb17-4908-ab72-8bf8f7fb453a` |

这些 store 路径是本轮构建记录，不是冻结的验收值。若用户之后修改 flake、输入或补丁导致路径变化，沿实际派生重新核对补丁；不要仅凭路径不相同就拒绝继续。

新窗口首先在 Uta! Studio 仓库执行以下只读检查；它们不创建设备或提交 GPU 工作：

```sh
git status --short --branch
git -C /home/bintis/Code/xirin-nix status --short --branch
uname -r
cat /proc/version
cat /proc/cmdline
cat /proc/sys/kernel/random/boot_id
readlink -f /run/current-system
readlink -f /run/booted-system
readlink -f /run/current-system/kernel
readlink -f /run/booted-system/kernel
readlink -f /run/opengl-driver
```

再只读解析 `/run/opengl-driver` 下实际存在的 `libze_intel_gpu.so*` 与 `lib/intel-opencl/libigdrcl.so` 链接，核对是否来自新 NEO 输出。若需核对 Vulkan 路径，读取 Intel ICD JSON 及其 `library_path`；不要为了查版本而先启动 `vulkaninfo`、`clinfo`、`sycl-ls` 或推理程序。库链接只能说明配置指向，之后实际测试进程的加载路径还需分别记录。

将这次启动识别的输出写入仓库内新建的、唯一命名的 `test-artifacts/roformer-b580-postboot-<时间>/`，与旧故障记录分开。只保存实际检查结果，不假造已经加载、没有错误或稳定性通过；已有本地日志缺失不是要求复现掉电的理由。

## 6. Rust 代码与性能接续点

当前生产模型路由／READY 状态未改变，RoFormer 仍使用已选定的 GGML 路由。Rust worker 仍是迁移验证对象；不要因本轮编译成功就提升路由。

关键代码在 `native-inference/roformer-native/src/engine.rs`、`src/weights.rs`、`src/engine/resident_tests.rs`，以及 `native-inference/wgpu-runtime/src/gpu/` 中的 pool、batch、coopmat_pooled、flash_attention 等模块。已修复：

- standalone 输出错误回收到无人取用的池，可在 Leap 单 chunk 累积约28.4 GiB；现已修正生命周期。
- raw Vulkan 的 memory-model feature、tile／subgroup 能力、导入前初始化、错误清理；编码失败不会提交部分操作，真实设备错误不被自动 fallback 吞掉。
- pooled cooperative GEMM 覆盖 Denoise 384通道 out／FF2 和 mask；补齐 C 容量直接复用，完整 QKV 少一份 **908.086 MiB** 输出复制；F16 上传去掉中间打包复制，GELU 复用池。
- CPU 权重加载现在用 SIMD F16 解码与互不重叠输出块的并行转置，保留所有65536种半精度位模式的既有 reader 语义。

worker 当前选择的是 **FP32 Q-cache WGSL attention**。F16 K/V attention 与 cooperative FA 候选尚未接入模型。完整形状 cooperative FA 用例45／49为1159.586／1168.890 ms，慢于保留的 FP32 432.246 ms；减少 spills/fills 并未得到性能收益。最近故障记录的最后完整结果是49，但**具体触发调用未确认**，不能推断49是唯一原因。

| 同模型／输入／chunk／overlap 的历史比较 | GGML 进程墙钟 | Rust 进程墙钟 | 边界 |
| --- | ---: | ---: | --- |
| Leap，前6 s，chunk881559／overlap2，用例12／16 | 11.319 s | 15.762 s | Rust 后续优化尚未重测此模型；音频 SNR60.577 dB |
| Denoise，1.2 s，chunk352800／overlap4，用例17／44 | 2.089 s | 3.672 s | Rust 从5.499 s逐步改善；用例44 chunk2.154 s，SNR61.259 dB |
| Dereverb，同1.2 s，用例30／47 | 1.582 s | 4.037 s | 输入极低电平，SNR23.508 dB，不能外推整曲质量 |
| 重启后仅 Denoise CPU 权重加载，用例50–53 | 不涉及 GGML | 旧平均1.044→新0.576 s | 减少44.85%；各两次，存在后台 CPU 编译，不是整模型推理成绩 |

历史354.88 s整曲的同输入 GGML 参考为内部 **368.481 s**、墙钟368.996 s；不是126 s。本轮未重跑整曲，不能混用其他模型／音频的耗时。

权重新代码已通过 CPU 测试，但未进入新的整模型 GPU 测量。`target/release/uta-roformer-worker` 路径本身不能证明包含最新代码；决定后续模型验证后，先定向构建当前源码：

```sh
bash dev.sh --command cargo build -j2 --release -p uta-roformer-worker --features gpu --bin uta-roformer-worker
```

这是 CPU 构建命令，不会启动模型。源码没有再改动时可复用已通过的测试；需要重验时，本次的定向 CPU 测试命令为：

```sh
bash dev.sh --command cargo test -j4 -p uta-roformer-worker --features gpu --lib -- --test-threads=4
```

运行前按当前工作树核对 GPU 用例仍为显式 ignored；不要用 `--ignored` 批量运行整个集合。无需执行整仓库 release 检查、Nix 应用打包或所有模型测试。

## 7. 接下来的验证顺序与证据边界

1. **实际启动身份已经核对，修补内核下仍断电。** 后续按当前工作树和已保存证据接续；若再次切换系统或重启，再核对实际启动身份，不能把其他内核的结果算到本轮修复名下。
2. **核对实际源码和已有证据。** 复用已通过的编译／CPU结果，只因新改动、失败或具体未解决问题扩展检查。所有 Rust／原生构建使用 `bash dev.sh`；网络／Nix daemon 的沙箱拒绝按工具权限流程处理，不把它混作代码失败。
3. **按最新授权继续 Rust 性能实现和 CPU 测量。** 优先消除权重准备、音频前后处理和数据布局中的重复工作，保留实际数值／布局语义与既有安全边界。本轮不重放故障负载；未来单项 GPU 验证沿用户已授权范围具体确定，成功启动、CPU 测试和源码修复都不能作为“不会再断电”的证明。OpenVINO 也没有因此成为已证明安全的替代通道。
4. **恢复后先固定单一变量。** 原子块／池复用测试和 Denoise 单 chunk 已有直接参照，适合作为讨论下一项验证的具体对象；Leap 全形状和 cooperative FA 不应成为自动启动动作。新用例单独命名，保留完整上下文和三个参数，并在结束后观察实际桌面／整机状态；退出码0和有限数值只能证明对应运行阶段。
5. **性能继续分层比较。** 当前优先在同模型、同输入和同布局下量化 CPU 准备／音频处理收益；权重读取／转置／打包与 GPU 上传、host encode／wait、GPU timestamp、chunk／进程墙钟分别记录。整模型未测不得用纯加载或预处理收益替代。编译与计时应分开。

原始请求和输入可从证据目录复用：`denoise-direct-upload-single.ndjson`、`vocals-1.2s.wav`、`input-6.0s.wav`、`bs_roformer_leap_xe90_vocals-single.ndjson`。旧 NDJSON 内含固定 `task_id` 和 `output_dir`，新运行须写新请求／新输出位置，不能直接覆盖旧结果。模型仍位于用户的 `~/.local/share/uta-studio/runtime/`，只读使用，不能删除、替换或清理用户缓存。

`run_case.py` 只记录一个明确选定的命令，不做隐含重试；它的 `command.json` **没有保存 stdin 请求**，因此需要同时保存对应 NDJSON。记录进程的 boot ID／fdinfo 不是防断电机制，也不能弥补掉电时截断的日志。工具脚本是开发诊断，不是产品推理 fallback。

三个参数的精确含义仍是：batch=1 为单音频 chunk；no_async 保留同步提交／等待；serial_pipeline 防止前处理／推理／后处理重叠。GGML 图内仍按节点及 FLOP 切提交，Rust 的单 dispatch invocation 常量不等于整份 command buffer 工作量限制。不要靠削弱现有等待来换吞吐，也不要未经明确授权新增 gate、冻结 baseline 或其他限制。

## 8. 下个窗口不必重新调查的已知边界

- ANV／WGPU／GGML Vulkan 与 NEO／Level Zero 是不同用户态路径，共用 Xe。Level Zero 的 LR VM 超时行为不能直接外推为当前 WGPU 根因。
- 7.2.3 全部538个 Xe C／头文件未见主动整机关机调用；不能据此排除间接固件、MMIO、内存或负载触发。
- `power1_cap=190000000` 表示短时 PL2 阈值190 W，`power1_crit=380000000` 表示 I1 reactive 阈值380 W，均非实际耗电值或温度关机阈值。PCODE 补丁不改这些阈值。
- 固定 GGML 提交 `8c63e70982c95ceb862e3a1073a2c1beef75d60a` 与 B580 能力交叉核对表明四个已测模型走 FA_SCALAR；GEMM 能用 coopmat 不等于 FA 也用。Leap 的 QK／Sf 累加是 F32，三个 legacy 模型为 F16；四者的 Q缓存、概率及 PV／输出累加仍包含 F16，不能笼统称整段 GGML FA 都是 F32。
- 当前 B580 DPAS 的 F16 subnormal **操作数** FTZ 是设备实测，不能当成所有 Intel GPU 的通则。参考已对齐实测行为，未放宽误差容限。
- 所有 `/tmp/uta-*` 源码副本、比较工程、旧 shader 临时文件可能随重启消失。持久源码与补丁在仓库，构建证据在上述 test-artifacts；先检查是否存在，需要时从实际 Nix 派生对应的原版源码恢复，不能依赖已丢失的临时路径或假造先前结果。

每完成一个实际启动核对、修复或测量，就更新本文件的有效状态、`tasks/remaining-models/STATE.md` 和相关结论。始终区分“源码正确性”“编译／CPU 模拟”“GPU数值与耗时”“整机稳定性”，保留未完成项直到有对应证据。
