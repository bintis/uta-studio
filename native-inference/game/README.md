# GAME GGUF conversion and native runtime support

This directory contains model conversion and verification tooling for Uta! Studio's
native GAME (Generative Adaptive MIDI Extractor) pipeline.

## Model conversion

Convert the official GAME 1.0.3 medium ONNX directory into a single F32 GGUF artifact:

```sh
python tools/convert_game_to_gguf.py /path/to/extracted/GAME-1.0.3-medium-onnx /path/to/game-medium-f32.gguf
```

The converter requires `onnx`, `numpy`, and `gguf`. It unpacks the three official
subgraphs (`encoder.onnx`, `segmenter.onnx`, and `estimator.onnx`) from the upstream
`GAME-1.0.3-medium-onnx.zip` distribution and combines them into one GGUF container
with 668 F32 tensors and standardized `game.*` metadata.

The conversion contract:
- Restores transposed PyTorch Linear weights from ONNX MatMul initializers.
- Maps LayerScale, depthwise convolution, and pool token initializers to canonical state-dict names.
- Synthesizes structural compatibility tensors for the Estimator layer 3 frame stream (`_x`), which was pruned by upstream ONNX export but is bound by native GGUF loaders.
- Injects sampling, mel-spectrogram, D3PM, and language parameters required by native execution.

## Native GPU backend (`--features gpu`)

The `Tensor` trait (`src/core/tensor/mod.rs`) has two implementations: `CpuTensor`
(always available) and `GpuTensor`, a wgpu/Vulkan backend behind the `gpu` Cargo
feature. `core/model/{encoder,segmenter,estimator,ops,blocks}.rs` are written
generically over `T: Tensor`, so both backends run the identical model graph
unmodified.

`GpuTensor` mirrors `CpuTensor`'s view/buffer design (shape/strides/offset over a
shared backing store) instead of hand-rolling stride math per shader: reshape,
transpose, slice and contiguous-view detection are metadata-only, zero GPU work.
Compute-heavy ops (matmul, elementwise, softmax, rms_norm, conv1d_dw, rope) run as
real WGSL compute shaders (`src/core/tensor/gpu/shaders/*.wgsl`); structural ops
with negligible FLOP cost (concat, embedding, repeat) go through a CPU round trip
using the same indexing algorithms as `cpu/{layout,indexing}.rs`.

Every GPU dispatch is followed by an explicit `queue.submit` +
`device.poll(PollType::Wait)` before returning — this is the wgpu-level equivalent
of this codebase's established Arc-B580 stability default (`GGML_VK_DISABLE_ASYNC=1`
/ `--vulkan-no-async`, see `native-inference/roformer/README.md`), applied to every
submission since wgpu has no matching env var.

```sh
cargo build -p uta-game-worker --features gpu
```

`cargo test -p uta-game-worker --features gpu` runs two kinds of test:
`wgsl_shaders_parse_and_validate` (`src/core/tensor/gpu/tests.rs`) parses and
validates every shader with `naga` — pure CPU, no GPU/adapter contact, always safe
to run. Every other `core::tensor::gpu::tests::*` test creates a real `GpuDevice`
and dispatches real Vulkan compute on whatever adapter is present, so only run
those with the same explicit authorization this repository requires for any
non-Qwen Vulkan/GPU execution.

In a plain shell (outside the Nix dev shell, which already wires this via
`flake.nix`), the default environment usually can't find a working Vulkan loader —
`Instance::enumerate_adapters` silently returns zero adapters rather than erroring
(check with `RUST_LOG=wgpu_hal=debug,wgpu_core=debug` if that happens and the cause
isn't obvious). On NixOS, `/run/opengl-driver/share/vulkan/icd.d/` has the system
ICD manifests but not `libvulkan.so.1` itself; find a *64-bit* `vulkan-loader`
output on the store (some resolve to a 32-bit build, which fails with a
`dlopen`/`wrong ELF class: ELFCLASS32` error, not a clear "not found") and put it
on `LD_LIBRARY_PATH`:

```sh
loader_lib=$(for p in /nix/store/*-vulkan-loader-*/lib/libvulkan.so.1; do
  [ "$(od -An -tx1 -j4 -N1 "$p")" = " 02" ] && dirname "$p" && break
done)
VK_ICD_FILENAMES=/run/opengl-driver/share/vulkan/icd.d/intel_icd.x86_64.json \
LD_LIBRARY_PATH="$loader_lib:/run/opengl-driver/lib:$LD_LIBRARY_PATH" \
cargo test -p uta-game-worker --features gpu -- --test-threads=1
```
