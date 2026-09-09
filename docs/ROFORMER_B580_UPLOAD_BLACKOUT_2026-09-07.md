# Uta! Studio — 微型上传用例后的整机断电记录

记录日期：2026-09-07，Asia/Tokyo。

**稳定性结果：失败，确切触发步骤不确定。** 用户先报告：“微型用例后出现；整机断电且需拔 AC 瞬间断电了所以你没有log”，随后指出也可能是微型用例已通过、下一步刚执行就断电。两次说明均保留，不能把微型用例认定为确定触发点。此次故障时点没有日志；不得以测试进程退出成功或本地日志缺失否定故障，也不要求再触发断电来补日志。

本助手可见的完整 GPU 执行记录只有一次微型 F16 上传／回读。优化后的 Denoise 请求有准备文件，未见可核实的启动／结果记录。不能仅凭缺少记录排除某个后续步骤刚启动便中断，也不能将其写成已证实的 Denoise、完整 FA 或全曲失败。下文调用链描述的是**最后一个有完整记录的测试**，不等同于已确定的断电调用链。

## 证据与时间线

证据目录：`test-artifacts/roformer-upload-blackout-20260907T193517/`。

- `incident.json`：用户报告、重启检查、执行结果、源码时间及后台负载事实。
- `source-path-review.json`：独立只读复核的完整调用链、代码位置和推断边界。
- `command.json`、`stdout.txt`、`stderr.txt`、`result.json`：实际执行记录。
- `source/`、`source-changes.patch`、`executed-wgpu-test`：当前改动的源码快照、相对本轮开始工作树的差异、实际测试可执行文件。未添加 hash 校验或冻结验收值。
- `host-before-denoise.txt`、`background-build-start.txt`、`host-before-model-run.txt`：同期 CPU 编译与桌面负载，不能假设独占主机。

| 时点 | 实际观察 |
| --- | --- |
| 19:24:53–19:24:59 | 四款模型的 CPU 权重加载对照完成；这不是 GPU 执行。 |
| 19:25:04 | 另一个 Nix 任务开始构建 Linux 7.2.3，之后观察到 `make -j15`。 |
| 19:28:28 | 微型上传测试结果写出：1 passed，退出0，进程墙钟0.235 s；当时 boot ID 为 `5fe2e889-c892-411a-9765-7e49deaf46ea`。 |
| 19:29:11 | 保存的主机进程快照显示多路 C 编译仍在进行；未开始模型计时。 |
| 19:30:45 | 编译高负载已消退；同次工具读取仍返回上述 boot ID。该观察不能确定之后断电的具体时点。 |
| 用户随后报告 | 瞬间整机断电，需要拔 AC 恢复；后来进一步质疑是否下一步启动才触发。 |
| 19:35:17 | 新 boot ID 为 `a4acf780-23bb-453d-9a68-5bf560b86a55`；运行／启动 kernel 仍为修补后的 `kgr3m4y9…-linux-7.2.3/bzImage`。没有运行中的 RoFormer／WGPU 测试进程。 |

这次只读内核日志命令另因文件权限失败；这是取证可用性，不是内核日志正常的证明。正在编译另一内核不等于正在运行它，当前实际启动路径已单独核对。

测试源码在19:28:44仅做 rustfmt 排版，保存的源码与执行时语义相同。原始结果仍保留数值通过；**后续整机故障覆盖了稳定性结论**。

## 本次执行了什么代码

实际测试名：`gpu::tests::f16_upload_keeps_odd_padding_and_empty_storage`，单独使用 `--exact`、`--ignored`、`--test-threads=1` 执行。入口见 [上传回归用例](../native-inference/wgpu-runtime/src/gpu/tests.rs)。

三个参数均保留：`batch_size=1`、`vulkan_no_async=true`、`serial_pipeline=true`。这些参数不消除驱动、设备初始化或退出清理风险。

1. [GpuDevice::new](../native-inference/wgpu-runtime/src/gpu.rs) 创建 Vulkan instance、枚举离散 GPU、请求 device／queue，安装错误回调。
2. [Pipelines::new](../native-inference/wgpu-runtime/src/gpu/pipelines.rs) 预先创建普通 WGSL shader modules、pipeline layouts 与 compute pipelines，然后 poll。**没有 dispatch 这些计算管线，但此步骤会调用驱动，不能称为“没有 GPU 设备工作”。**
3. 五个 F16 fixture 的逻辑长度为0、1、7、2、258，目的 buffer 物理尺寸分别为4、4、16、4、516字节。三个使用原 mapped-init，两个使用本轮新增的 queue 写入。
4. 测试先创建全部五个上传结果，再开始回读；首次 download 的既有 submit 会提交全部 pending uploads 及该次回读复制。
5. 共执行五次 `download`：创建读回 staging，编码 `copy_buffer_to_buffer`，submit 后 wait，`map_async` 后再 wait，逐位比较后 unmap、释放相应对象。
6. 最终进入 `DeviceInner::drop`：额外 poll，其 Result 在原有实现中被忽略；随后 `device.destroy()`，再释放其余句柄和进程资源。用户态退出成功不能覆盖后续 Xe 队列、BO、VM 等驱动清理的实际状态；是否与此次故障有关尚未确认。

此用例**没有模型权重加载、RoFormer图计算、attention／FA dispatch、raw 或 pooled cooperative-matrix 构造／计算**。本轮 CPU 并行解码改动不在这个 WGPU crate 测试的调用链中。

544字节只是五个目的 buffer 的尺寸总和；不是设备总内存。记录器采样的 `drm-resident-vram0` 峰值为219756 KiB（约214.605 MiB），包含没有进一步归因的设备／驱动资源，也不是高频测得的真实峰值。

## 与此前代码相比，具体改变了什么

本轮 GPU 改动位于 [upload_f16_bits](../native-inference/wgpu-runtime/src/gpu.rs)。对小端机器的非空、偶数长度输入：

```rust
// 原路径：create_buffer_init 内部 mapped_at_creation=true，复制后 unmap。
device.create_buffer_init(&BufferInitDescriptor { contents, usage, label });

// 本轮新路径：完整、四字节对齐的 queue 写入。
let raw = device.create_buffer(&BufferDescriptor {
    mapped_at_creation: false,
    size: contents.len() as u64,
    usage,
    label,
});
queue.write_buffer(&raw, 0, contents);
```

上面仅展示 API 差异；完整实现保留原尺寸验证和错误检查，见源码快照。奇数、空输入、大端机器保持原路径；raw Vulkan CoopmatBuffer 路径没有修改。没有减少 wait、改变模型 chunk／overlap、放大计算 dispatch 或删除保护。

对锁定的 wgpu 29.0.4 源码检查表明：两种路径都将复制放入 pending writes，在下一次既有 submit 前执行，staging 由该提交完成后释放。新路径覆盖全部目的字节，且满足四字节对齐。**这些是 API／源码层面的审查结果，不是此次设备行为安全的证明。**

测试也有一个独立变化：旧测试逐个“上传→回读”，本次改为“五个上传先排队→逐个回读”，用以检查多个待提交复制的数据隔离。此次同时改变上传实现和微型测试的排队顺序，不能用一次故障将两者区分。

## 值得后续关联的代码形态

| 候选路径 | 本次为何相关 | 目前能下的结论 |
| --- | --- | --- |
| 非 mapped 创建后 `queue.write_buffer` 全量初始化 | 两个 fixture 确实执行新路径；分配、初始化追踪、复制编码方式不同 | 可以作为差异点比对，未证明存在越界、未初始化读或其本身导致断电。 |
| 多个上传及 mapped-init／queue 写入混合进入 pending writes | 第一次 submit 包含五个目的 buffer 的上传复制 | 本次实际覆盖这种顺序；源码生命周期检查和回读通过，不能保证驱动执行及清理无缺陷。 |
| Vulkan device 与普通计算管线的创建／销毁 | 微型测试即使不 dispatch 模型，也执行了这些驱动调用并分配设备资源 | “没有重计算”不能排除这类路径；微型记录没有 FA，但不能据此补齐缺失的故障时点记录。 |
| copy、map/unmap、wait 及 buffer 回收 | 五次 readback 实际走过这些路径 | 数值通过说明采样阶段复制正确，不能覆盖之后的内存释放或驱动后台行为。 |
| device／进程退出后的驱动清理 | 进程已退出，之后用户报告整机断电；更早故障也有退出边界未覆盖的问题 | 值得保留时间关系；没有故障时点日志，不能声称已经证实延迟清理是触发源。 |
| 同期 CPU 内核构建和日常桌面负载 | 微型测试与另一任务的高并行编译重叠，且在双屏日常主机执行 | 是混杂因素，应随证据保存；不据此推断额定电源不足或某个硬件损坏。 |

这些是未来审查时可以交叉匹配的路径，**不是新的根因认定**。上游问题仍是待查方向，尚无证据证明不可解决。源码审查未发现局部违规与实际整机失败必须同时保留。

## 当前优化与验证状态

- CPU并行F16解码的全位模式测试通过；真实Denoise／Dereverb／Harmony权重加载在旧／新／新／旧顺序中由约0.52 s降到0.18–0.19 s。对照的最后一次旧加载约0.48–0.49 s；Leap F32热缓存新旧都约0.167 s，未证明加速。
- RoFormer CPU测试16 passed；共享WGPU CPU测试31 passed。全部65536种半精度位模式、非对齐输入、并行尾块和原数值语义保留。
- 新GPU上传路径只有微型数值通过，随后出现本次整机失败；**尚未获得模型级性能或稳定性证据**。优化后的Denoise没有可核实的执行结果。
- 相关源码及二进制已保存，没有自动回滚或加新 gate。产品路由未晋级。后续GPU执行已停止；现有代码路径和CPU结果可继续用于只读审查。

相关记录：[重启后交接](ROFORMER_B580_REBOOT_HANDOFF_2026-09-07.md)、[此前GGML断电与迁移复核](ROFORMER_RUST_B580_REVIEW_2026-09-07.md)、[任务索引](../tasks/remaining-models/STATE.md)。

后续按用户要求执行[逐项提交和执行前持久记录](ROFORMER_OPERATION_RECORDING.md)，不为这次缺少的步骤或历史源码事后伪造 commit／启动日志。

## 后续候选处理与 CPU 接续

2026-09-07，`8bd5b82` 保存当前 WGPU 源码后，`d24c282` 单独撤下尚未取得稳定性证据的偶数长度 F16 队列上传分支，沿用既有 mapped 初始化实现。事故时源码、实际二进制和本节以上的执行路径记录保持可追溯；撤回候选不证明它是根因或旧路径安全。随后 `38120af` 的权重布局优化只执行 CPU 数值和性能校验，未再创建 GPU 设备。详见 [CPU 接续结果](ROFORMER_RUST_B580_REVIEW_2026-09-07.md)。
