# Uta! Studio — RoFormer 操作与提交记录

2026-09-07 用户要求：“以后每次操作前都留下log 和对应commit记录 改一个地方commit一下”。此规则用于后续实现、构建、检查和实验，尤其是可能导致整机掉电的原生／GPU 调用。

每个独立代码改动单独提交；运行改动后的程序前先记录对应提交。上传实现、测试排队顺序和算子修改应分别提交，避免一次故障包含多个无法区分的变化。提交只包含已授权的相关变更，保留其他工作树改动。不通过清空工作树、新增 hash、冻结 baseline 或额外 gate 实现记录。

开发记录器是 [tools/record-operation.py](../tools/record-operation.py)，只执行明确给出的命令，不重试、不运行替代后端。Rust／原生操作仍通过 `bash dev.sh`，例如以下纯 CPU 检查：

```sh
python3 tools/record-operation.py recorder-tests -- \
  python3 tools/test_record_operation.py
```

`--stdin-file` 保存实际输入请求的副本；可重复使用 `--input` 和 `--output` 记录文件路径、尺寸和修改时间。参数放在分隔命令的 `--` 之前。默认记录目录为 `test-artifacts/operations/<UTC时间>-<唯一ID>/`，可用 `--evidence-root` 改为明确的证据目录。

2026-09-07 用户追加说明：他同时运行其他测试，性能运行前应先看系统CPU/GPU占用，并可使用已安装的 `nvtop`。后续模型/性能实验先分别记录并人工查看：

```sh
python3 tools/record-operation.py host-before-run -- python3 tools/observe-host-load.py
python3 tools/record-operation.py gpu-before-run -- bash dev.sh -c nvtop --snapshot
```

`observe-host-load.py` 不创建GPU设备、不启动目标进程，只采两次 `/proc`/sysfs：CPU总占用和主要进程、AMD busy快照、可读DRM客户端的引擎差值。xe使用匹配的 `drm-cycles-*` / `drm-total-cycles-*` 差值，不猜GPU频率、不冒充纳秒；缺失/重置/权限不足明确不可用。本机 `nvtop --snapshot` 可提供B580统计但只列该设备，AMD仍需sysfs/DRM补位。没有可见活动不等于系统空闲。

`observe-roformer-run.py` 另在目标启动前、运行中每秒、退出后保存 `host-samples.ndjson`，以免用户的其他测试在预采样之后才开始。预/后采样不计入目标墙钟，但运行中观测会消耗CPU；A/B须使用同一观测配置。发现负载不同应注明结果不可直接归因，不自动终止别人的进程、不轮询等待空闲、不加阈值gate、不自动重试，也不以预检代替实际执行。旧数据没有这些采样时只能保留未知；新工具可明确重解析已有原始采样，不能伪造历史观测。

在2026-09-07候选全曲事故中，启动/进程样本存在，但stdout/stderr为零字节；不能据此说没有启动。观察器现于每个既有采样周期同步目标stdout/stderr，而不只在进程退出后同步；同步失败记为 `live-output-sync`，不重新启动目标。子进程自身缓冲、两个同步点之间的尾部和硬件掉电仍可能丢失。此变化单独提交并用CPU子进程验证，观测I/O开销应纳入后续运行说明，不伪造历史日志。

### Settings DEBUG（桌面黑屏排查）

**Settings → General → Full debug logs → DEBUG: OFF / ON** 是持久开关，默认关闭。开启后保存 `debug_logging` 设置，复制当前数据目录中的完整 `app.log` 和递归 `analysis-logs`（包括 JSONL）到独立的 `debug-logs/session-…/`，写入平台和设置上下文，再持续采集；重启后在渲染器初始化前恢复开启状态。默认位置是 `~/.uta-studio/debug-logs/`；界面完成提示给出实际目录。缺失日志或跳过的链接／特殊文件记录在 `snapshot-notes.txt`。

开启时桌面 tracing 使用 DEBUG 级别；后续 app 日志镜像到快照 `app.log`，后端原始 stderr 不经内存摘要上限截断地写入 `backend-stderr.log`，新建的本地后端命令收到 `UTA_STUDIO_DEBUG=1`。再次点击关闭：恢复普通日志过滤、关闭镜像，新建后端命令清除继承的 DEBUG 环境变量；普通运行／错误日志保留。已启动 worker 的日志级别不能动态下调，但不再进行额外 stderr 镜像。桌面以保存的开关为准，不由 `RUST_LOG` 或继承的 `UTA_STUDIO_DEBUG` 偷偷启用详细日志。请先开启并等完成提示，再启动分析或复现黑屏。没有自动上传；日志可能含路径、配置和歌词。

**Settings → Storage → LOGS** 显示 `app.log`、`analysis-logs`、`debug-logs` 的文件总大小和数量，可点 **Refresh size** 重新统计。**Clear logs…** 需要确认，只清理这些日志，不碰歌曲、图表、模型或设置；不跟随符号链接。普通 app 日志原位截断，其他日志文件删除，目录保留。开启 DEBUG 时清理会关闭旧文件并恢复全新实时采集，因此清理后可能立即出现少量新日志；操作失败显示错误，不能把部分清理说成成功。

这不是系统／内核日志采集器，无法补回此前被过滤的事件，无法改变已经启动的 worker 的日志级别，也不打开编译期 GGML Vulkan 日志。已有分析文件仅做快照；并发后端 stderr 共用原始文件，可能交错。写入不承诺逐条 fsync 或断电尾部完整；日志开启后的吞吐不能当正常性能结果。

2026-09-11 针对性验证：app-core 快照／stderr 10 个隔离测试，桌面 DEBUG 4 个测试、Workflow 22 个测试、UI 脚本 4 个测试及 API 目录 2 个测试通过；桌面构建通过。隔离 Weston/Wayland + lavapipe smoke 实际开启 DEBUG 并确认后续 DEBUG 行，另在合成 workflow 身份上切换并保存 GAME small/large/medium。证据 `test-artifacts/debug-workflow-ui/`；操作记录从 `test-artifacts/operations/20260911T171855-e6d7ee77524a/` 起。初次编译暴露 plugin-group builder 调用错误并已修复；初次 smoke 缺少 PATH 中的 weston，补用已安装 store 路径后通过。最初空工作流 smoke 只证明命令分发，不能证明 GAME 变更，后续有载入工作流的 smoke 才证明切换保存。审查还修复了选择性 `RUST_LOG` 覆盖时丢失默认目标过滤的问题，以及从 Workflow 进入 Settings 后返回会丢失未保存草稿的恢复路径。最终 `d0358fd` 构建通过，`return-report.ndjson` 的三次 Back 均回到原 Workflow；独立 SQLite 检查确认离开前未保存的 GAME large 在返回后保存成功（操作 `20260911T172908-f85e1b79ce6d`）。没有模型推理、真实 GPU 黑屏复现、Windows 或物理鼠标点击验证；没有提升生产就绪状态。

### 全链路 debug 日志

`UTA_STUDIO_DEBUG=1` 让 `uta-analyze analyze` 将生命周期事件、worker 启动／命令意图、每个协议帧和退出状态即时写入 stderr；worker stderr 同时逐次读取转发，不受错误摘要的 1 MiB 内存上限截断。stdout 仍只承载机器结果。日志写入失败不终止 worker 排空；退出前收集 stderr 尾部。日志会包含本地路径、配置和转录文本，应作为本地诊断数据保管。

GGML 的 `VK_LOG_DEBUG` 是编译期开关，不能用 `RUST_LOG` 打开。需要用相同已声明补丁、独立构建／输出目录，并传 `-DGGML_VULKAN_DEBUG=ON`；无需改模型算法或安装用户运行库。`GGML_VK_MEMORY_LOGGER=1` 输出分配记录，`UTA_STUDIO_STAGE_PROFILE=1` 输出模型阶段计时。不要把 `GGML_VULKAN_CHECK_RESULTS`（额外计算）当作日志开关。本次 debug 回归保持优化构建，只增加日志，不能拿日志开启后的墙钟比较正常吞吐。

使用既有观察器同步目标 stdout/stderr 和主机采样；不宣称逐条 fsync，也不保证掉电尾部完整。缺少完成记录的全曲任务保持未知，新的 debug 执行使用新目录，禁止覆盖旧证据或自动重试。

| 文件 | 实际含义 |
| --- | --- |
| `prepared.json` | 启动任何子进程前，已记录 commit、dirty status、完整 argv、cwd、boot ID、时间及声明输入输出，并执行文件／目录 fsync。 |
| `stdin.bin` | 提交给命令的实际 stdin 字节，在启动前复制并 fsync。 |
| `exec-intent.json` | 子进程已记录自身 PID 和执行意图并 fsync，即将 `exec` 目标。**不证明 exec 成功或目标初始化完成。** |
| `spawn-error.json` / `exec-error.json` | 分别表示创建记录子进程失败或替换为目标命令失败。 |
| `stdout.txt` / `stderr.txt` | 程序输出，结束后 fsync；目标自身缓冲和断电仍可能导致尾部缺失。 |
| `result.json` | 父进程已收集退出码／信号、boot ID 与含子进程记录开销的墙钟。不证明退出后的桌面或整机稳定。 |

缺少 `result.json` 表示完成状态未知，不能推断程序没有启动。缺少启动记录也不能仅据此排除断电导致的记录丢失。已有记录描述观测边界，不是硬件故障时点的完整日志。

命令使用独立进程组；记录器将 INT／TERM／HUP 转发一次并等待结束，避免取消记录器却留下命令继续运行。SIGKILL 或整机掉电不能依赖用户态清理完成。

Git HEAD 不是未提交工作树的完整身份。记录器保存 dirty status，不宣称仅凭 commit 就能重建所有旧改动。后续将每次相关修改提交，并让构建记录和运行记录指向同一代码提交；旧未提交改动及此前没有启动日志的实验不能事后伪造对应提交。

本次瞬间掉电的确切触发步骤仍未确认，参见 [故障与候选代码路径](ROFORMER_B580_UPLOAD_BLACKOUT_2026-09-07.md)。记录方式的测试只用隔离 CPU 小程序，覆盖 stdin、退出失败、exec 失败、自身信号退出及记录器取消传播；不触发 GPU。
