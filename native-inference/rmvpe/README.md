# RMVPE GGML/Vulkan runtime

This directory contains Uta! Studio's native RMVPE engine. Production requests
reach it only through `uta-ggml-worker`; the engine does not provide an
OpenVINO or automatic CPU fallback.

## Model conversion

Convert the cataloged `rmvpe.onnx` without modifying it:

```sh
python tools/convert_rmvpe_to_gguf.py /path/to/rmvpe.onnx /path/to/rmvpe-f32.gguf
```

The converter requires `onnx`, `numpy`, and `gguf`. It writes the 282-tensor
F32 `rmvpe` GGUF architecture consumed by `src/graph.cpp`. Runtime Manager
imports the result as `model:rmvpe` and retains source, converter, GGUF, and
runtime identities separately as provenance.

## Runtime build

The shared offline build recipe builds both the RoFormer and RMVPE engines
against the pinned GGML checkout:

```sh
UTA_GGML_SOURCE_DIR=/path/to/pinned/ggml \
  native-inference/ggml-worker/build-ggml-runtime.sh
```

The resulting runtime manifest is schema 2 and declares
`bin/uta-roformer-runtime` and `bin/uta-rmvpe-runtime`. The worker decodes input
to 16 kHz mono, invokes RMVPE with an explicit Vulkan device, validates the
ordered 10 ms output, and atomically publishes schema-2 pitch evidence.

RMVPE is currently a `BenchmarkCandidate`. Do not promote it from historical
OpenVINO evidence; accepted real-audio Vulkan parity and stability evidence is
still required.
