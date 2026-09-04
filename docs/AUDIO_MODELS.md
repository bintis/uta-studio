# Native audio model catalog

Uta! Studio keeps audio transformations, analysis experts, and runtime recipes separate.

- Audio transformation models are listed by `app-core/src/audio_model.rs`.
- Speech, pitch, boundary, and technique experts are represented by the workflow capability and native runtime registries.
- Exact Qwen runtime identities are locked in `native-inference/runtime-lock.json`.
- Runtime formats are model-specific. Most generic models use explicitly
  installed OpenVINO IR, while Qwen, RMVPE, and the selected RoFormer family use
  locked GGML/Vulkan recipes. These native GGUF routes never launch OpenVINO.

A catalog entry is not production support. Every `(model revision, backend, runtime recipe)` is classified independently as production-pinned, benchmark candidate, experimental, or unsupported. The router uses only production-pinned combinations and fails closed when no validated backend is available.

Models and runtime components are installed only after confirmation in **Settings > Models & runtime**. Startup, page rendering, status checks, diagnostics, and workflow compilation are read-only and never download artifacts. Existing model directories are user data and are not automatically removed or replaced.

Workflow nodes store catalog model IDs, never arbitrary checkpoint paths. Model file hashes, runtime recipe digests, exact input revisions, normalized parameters, and algorithm versions participate only in artifact identity, provenance, and cache identity; hashes are not acceptance gates.

## RMVPE GGML/Vulkan worker

`uta-ggml-worker` implements RMVPE continuous-F0 inference through the dedicated
`uta-rmvpe-runtime` engine. The engine runs the 16 kHz/128-bin log-mel frontend,
CNN/U-Net, chunked bidirectional GRU, output head, and continuous pitch decoder
natively. It emits ordered 10 ms pitch frames and does not quantize evidence to
MIDI notes.

Explicit local conversion records the source RMVPE ONNX identity
`5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd`,
the F32 GGUF identity
`1b4095d1b57818f5e812b1986ea5a7d7e6d64ccd9e1b1d7b71f4091304513fd2`,
and conversion recipe
`07856e413b0f141b7e0354f6edc52ffcfd853f8b33f4641d15e930aa1b888776`
as separate provenance. The multi-engine GGML runtime recipe is
`dd364845b256b8adc04c291e9c79a3426fe960ca1a7beab3990fdbcdc9e7bfd2`.
The worker validates runtime structure, model size and RMVPE GGUF metadata,
selects the requested Vulkan device class, and removes inherited diagnostic CPU
controls before execution. There is no automatic CPU or OpenVINO fallback.

RMVPE currently remains a `BenchmarkCandidate`. Prior OpenVINO measurements do
not qualify this new backend; promotion requires accepted real-audio Vulkan
output and stability evidence.

## RoFormer backend selection

`uta-ggml-worker` validates safe paths, declared files, byte sizes, and exact
GGUF/runtime semantic identities while retaining hashes only as provenance
metadata. It emits typed lossless stem outputs without CPU fallback. All five RoFormer resources—BS-RoFormer
Vocals EP317, MelBand Inst V2, MelBand Harmony, Denoise and Dereverb—expose only
their user-selected GGML/Vulkan `ProductionPinned` routes and must never
launch OpenVINO. The Worker always passes `--batch-size 1`,
`--vulkan-no-async` and `--serial-pipeline`. All five exact GGUFs have isolated
305.813333-second full-song evidence; this does not authorize concurrent or
stress execution and backend-specific evidence is never interchangeable.
