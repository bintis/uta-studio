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

## Rust WGPU/Vulkan workers

The dedicated Rust GGUF workers for JBM555, FCPE, Basic Pitch, FireRed, STARS,
and ROSVOT have a separate WGPU/Vulkan execution lane. This is not GGML:
GGUF is the weight container, while model kernels are implemented in Rust and
WGSL. The shared runtime compiles only its Vulkan/WGSL backend and has no
OpenVINO, OpenCL, SYCL, oneAPI, or Level Zero dependency.

Every GPU request must carry the exact `wgpu-vulkan-serial-v1` profile. Both
Analysis Engine and the worker enforce batch size 1, synchronous completion of
each bounded submission, and a serial pipeline before a Vulkan device is
created. Requested device classes never fall back to another class or CPU.
JBM555 uses receptive-field-preserving bounded frame chunks; FCPE and Basic
Pitch retain their fixed source windows. STARS/ROSVOT move their relative and
cross-attention score, row-softmax, and context operations to the same bounded
GPU lane while retaining stage/bucket boundaries and deterministic host
orchestration. ROSVOT additionally keeps both directions of each annotation-
RMVPE GRU window inside one bounded Vulkan dispatch instead of synchronizing six
matrix-vector operations per frame. FireRed uses the shared relative- and
decoder-attention kernels, read-only memory-maps its 4.7 GB GGUF, keeps native
F32 tensors as shared lazy views, and uploads bounded operation weights rather
than making a second all-model host copy or all-model GPU copy.

Per explicit repository-owner direction, these six WGPU capabilities are
`ProductionPinned` and are each model's default backend; CPU remains an explicit
diagnostic/reference lane rather than a fallback. Separately authorized
2026-09-05 execution on Intel Arc B580 passed short CPU/WGPU parity for every
exact graph: JBM555 produced the same five notes with maximum numeric delta
`2.4e-7`; FCPE's 601 frames had maximum delta `0.0001 Hz`; Basic Pitch's 2,589
compared values had maximum delta `3.784e-5`; STARS's 628 values had maximum
delta `3.6e-5`; FireRed emitted identical `你好世界` text and token sequence
`1202,2246,1019,4710`; and ROSVOT's 492 values had maximum delta `9.5e-5`.

One serial representative full-input WGPU run also passed for each graph:
JBM555 processed 305.813333 s in 312.083 s and emitted 33 notes; FCPE took
55.401 s and emitted 30,582 F0 frames; Basic Pitch took 30.224 s and emitted
26,340 activation frames; FireRed processed the 216.880 s Chinese `崔子格 -
卜卦` input in 80.792 s across 94 windows; STARS processed a 245.120 s real
Chinese vocal track with 197 timed words in 161.424 s across 78 conditioned
segments; and ROSVOT processed 305.813333 s across 114 conditioned segments.
The original ROSVOT validation accidentally used a debug worker and took
823.102 s; a release rebuild took 522.403 s, and the persistent Vulkan GRU path
reduced the same run to 155.613 s (3.357× faster than the unoptimized release,
5.289× faster than the debug run). It retained all 397 regulated notes and
57,340 valid frames; only raw audit logits in one sensitive region changed,
while regulated semantic evidence remained equal. All validated numeric
evidence was finite. STARS deliberately keeps
its pinned G2P Chinese-only: an attempted Japanese input failed closed on `を`
and was not counted as success.

The ROSVOT run uses the official conditioned 50,000-step checkpoint
`7501fb5f…3fcb` (245 F32 tensors), converted to F32 GGUF `a8d8eeb8…df6f` and
published as an immutable runtime generation without replacing the existing
staged OpenVINO generation. The checkpoint passed genuine PyTorch-reference
stage checks before CPU/WGPU parity. Every full run used the mandatory serial
profile; source identities and boot ID remained unchanged, a sudo-readable
whole-boot kernel review found no GPU hang/fault/reset, and FireRed additionally
remained clean through a 420 s post-exit watch. The Production promotion is an
explicit owner policy decision rather than an automatic inference from one
full-input pass; repeat and broader quality coverage remain advisory follow-up.
Historical OpenVINO, GGML, or CPU evidence remains backend-specific and
non-interchangeable.

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
