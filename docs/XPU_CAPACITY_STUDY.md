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

由实际运行完成后补入，不把计划写成完成。
