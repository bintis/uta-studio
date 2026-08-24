# Native audio model catalog

Uta Studio keeps audio transformations, analysis experts, and runtime recipes separate.

- Audio transformation models are listed by `app-core/src/audio_model.rs`.
- Speech, pitch, boundary, and technique experts are represented by the workflow capability and native runtime registries.
- Exact Qwen runtime identities are locked in `native-inference/runtime-lock.json`.
- Generic production models are converted during explicit installation to pinned
  OpenVINO IR; production workers do not parse source ONNX/checkpoint formats.
  The locked Qwen GGML/Vulkan recipes are the only format/backend exceptions.

A catalog entry is not production support. Every `(model revision, backend, runtime recipe)` is classified independently as production-pinned, benchmark candidate, experimental, or unsupported. The router uses only production-pinned combinations and fails closed when no validated backend is available.

Models and runtime components are installed only after confirmation in **Settings > Models & runtime**. Startup, page rendering, status checks, diagnostics, and workflow compilation are read-only and never download artifacts. Existing model directories are user data and are not automatically removed or replaced.

Workflow nodes store catalog model IDs, never arbitrary checkpoint paths. Model file hashes, runtime recipe digests, exact input revisions, normalized parameters, and algorithm versions participate in artifact provenance and cache identity.

## RMVPE OpenVINO worker

`uta-openvino-worker` implements the RMVPE continuous-F0 contract with a native
Rust log-mel frontend and OpenVINO `GPU` inference. It rejects CPU fallback.
Explicit installation verifies the source RMVPE ONNX SHA-256
`5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd`
and writes a new, atomic OpenVINO IR v11 installation without moving, replacing,
or loading the source ONNX in production. The Worker verifies the IR manifest
and every consumed graph/weights hash, then emits 10 ms pitch evidence without
rounding frames directly to MIDI notes.

The generic OpenVINO Worker/runtime recipe identity is
`bdeac2a4e1299e4bf82cb2d4edf64c7bdbc613fa40f58727c58793cf7f1a4093`.
RMVPE's independently pinned bucket-conversion recipe remains
`ac3df548a9e51d36b5d5817ba6988eeaaa29f168d121588fd088daf91dbdf876`.
It pins source-built OpenVINO 2026.3.0 commit
`8a17657b995fd3b4a52f8484acfcf2bb61214623`; CPU and GPU plugins are enabled,
while NPU, Python bindings, automatic device plugins, and unused frontends are
disabled. CPU execution is allowed only for model-manifest-pinned islands and is
never an automatic fallback. OpenVINO 2026.3 requires static IR shapes
plus `GPU_ENABLE_LOOP_UNROLLING=NO` for RMVPE's GRU graph. The installer creates
32–1024-frame IR buckets sharing one immutable weights file; inference uses
128-frame overlap so song tails do not receive long artificial padding.
A synthetic 440 Hz real-decode/OpenVINO Arc smoke produced a 440.72 Hz mean F0;
full-song golden, repeat, cancellation, and contention acceptance remain
required before the complete native workflow can be declared finished.
