# Isolated native XPU / Vulkan operator comparison

These executables generate synthetic tensors with the current XE90 projection and
attention shapes. They do not load a model, process user audio, replace a runtime,
or register a production backend. All inference operations are C++ calls; Python
is used only for acquisition, operation recording, host observation and summaries.

## Build

Use a private directory, `root`, outside installed runtime and model directories.
The experiment extracts the official `torch 2.13.0+xpu` Linux wheel into
`$root/native`, providing `torch/include` and `torch/lib`. Native dependencies are
installed into `$root/venv` from their official PyPI releases. The explicit helper
`fetch_dependencies.py ROOT all` downloads the versions recorded in its source;
it has no automatic retry or inference behavior. It requires an existing private
venv with pip and does not install the complete Python torch package.

Copy the selected GGML library directory to `$root/ggml-reference` for a read-only
comparison. The root itself must be a new diagnostic directory. Keep the original
application libraries and models unchanged.

```sh
cmake -S tools/libtorch-xpu-probe -B "$root/cmake-build" \
  -DTORCH_ROOT="$root/native/torch" \
  -DXPU_DEPENDENCY_LIB="$root/venv/lib" \
  -DGGML_INCLUDE_DIR="$ggml_source/include" \
  -DGGML_LIBRARY_DIR="$root/ggml-reference/lib"
cmake --build "$root/cmake-build" -j2
```

Run native build commands through `bash dev.sh` and persist their full commands
with `tools/record-operation.py`, following the repository operation rules. The
probe links XPU registration libraries explicitly without Python initialization.

## Run

On NixOS, expose existing Level Zero and OpenCL loaders only to the probe process:

```sh
export UTA_PROBE_LEVEL_ZERO_LIB_DIR=/path/to/existing/level-zero/lib
export UTA_PROBE_OPENCL_LIB_DIR=/path/to/existing/ocl-icd/lib
export UTA_PROBE_OPENCL_VENDORS=/run/opengl-driver/etc/OpenCL/vendors
bash tools/libtorch-xpu-probe/run-probe.sh "$root" torch attention-time f16 8 8
bash tools/libtorch-xpu-probe/run-probe.sh "$root" ggml attention-time f16 8 8
```

Use `tools/observe-roformer-run.py NEW_CASE_DIRECTORY -- COMMAND...` within a
recorded dev-shell operation for actual comparisons. It records host load and
other DRM clients alongside child execution and results. Avoid overlapping probe
runs. Existing user jobs are not stopped and observations do not prove exclusive
access. Use `UTA_PROBE_VERBOSE=all` for oneDNN implementation diagnostics, not
performance acceptance. The oneDNN SDPA microkernel requires OpenCL loader access
even when the selected PyTorch execution stream uses Level Zero.

Cases: `smoke-gemm`, `smoke-attention`, `qkv-time`, `qkv-frequency`, `ffn-time`,
`ffn-frequency`, `down-time`, `attention-time`, and `attention-frequency`.
Storage choices: `f32`, `f16`, `bf16`. Each invocation accepts warmup and measured
iteration counts after the case and storage arguments.

`UTA_PROBE_GEMM_LAYOUT=packed` adds an explicitly timed GGML contiguous copy;
`flat` also folds independent shared-weight batches into the row dimension.
`UTA_PROBE_GEMM_LAYOUT=batched` forces LibTorch BMM with zero-stride shared weights
instead of ordinary matmul. These are diagnostics, not installed graph changes.

## Interpretation and precision

The common input generator is deterministic across backends. Frequency GEMMs
start from the same transposed physical input view, not a hidden prepacked copy.
Torch may internally flatten, broadcast or reorder; those operations are included
in its compute interval. GGML optional packing is also inside its timed graph.
Input generation, dtype preparation, allocation/upload and warmup are separate.

Primary timings measure synchronized host compute, including dispatch, completion
and any internal output dtype conversion. Additional XPU profiling-event times
are reported separately. The GGML perf logger records its GPU operations. Do not
compare host time from one backend to GPU-only time from the other.

The FP32 GEMM policy requests IEEE behavior and disables oneDNN TF32. The probe
uses a stricter FP32 GEMM NMSE threshold of `1e-10`; mixed precision and attention
use `5e-4`. Each result scans every output for finite values and checks 128
positions against a full-contraction FP64 reference. It is not a complete-output
error comparison or model-quality qualification. Both rounded-input and original
FP32-input reference errors are retained.

LibTorch low-precision operations produce low-precision outputs, then convert to
FP32 inside the timing interval. GGML directly produces FP32 accumulators/output.
These are not bitwise-equivalent arithmetic paths. GGML attention requires F32 Q
storage and preserves its existing internal operand/probability rounding. F32
attention storage is therefore not a promise of strictly FP32 attention arithmetic.
The GGML BF16 GEMM route stores BF16-rounded activations in F32 because its backend
mixed decoder requires that representation. Report these distinctions explicitly.

Math-SDPA fallback is disabled in LibTorch to avoid silently allocating the full
quadratic attention matrix. oneDNN diagnostics still need to confirm its chosen
implementation; an overrideable SDP dispatch alone is not proof of a microkernel.

```sh
python3 tools/libtorch-xpu-probe/summarize.py "$root"
```

The summary preserves failed, incomplete and contended runs. The experiment does
not establish actual XE90 model throughput, audio equivalence, AOTInductor results,
release readiness, Windows support, or other-vendor behavior.
