# Bounded native AMD LibTorch checks

A native C++ executable checks GEMM, biased convolution, bidirectional GRU and
fused SDPA against complete double-precision mathematical references. It reads
no model or user audio, invokes no Python inference, and has no XPU or CPU
execution fallback. The selected AMD device and any architecture override are
reported explicitly. Use no architecture override for native gfx1103 verification.
CPU code is only the explicitly declared numerical oracle.

This is operator-support/correctness evidence, **not seventeen-model acceptance,
whole-model speedup, a sustained benchmark or post-exit host stability**. Every
case is a separate explicit invocation; do not automatically retry a GPU failure
or replace an unavailable fused-attention implementation with dense math SDPA.

Build using the private AMD native dependencies, inside `bash dev.sh` and the
repository operation recorder:

```sh
cmake -S tools/libtorch-device-check -B test-artifacts/libtorch-models/amd/check-build \
  -DCMAKE_BUILD_TYPE=Release -DTORCH_ROOT="$torch_root" -DROCM_ROOT="$rocm_root"
cmake --build test-artifacts/libtorch-models/amd/check-build -j2
```

`TORCH_ROOT` is the installed native torch directory, not site-packages itself.
`ROCM_ROOT` is the expanded private `rocm-sdk path --root` directory. Preserve
`torch/.kpack`, `torch/lib/aotriton.images` and ROCm device files: copying only
shared libraries omits architecture-specific kernels. Build does not require
running/importing torch. On NixOS, expose needed native dependency directories
only to the diagnostic process; do not modify global loader settings or installed
application libraries.

After recording host/GPU load, launch **one** selected case under the existing
`observe-roformer-run.py` observer and operation recorder:

```sh
uta-libtorch-device-check gemm
uta-libtorch-device-check convolution
uta-libtorch-device-check gru
uta-libtorch-device-check attention
```

The listed commands are individual test choices, not an instruction to launch
them concurrently. Each result reports device identity, full element count,
finite output, NMSE, maximum absolute error and synchronized *cold* compute time.
Initialization, upload, reference computation and final readback are outside
that interval. Cold compilation/autotuning may be included; these times are not
steady-state performance. No warmup or favorable sample is silently selected.

GEMM, convolution and GRU use FP32 tensors with IEEE precision requested. SDPA
uses FP16-rounded inputs and FP16 output, converted to FP32 inside the timed
interval. Its reference uses the same rounded inputs, so it does not establish
parity with a wholly FP32 model. Full-output NMSE limits are `1e-10` for FP32
arithmetic and `5e-4` for the explicitly mixed-precision attention check.
