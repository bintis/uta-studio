# Native LibTorch execution alongside GGML

## Scope and status

Authorized on 2026-09-10: implement independent native LibTorch execution for all seventeen current catalog resources, retaining GGML as a separate backend. This authorization supersedes the former single-GGML model-computation restriction for this work only. Native-only inference, the Studio/Analysis Engine/Runtime Manager process boundaries, read-only source media, and explicit device selection remain unchanged.

Status: **implementation in progress; no LibTorch whole-model GPU result or production qualification yet**. Original unrelated working-tree changes are preserved. Development evidence is under `test-artifacts/libtorch-models/` and the operation recorder.

The motivation is `docs/ROFORMER_B580_LIBTORCH_XPU.md`: native ATen/oneDNN outperformed GGML Vulkan for the tested projection and full-context attention operators. Those measurements are not whole-model speedups and do not establish audio parity. No multiplication of operator ratios is used to predict production throughput.

## Execution boundary

- Rust continues to own request validation, audio preparation/decoding, model conditioning, typed evidence, progress and publication. Studio does not prepare tensors or import backend implementation crates.
- The new native route computes with ATen/LibTorch directly. It must not execute Python, call a remote inference service, require TorchScript conversion, emulate the GGML C ABI, or silently call the GGML implementation.
- Existing GGUF files are read without model-store mutation. Stored dimensions and dtypes are translated deliberately into row-major native tensors. All model weights remain resident on the explicitly selected device for the session lifetime.
- The native C ABI uses explicit owned handles, catches exceptions, returns actionable errors, and retains the loaded shared library until the last native object is released. Rust builds do not depend on a globally installed LibTorch; the native library has a separate CMake build.
- `libtorch_rocm` and `libtorch_xpu` are distinct explicit choices. The AMD implementation uses the CUDA-named interfaces provided by ROCm LibTorch. An explicit CPU diagnostic lane is not a production fallback. The existing default remains `ggml_vulkan`.
- Runtime Manager remains the installation/readiness authority. An unbuilt or uninstalled native runtime must not be advertised as ready. The application must not acquire dependencies during normal analysis.

## Per-model execution plans

The following are implementation targets, not claims that a model has already passed verification.

| Resource | Native execution and reuse | Precision and correctness focus |
| --- | --- | --- |
| `bs_roformer_leap_xe90_vocals` | Band split and mask projections, alternating full-context time/frequency SDPA; shared-weight contiguous projections may fold batches; retain chunk scratch and resident weights; emit vocals and the instrumental residual from one invocation. | FP32 norm/residual/projection reference; explicit FP16 SDPA rounding; compare both complete stems and reconstruction. |
| `bs_polarformer_public_instrumental` | Separate PolarFormer plan honoring its own band/position/mask geometry rather than assuming the XE90 graph; bounded overlap-add and one shared input spectrum. | Compare instrumental and vocal residual; preserve the model's actual positional transform and complex-mask convention. |
| `melband_roformer_harmony` | Mel-band gather/project, alternating axis attention and mask estimation; reuse overlap-add/frontend work; emit lead plus residual. | Model-specific band overlaps, complex masks and exact output length; do not infer parity from XE90. |
| `melband_roformer_denoise_aufr33` | Native mel-band RoFormer with its own dimensions/depth and chunk geometry; resident weights and fused SDPA. | Complete denoised waveform comparison and boundary checks. |
| `melband_roformer_dereverb_anvuew` | Native mel-band RoFormer with its own configuration and bounded chunk execution. | Complete dereverberated waveform comparison, tails and chunk seams. |
| `rmvpe` | Native convolutions, pooling/upsampling and bidirectional GRU; sequence computation remains on device rather than dispatching each timestep from the host. | Preserve FP32 reference computation and pitch/confidence/voicing decoding, especially low-confidence onset frames. |
| `fcpe` | Native convolution/depthwise convolution and the actual model linear-attention formulation; keep the whole bounded sequence on device. | Do not replace linear attention with softmax SDPA; compare F0, confidence and every voiced decision. |
| `basic_pitch` | Native convolution/pooling with reusable fixed-window batches and unchanged frontend/output cadence. | Preserve onset, frame and contour axes and exact frame timestamps; compare full activation outputs. |
| `game_1_0_3_small` | Size-derived native encoder, conditioned iterative segmenter, pitch estimator; cache encoder/conditioning features across diffusion steps. | Honor the checkpoint's dimensions, schedule, RNG and boundary decoding; independent small-model execution. |
| `game_1_0_3_medium` | The same native architecture family with medium checkpoint geometry, bounded overlapping windows and reused iterative features. | Preserve ordered note timing/pitch/confidence and deterministic seeded fixtures. |
| `game_1_0_3_large` | Large-size native plans, bounded temporary memory and shared feature reuse across segmenter/estimator calls. | Independent large-model execution, not a readiness inference from medium. |
| `jbm555_cectc_80` | Native CNN over mandatory mix plus prepared-vocal features; bounded long-input chunks with contextual overlap. | Preserve both inputs, chunk context and onset/offset/CTC-style note decoding. |
| `stars` | Native learned stages for acoustic/pitch conditioning, utterance/rhythm, note pitch, sentence/style and technique heads; reuse stage outputs and real transcript/RMVPE conditioning. | Canonical weight names, masks, positions, grouping/aggregation and all nine technique classes; verify each stage and final evidence. |
| `rosvot` | Native mel/pitch/word conditioning, convolution/conformer backbone and frame/note heads; bounded frame/note buckets. | Preserve transcript and RMVPE dependencies, valid-frame masks and final note aggregation. |
| `firered_asr2_aed` | Native CNN/conformer encoder and AED decoder; retain encoder output and precomputed cross-attention K/V, use incremental self-attention cache where applicable. | Strict FP32 convolution/matmul, required projection biases, exact reference tokens; preserve CMVN/dictionary and unfinished-window semantics. |
| `qwen3_asr_1_7b` | Native CNN/transformer audio encoder, GQA/RoPE autoregressive decoder, persistent per-layer KV cache and device-resident audio embeddings. | Preserve F16/F32 stored weights, activation policy, positions, audio-token insertion, tokens/language and bounded long-input handling. |
| `qwen3_forced_aligner_0_6b` | Separate aligner geometry and timestamp-classification head over the shared native encoder/decoder family; audio embeddings remain resident. | Preserve prompt construction, timestamp token decoding, ordered word boundaries and exact model-specific dimensions. |

## Hardware and memory policy

Inference mode disables autograd. Transfers happen at bounded model/stage boundaries, not between every native operator. Shared-weight GEMMs use layouts that permit efficient native linear/matmul dispatch; noncontiguous frequency layouts are measured including any copy. Native convolution, normalization, recurrent and attention primitives are preferred over host-expanded loops. Large full-context attention must not silently materialize a quadratic score tensor just because a fused kernel is unavailable.

FP32 correctness-sensitive paths are kept separate from explicit mixed-precision candidates. BF16, TF32, approximate GELU and indiscriminate model-wide FP16 are not default substitutions. Backend diagnostics must make the executed device, native runtime, storage/compute choices and any supported-kernel failure visible. A faster precision setting is accepted on its own numerical/output evidence, never because another model tolerated it.

RoFormer chunk buffers, GAME iterative features, Qwen KV caches and encoder outputs, and FireRed cross-attention projections have different lifetimes. Their ownership must match those lifetimes; merely reserving memory or retaining an unused copy is not a residency optimization. No unrelated super-acceleration scheduling changes are required by this task.

## Verification order and measurement

1. Finish the native implementation and routing for every listed resource. Compile and run focused host-only tests for ABI ownership/errors, GGUF names/dimensions, masks, positions, cache progression, output shapes and unavailable-backend behavior.
2. Inspect current host load and run the AMD GPU lane first using the exact selected device and an isolated native runtime directory. Start with bounded deterministic operator/model fixtures, then exercise each model with real installed weights and the existing dependency-ordered audio fixture. Record failures without fallback or automatic GPU retry.
3. Only after all implementation code exists and the AMD results have been inspected may XPU tests be invoked. Reuse the same inputs, correctness checks and timing boundaries. Do not invoke an XPU probe early as a substitute for AMD availability.
4. Keep compilation, successful process exit, finite output, numerical parity and listening/production qualification as separate statuses.

Each actual native execution is committed and recorded before launch through `tools/record-operation.py`, with complete command, environment, inputs/outputs and matching commit. Rust/native commands run inside `bash dev.sh`. GPU observations retain host load and other clients; no unrelated process is stopped. Missing completion evidence means unknown.

Record model load, frontend preparation/upload, synchronized native computation, final readback/postprocessing and whole worker wall time separately. Warmups are separate from measured runs. Compare synchronized time with synchronized time, not GPU-event time with process wall time. Retain every measured sample, failed invocation and contention observation.

Full output checks include all finite values, sample/frame count, timing/order and mandatory conditioning. Numerical comparisons use waveform residual/reconstruction and complete activation/logit vectors where available, plus discrete voicing/note/token/alignment differences. A same-device GGML comparison is a useful implementation comparison, not automatically checkpoint truth; existing checkpoint/reference fixtures remain important, particularly FireRed FP32 and RMVPE low-confidence frames.

## Source references

- Local measured motivation: `docs/ROFORMER_B580_LIBTORCH_XPU.md`.
- Current model inventory and earlier reference evidence: `tasks/remaining-models/STATE.md`.
- Execution provenance and host observation: `docs/ROFORMER_OPERATION_RECORDING.md`.
- PyTorch native C++ interface: https://docs.pytorch.org/cppdocs/
- PyTorch ROCm/CUDA-interface semantics: https://docs.pytorch.org/docs/main/notes/hip.html

Exact installed runtime versions, model results and remaining blockers will be appended after execution; none are inferred from package availability or historical operator measurements.
