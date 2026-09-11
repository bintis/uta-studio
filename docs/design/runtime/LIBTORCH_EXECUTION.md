# Native LibTorch execution alongside GGML

## Scope and status

Authorized on 2026-09-10: implement independent native LibTorch execution for all seventeen current catalog resources, retaining GGML as a separate backend. This authorization supersedes the former single-GGML model-computation restriction for this work only. Native-only inference, the Studio/Analysis Engine/Runtime Manager process boundaries, read-only source media, and explicit device selection remain unchanged.

Status: **all eighteen resources completed real full-song XPU diagnostic execution; product routing and production qualification remain incomplete**. The original seventeen-resource authorization count is historical. See [current full-song results](../../LIBTORCH_XPU_FULLSONG_RESULTS.md) for actual dependencies, repairs, output checks and unresolved quality/host-stability questions. Original unrelated working-tree changes are preserved.

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
| `bs_roformer_leap_xe90_instrumental` | Independent installed XE90 instrumental checkpoint; native mask output plus vocal residual, not a second name for the vocal checkpoint. | Complete instrumental waveform, residual reconstruction and checkpoint-specific output checks. |
| `bs_polarformer_public_instrumental` | Separate PolarFormer plan honoring its own band/position/mask geometry rather than assuming the XE90 graph; bounded overlap-add and one shared input spectrum. | Compare instrumental and vocal residual; preserve the model's actual positional transform and complex-mask convention. |
| `melband_roformer_harmony` | Mel-band gather/project, alternating axis attention and mask estimation; reuse overlap-add/frontend work; emit lead plus residual. | Model-specific band overlaps, complex masks and exact output length; do not infer parity from XE90. |
| `melband_roformer_denoise_aufr33` | Native mel-band RoFormer with its own dimensions/depth and chunk geometry; resident weights and bounded explicit mixed attention on ROCm. | Complete denoised waveform comparison and boundary checks. |
| `melband_roformer_dereverb_anvuew` | Native mel-band RoFormer with its own configuration and bounded chunk execution. | Complete dereverberated waveform comparison, tails and chunk seams. |
| `rmvpe` | Native convolutions, pooling/upsampling and bidirectional GRU; sequence computation remains on device rather than dispatching each timestep from the host. | Preserve FP32 reference computation and pitch/confidence/voicing decoding, especially low-confidence onset frames. |
| `fcpe` | Native input convolutions, four-group timeline normalization and six gated depthwise-convolution blocks; keep the complete bounded sequence on device. The current 59-tensor catalog checkpoint has no learned attention weights. | Preserve the exported output-matrix orientation; do not introduce attention absent from this checkpoint. Compare F0, confidence and every voiced decision. |
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

Inference mode disables autograd. Transfers happen at bounded model/stage boundaries, not between every native operator. Shared-weight GEMMs use layouts that permit efficient native linear/matmul dispatch; noncontiguous frequency layouts are measured including any copy. Native convolution, normalization, recurrent and attention primitives are preferred over host-expanded loops. Large full-context attention must not silently materialize a quadratic score tensor just because a fused kernel is unavailable. ROCm projection submissions are also bounded by contraction work, including per-band mask estimators rather than only transformer feed-forward layers.

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

## Bounded XPU resumption (2026-09-10 UTC)

The user's explicit request to resume XPU testing authorized this separate lane; it did not resume
Vulkan Super crash reproduction or hardware-counter stress groups. Existing AMD strict results
(`test-artifacts/libtorch-models/amd/final-strict-18.jsonl`) were inspected first: 17/18 resource
routes completed, with a recorded ROCm/MIOpen RMVPE GRU failure. Subsequent parallel AMD work is not
requalified by this XPU run. Current native factories list eighteen resources, including both LEAP
XE90 outputs; the earlier seventeen-resource plan is not an acceptance count.

- `5a46dfc` adds `native_tensor_check`: Rust owns explicit backend/device/precision selection and
  ordered model calls; complete typed outputs, shapes and timing metadata are saved in new
  diagnostic directories. It has no Python inference, hidden CPU/GGML fallback or model-store
  mutation. Its focused host test and release build passed (`20260910T171947-8a16a5b71d86`).
- The first rebuilt XPU contract invocation (`20260910T172119-cb27d811ef7b`) exited 127 in ELF loading:
  private `libsycl.so.9` was present, but RUNPATH did not cover indirect dependencies. No model test
  was entered. `912c9f9` applies inherited private RPATH for XPU, as already used for ROCm. The failed
  build remains in `xpu-resume/runtime-build`; the corrected independent build is
  `xpu-resume/linked-runtime-build` (`20260910T172237-5475b5576ad9`). No system library was changed.
- The corrected native ABI/synthetic FCPE plan passed on **`libtorch_xpu`, device 0, strict**
  (`20260910T172427-1d4e46c7699f`): complete fifteen-value comparison, F32/F16 stored weights,
  cancellation, ownership after runtime release, output shape and error handling.
- Read-only real FCPE weights then executed 201/97/201-frame synthetic mel inputs in one retained
  model, with identical first/repeated inputs. Explicit CPU reference:
  `20260910T172609-fe134b3ec34c`; XPU: `20260910T172724-e05fc4d00416`.
- **All 179,640 CPU/XPU activation pairs were finite and compared**, not sampled. Per-window NMSE:
  `1.57628370956e-11`, `1.18181985167e-11`, `1.58845205696e-11`; maximum absolute error across the
  pair: `6.29370333627e-10`. XPU first/repeated outputs were **not bit-identical**: maximum difference
  `1.23691279441e-10`, NMSE `1.02478977560e-12`. No exact-determinism or perceptual claim follows.
- Observations retain the target's Intel Level Zero driver and xe PCI device `0000:07:00.0`.
  Before real-weight XPU execution, host CPU busy was about 17%, B580 busy about 12%; other UI/test
  processes remained active. There was no exclusivity assertion, process termination or idle poll.
  Upload, synchronized compute and readback are separate host timings; cold/shape compilation and
  one repeated call do not constitute a controlled speed benchmark.

Evidence: `test-artifacts/libtorch-models/xpu-resume/`, especially `fcpe-comparison.json`
(`20260910T172812-7e4b12c40817`), full output files and per-run observations. An initial request-file
preparation did not forward undeclared recorder stdin and generated nothing; its attempted CPU
reference exited before model loading (`20260910T172455-20e45cf6434e`). The corrected generator is
recorded in argv (`20260910T172546-f9404a29aa2d`); both failed/setup records remain intact.

A subsequent Qwen ASR check used the installed 4,083,087,904-byte F16 container read-only and the
real 12-second mono 16 kHz fixture. On explicit **`libtorch_xpu`, device 0, strict**, the complete
frontend and 24-layer encoder returned 156 x 2,048 finite values in 0.803520399 seconds, excluding
runtime/model load and frontend preparation. This is one traced cold call, not a controlled speed
measurement. All 79 synchronization checkpoints completed, through layer 23 feed-forward. The
observer sampled only Intel Level Zero `libze_intel_gpu.so` and xe PCI `0000:07:00.0`, with peak
resident VRAM 8,550,948 KiB, no observer errors and an unchanged boot ID. The oneDNN stream and
checkpoints were continuously persisted in the case directory. Operation:
`20260910T180636-d09ab6d785e8`; evidence:
`xpu-resume/qwen-asr-strict-trace-observation/`.

`2023966` adds trace-only Qwen decoder checkpoints for session allocation, positions, each layer's
QKV/cache/attention/feed-forward and final logits. They do not synchronize unless
`UTA_STUDIO_LIBTORCH_TRACE_SYNC=1` and do not change the arithmetic path. The matching XPU native
build and host check passed (`20260910T181248-23771c747dd3`,
`20260910T181506-e74575688ac6`). One synthetic zero-mel row then exercised encoder output, an
explicit four-position KV session, audio injection and two incremental strict decoder steps on CPU
and XPU. All **305,920 F32 values and three positions** were compared, not sampled. Encoder maximum
absolute error was `1.28522515297e-7` (NMSE `6.96887076282e-13`); prefill logits maximum was
`1.09672546387e-4` (NMSE `1.18774244385e-10`); second-step logits maximum was
`5.53131103516e-5` (NMSE `7.97036728004e-13`). Complete-logit argmax matched at 198 and 16,
respectively, and every position matched exactly. XPU operation:
`20260910T181634-369af609eb70`; comparison: `20260910T181727-866beb5afff1` and
`xpu-resume/qwen-decoder-comparison.json`. The XPU observer again sampled only the Intel xe device
and Level Zero library and reached the second final-logits checkpoint.

The installed 1,842,216,416-byte Qwen forced-aligner container was then read-only tested with its
separate 1,024-wide encoder and 5,000-class head. One synthetic mel row plus two selected
classification rows produced 11,024 finite CPU/XPU float pairs and one identical position. Encoder
maximum absolute error was `8.34465026855e-7` (NMSE `9.28168271953e-14`); classification-logit
maximum was `9.17911529541e-6` (NMSE `6.01949681613e-13`). Per-row argmax matched at 202 and
1,702. The recorded XPU process reached final logits and again sampled only Intel Level Zero and
xe PCI `0000:07:00.0`; peak resident VRAM was 3,978,904 KiB. CPU operation:
`20260910T182029-11ad2ef9116d`; XPU operation: `20260910T182102-4601fe611d8d`; full comparison:
`20260910T182137-6223c2007e24` and `xpu-resume/qwen-aligner-comparison.json`. Synthetic token IDs
and zero mel do not establish real-word timestamp alignment.

This verifies bounded native FCPE plans plus Qwen strict encoder/decoder and aligner-core execution.
It does not establish real-audio encoder parity, a real transcription/token sequence, real-word
alignment, all eighteen XPU resources, fused attention availability, Super scheduling or production readiness. The historical
OpenCL-dependent fused-SDPA failure was not rerun or repaired here. Next work uses recorded,
non-retrying model-specific real-audio/conditioning checks and explicit attention dependency
diagnosis. Successful exit does not establish later host stability.

## All-resource real full-song XPU validation — authorized 2026-09-11

**FULL-SONG EXECUTION COMPLETE; production/routing not qualified.** The user authorized all models' real full-song XPU execution and
fixing diagnosed problems before continuing. The current catalog and native
factory both contain **eighteen** resources, including the independent XE90
instrumental checkpoint. Earlier seventeen-resource counts and the earlier
ROCm RMVPE failure are historical: `901c134` implements native fused GRU cells,
and the recorded subsequent 30-second ROCm run completed with 3,001 frames.
Neither correction establishes current XPU qualification.

Use the existing real Chinese song `崔子格 - 卜卦.flac` read-only, so STARS can
receive genuine transcript-derived Chinese phonemes rather than invented
conditioning for the previous Japanese song. Keep diagnostics, decoded scratch,
new native builds and every output in `test-artifacts/libtorch-xpu-fullsong-real/`.
The authorization receipt is `20260911T063344-cba0c64793b8`.

Plan: complete reusable native ASR/alignment host adapters and an isolated Rust
real-audio diagnostic entry; run all eighteen resources serially on explicitly
selected B580 XPU; feed actual separated vocals, RMVPE pitch and ASR/alignment
outputs into dependent models; retain complete tensors/evidence and lossless
stems. Record host/GPU load before each execution and continuous observations.
Diagnose failures, commit each independent repair, run focused checks, then
continue with new evidence directories. No blind retries, synthetic lyrics/F0,
CPU/GGML inference fallback, installed-asset replacement, source mutation,
Vulkan Super restart, counter pressure or power/clock changes. This is model
qualification work, not authorization to claim Studio routing or production
readiness. Missing completion records remain unknown.

Outcome: **18/18** completed the full 216.88-second source on B580, including
full Qwen/FireRed decoding, real forced alignment and real conditioned STARS/ROSVOT.
Native speech adapters, RMVPE unvoiced conditioning, complete Chinese G2P data,
PolarFormer equal-width fused attention and actual 32-bit FLAC publication were
implemented and verified. Harmony also passes its recorded earlier 12-second
failure input under current code. Complete output/publication evidence is in
`test-artifacts/libtorch-xpu-fullsong-real/summary.json` and `flac-verification.json`.
All 193,404,960 decoded publication values were checked; no current publication
clips. Final focused Rust checks: 123 passed, seven explicitly ignored.

The first PolarFormer full-song attempt reported device loss and retains its
missing outer completion record; current corrected execution does not establish
driver root cause or host stability. Qwen has English guesses and 128 unresolved
word timings; STARS uses a separate actual FireRed Chinese alignment branch with
46 unresolved words. No invented conditions, model quality claim or automatic
primary-provider substitution follows. Full details, exact commits/operations,
timings and remaining integration work: [full-song results](../../LIBTORCH_XPU_FULLSONG_RESULTS.md).

## AMD ROCm 10 scoped validation — lightweight models 9/9 passed 2026-09-11

**THE USER-SCOPED LIGHTWEIGHT LANE PASSED NINE OF NINE REAL TWELVE-SECOND RUNS.** The authorized isolated environment resolves the official AMD stable
combination, **ROCm 10.0.0 + PyTorch 2.13.0**, with the Radeon 780M `device-gfx1103` package. The
repository's pinned ROCm 7.2.3 is not evidence for this lane. The private environment remained under
ignored test evidence and did not alter the system driver, global Python, installed model store,
source media or prior runtime. Authorization receipt: `20260911T094255-176d942db7f9`.

Native build and synthetic device contracts passed. The official experimental AOTriton setting was
initially required to make gfx1103 fused SDPA execute. Both Leap-width fused-attention oracle shapes
then passed; the finite unequal-width PolarFormer result missed the existing NMSE threshold. The first real
resource, `bs_roformer_leap_xe90_vocals`, still failed without fallback: a synchronized attempt
located an unspecified launch failure after first-layer frequency feed-forward at about 4.70 GiB
sampled GTT. A separately committed row-tiled feed-forward retained complete outputs and passed six
GPU-to-double-CPU oracle cases.

Executing that change against the real twelve-second excerpt triggered an observed brief blackout
and recovery of the AMD-connected display. Passive host records sampled `amdgpu-reset-dev` work for
about four seconds. The target never advanced past zero of two chunks and, after the reset, consumed
nearly one CPU until manually terminated following the harness timeout. The child observer records
`SIGTERM`, 1,841.906 seconds and about 3.81 GiB peak sampled GTT; the timed-out outer recorder has no
completion record. No result artifact was published. Evidence is under
`test-artifacts/amd-libtorch-rocm10/bounded/`, especially
`observations/leap-vocals-feed-forward/` and
`diagnostics/leap-display-reset-sample-summary.json`.

The user then authorized one memory fix and one single-chunk retest. Commit `37eef2a` partitions the
complete RoFormer attention block along independent batches, bounding normalization, QKV, rotary,
full-context SDPA, gate and output intermediates without partitioning any key/value sequence. The
9.000-second real excerpt is below the Leap overlap-add step and produced exactly one full-size model
chunk. It completed in 32.614 seconds at 1,823,576 KiB peak sampled GTT. Both signed-32-bit FLAC stems
fully decode to 793,800 finite, non-silent values; exact float-stem reconstruction differs from the
mix by at most `5.960464477539063e-8`. No `amdgpu-reset-dev` worker appeared in the in-run or immediate
post-run passive samples.

The sysfs AMD busy value was already 99% before launch and remained 69–99% through observation and
99% afterwards despite no visible pre-run compute owner. Consequently this is a functional
single-chunk pass, not a clean throughput measurement or proof that the earlier reset/display fault
is resolved. The original twelve-second sweep remains zero of eighteen and full-song execution was
not started. Evidence: `test-artifacts/amd-libtorch-rocm10/single-chunk/`.

The user subsequently authorized resuming all eighteen twelve-second resource checks; receipt:
`20260911T110231-4ad310e0db43`. Corrected Leap vocals, the independent Leap instrumental checkpoint
and PolarFormer passed, each with complete finite twelve-second stems and float-epsilon residual
reconstruction. Execution times were 64.153, 64.035 and 45.717 seconds; sampled peak GTT was
1,823,576, 1,823,576 and 1,489,372 KiB respectively.

Denoise then ran against the actual Leap guide-vocal publication. It completed five of six chunks
before `SIGBUS` at 34.978 seconds and published no result. Its sampled GTT peak was 1,807,832 KiB,
not the former approximately 4 GiB peak. The operator observed another brief blackout/recovery of
the AMD-connected display, while the last passive sample records `amdgpu-reset-dev`; AMD busy stayed
76–99% through the run. No later sweep resource was launched. Evidence:
`test-artifacts/amd-libtorch-rocm10/bounded-resumed/`.

A subsequent shared-risk review found that six RoFormer, three GAME and two Qwen resources could
select the same experimental fused SDPA family on ROCm. Commit `0fa8110` replaces all production
ROCm mixed-attention calls with explicit GPU FP16 input rounding, bounded FP32 query-tiled
contractions/softmax and FP16 output rounding. It preserves complete K/V context, additive and
boolean masks, causal layout and GQA. The shell no longer enables experimental AOTriton. Three
RoFormer geometries and direct GAME-mask, Qwen-mask/GQA and causal cases passed **6/6** against
complete rounded-input double references; the observer sampled `amdgpu 0000:10:00.0`, about
292,644 KiB peak GTT, exit zero and no experimental fused kernel.

The authorized real Denoise retry disproved the hypothesis that AOTriton was the sole reset cause.
Preflight AMD use was 2%; the model loaded and reported zero of six chunks, then received `SIGBUS`
after 5.619 seconds without publishing a result. Peak sampled GTT was 1,818,668 KiB and the final
host sample records `kworker/u64:6+amdgpu-reset-dev`. No task-owned model process remained. This is
the third recorded AMD display-reset incident in this lane. Evidence:
`test-artifacts/amd-libtorch-rocm10/repair-review/denoise-retry/`.

Offline model inspection found that the private Denoise mask estimator bypassed projection tiling:
for each band it could submit an `801 x 1536` by `1536 x 1536` GEMM, about 1.89 billion
multiply-accumulates. Commit `8268ab9` routes those layers through the common projection helper,
initially bounded each ROCm GEMM to 268,435,456 multiply-accumulates, and added actual-shape oracle
and trace coverage.

The user explicitly directed same-boot continuation after preflight found the attached `amdgpu`
driver and 2% AMD use. The Denoise square case passed all 1,230,336 values at row tile 113. The
next `60000 x 384` by `384 x 1536` projection received `SIGBUS` after 4.371 seconds and the final
sample records `amdgpu-reset-dev`; sampled peak GTT was 688,644 KiB. That shape had passed earlier
at row tile 1024 and comparable 692,616 KiB GTT, but the first work formula reduced it to tile 455
and increased submissions from about 59 to 132. Denoise itself was not launched. Evidence:
`test-artifacts/amd-libtorch-rocm10/mask-projection-resume/projection-check/`.

Commit `4fc2739` corrected this over-partitioning: the previously passed
`1024 x 384 x 1536` contraction defines the submitted-work bound, so transformer projections retain
row tile 1024 and private square mask projections use tile 256. The full projection/FFN oracle then
passed **7/7**, including the square mask and 60,000-row cases, with no active reset worker sampled.

A synchronized Denoise attempt completed band split and first-layer normalization before reporting
an unspecified launch failure at the first time-attention QKV checkpoint; that attempt had no
sampled reset worker. Commit `e0da4a8` made tiled projections write directly into their destination
through `mm_out`/`addmm_out`, eliminating temporary linear outputs and copies. Its expanded oracle
passed **8/8**, including all 9,842,688 values of the exact synthetic
`6408 x 384 -> 1536` QKV shape. Real Denoise still failed at the QKV synchronization surface, and
that attempt's final host sample did record `amdgpu-reset-dev`.

Trace-only tile synchronization in `b4bdb20` then showed that the label was not stable causality.
The next Denoise run failed before any QKV or projection-tile checkpoint, while `stack/cat` allocated
the combined representation after sixty asynchronous band-split projections; its final host sample
records a separate `amdgpu-reset-dev`.

Commit `7934e08` replaces the ROCm band-split lifetime pattern with one preallocated contiguous
`[band,time,channel]` destination. Each band GEMM writes directly into its slice, eliminating sixty
retained outputs and the final stack allocation/copy; trace mode synchronizes each band. The next
Denoise trace completed all **60/60** band projections and the combined band-split checkpoint.

That trace then completed QKV projection rows `0..1023` and `1024..2047` before an unspecified launch
failure on `start=2048, rows=1024`; the final sample records another `amdgpu-reset-dev`. The actual
QKV input shape is `[8,801,384]`, so commit `b26ca72` kept each complete 801-row sequence together.
Its exact three-dimensional projection/FFN oracle passed **8/8**, including all 9,842,688 QKV values,
with no active reset worker sampled.

Real Denoise still completed all sixty band projections and reset on the first batch-aligned
801-row QKV tile. Cross-batch tile boundaries are therefore not causal. Commit `e8b06e9` instead
bounds each complete ROCm attention subproblem to at most 1024 projected sequence rows: long
801/1722-row time attention runs one independent batch at a time, while short frequency attention
retains up to eight. Complete K/V context and arithmetic are retained while simultaneous
normalization, QKV and attention intermediates shrink. The native build passed. A real trace subsequently completed 44 long-axis batches before the next
QKV launch failure. Separating Q, K and V projections, valid serialization settings, stage fences,
pacing, smaller row tiles and reduction tiling moved failures among projection, normalization and
attention stages without producing a repeatable pass. One fenced execution completed all chunks,
but its immediate reproduction reset and therefore was not accepted.

The accepted repair replaces the remaining high-volume RoFormer contractions on ROCm. An
in-process HIPRTC F32 kernel supports contiguous and arbitrary two-dimensional projection strides;
it now executes RoFormer projections and FFNs. The same kernel supports independent head/batch
strides and executes both explicit mixed-attention contractions. This retains FP16 attention
input/output rounding, FP32 scores and softmax, complete K/V context, masks, causal behavior and
GQA. Production-scheduled projection/FFN oracles pass **10/10**, including complete 60,000-row and
strided Denoise geometries against double references. Attention oracles pass **6/6**. Every custom
contraction is explicitly synchronized and paced. The shell no longer exports
`AMD_SERIALIZE_KERNEL=3`, which PyTorch rejected as an invalid boolean.

The initial custom-kernel model still reset after the old row-bounded workaround multiplied
attention dispatches. Commit `2809b81` restores groups of at most eight independent batches while
keeping every projection internally bounded to 256 rows and preserving complete sequence context.
The exact real Denoise request then completed **6/6 chunks twice** on `gfx1103`: traced execution was
504.331 seconds and an independent non-trace reproduction was 501.916 seconds, each with 1,778,396
KiB peak sampled resident GTT. Both outputs contain 1,058,400 finite samples, peak `0.8360749483`,
RMS `0.1319012501`, zero clipping/out-of-range values and zero reconstruction error; their exact F32
files are byte-identical. Maximum sampled edge temperature was 36 and 39 degrees Celsius. Evidence:
`test-artifacts/amd-libtorch-rocm10/grouped-attention/denoise/` and `reproduction/`.

Denoise is now a reproduced bounded functional pass.

The user then excluded all six audio separation/cleanup resources, Qwen ASR, the Qwen aligner and
FireRed from further AMD execution. The resulting lightweight scope passed **9/9** real twelve-second
runs with explicit `libtorch_rocm`: FCPE `0.403 s`, RMVPE `0.408 s`, Basic Pitch `0.306 s`, GAME
small/medium/large `7.137 / 10.114 / 19.122 s`, dual-input JBM555 `0.972 s`, ROSVOT `0.712 s`, and
STARS `1.387 s`. All process exits were zero, observed boots were unchanged and all numeric evidence
values were finite. The result includes complete frame/note evidence and STARS technique/style heads.

Conditioned models used this sweep's AMD RMVPE output and a retained previous XPU alignment over a
decoded-waveform-identical twelve-second source; no Qwen model executed in this scope. A first ROSVOT
setup invocation rejected the product alignment representation before inference. The isolated native
representation retained all measured intervals and timing issues, after which ROSVOT and STARS used
18 resolved words and excluded five unresolved words as designed. Postflight reported AMD use at 0%
and no task process remained. Evidence:
`test-artifacts/amd-libtorch-rocm10/lightweight-sweep/summary.json`.

The active requested scope is complete at **nine passed of nine**. No full-song execution was started.
CPU/GGML fallback remains prohibited. Previous XPU and historical audio results remain separate, and
this work establishes no driver root cause, broad host stability, acceptable throughput, product
routing, whole-model parity, listening quality or production readiness.

## Product route — implemented and selectable (2026-09-11)

The user authorized promoting the native LibTorch XPU implementation to a production route on
2026-09-11. It is now wired through every boundary without changing the process architecture:

- **Runtime Manager** catalogs a second runtime resource, `runtime:libtorch_xpu`, whose executable
  component is the same packaged `uta-ggml-worker`. Every model advertises a production-pinned
  `libtorch_xpu` capability (`evidence_id: validation:libtorch-xpu-fullsong-real-2026-09-11`) beside its
  pinned `ggml` default and depends on both runtimes; `resolve_model_with_backend(..., libtorch_xpu)`
  resolves the same model files with `runtime_id: libtorch_xpu`. The runtime is usable only when
  `lib/libuta_libtorch.so` exists under the installed runtime directory (`UTA_STUDIO_LIBTORCH_RUNTIME_DIR`,
  else `<runtime store>/libtorch-xpu`); otherwise it reports `native_library_missing` and every LibTorch
  selection fails closed. `native-inference/runtime-lock.json` names both selectable runtimes; GGML remains
  the pinned default and no backend ever falls back to the other.
- **Analysis Engine** maps a resolved `libtorch_xpu` model to worker backend `libtorch_xpu` on the discrete
  GPU, rejects CPU/iGPU device classes for it, records `backend: libtorch_xpu` / `device: xpu` provenance in
  the result fingerprint, and accepts `libtorch_xpu` in every typed artifact validator.
- **Worker** (`native-inference/ggml-worker/src/libtorch.rs`): a task whose config names `backend:
  libtorch_xpu` validates `runtime-manifest.json`, applies the manifest's declared process environment
  (`ONEAPI_DEVICE_SELECTOR`, Level Zero driver path, SYCL kernel cache, strict oneDNN math), loads the
  native library through the existing `uta-libtorch-runtime` C ABI, opens the model on the explicit XPU
  device with the qualified precision policy (mixed attention for the six separators, strict elsewhere)
  and writes the same raw engine outputs as the GGML route, so publication stays shared. Each model
  module's request parsing and evidence publication is a backend-agnostic `infer_with`; LibTorch result
  structs are converted field-for-field. Super-acceleration preloads are never attempted for LibTorch.
- **Studio**: Settings > Models & runtime has a *Compute backend* selector (pinned default / GGML Vulkan /
  LibTorch XPU) persisted as `compute_backend` and sent as the exact `requested_backend`; per-model runtime
  menus list the advertised LibTorch capability and per-model overrides accept `libtorch_xpu`. An
  unavailable backend fails in Plan Preview.
- **Installation** is Python-free: `native-inference/libtorch-runtime/install-libtorch-xpu-runtime.sh`
  fetches the official `torch 2.13.0+xpu` wheel archive and its pinned native dependency wheels with
  curl (bounded parallel byte ranges), unpacks only shared libraries and C++ headers with unzip, builds
  `libuta_libtorch.so` with CMake inside `bash dev.sh`, and writes `runtime-manifest.json` with library
  digests and the machine's driver environment. The runtime recipe is
  `native-inference/libtorch-runtime/runtime-recipe.json`.

### Production verification on the 12-second excerpt (2026-09-11)

**12-second production verification (2026-09-11, `test-artifacts/libtorch-xpu-production-12s/`):**
the real `uta-analyze analyze` production request (balanced profile, full candidate chart, no lyrics,
`requested_backend: libtorch_xpu`) over the 12.000-second excerpt of 崔子格 - 卜卦 completed end to end
**outside the development shell**, with the Engine applying the installed runtime's declared process
environment to the worker. Operation `20260911T125308-42836a546e2e`: status `ok_degraded`
(`alignment_unresolved_words:5`), every resource resolved and executed as `runtime libtorch_xpu /
backend libtorch_xpu / device xpu` (Leap XE90 vocals, Qwen3 ASR, Qwen3 forced aligner, RMVPE), and it
published the candidate chart, pitch evidence, singing analysis, transcript, alignment and both
12.000 s / 44.1 kHz stereo FLAC stems. Typed evidence carries `backend: libtorch_xpu`. A warm-cache
repeat (`20260911T125817-c6ced5c7c4ff`) and the pinned GGML Vulkan reference on the same request
(`20260911T125624-62b05f34f2f4`, inside the dev shell) produced the identical transcript, the same
23/5 alignment items, the same 872/1201 voiced frames and the same empty candidate note track;
pitch differs from GGML by at most 0.155 Hz / 0.003 confidence and between the two LibTorch runs by
6.1e-5 Hz. Engine wall (per-node lifecycle, including worker spawn and weight load) was 55.8 s cold /
54.3 s warm on LibTorch versus 48.5 s on GGML: separation was faster (6.4 s vs 9.1 s) while the Qwen
and RMVPE nodes were slower on this short slice. Other user processes (a parallel AMD ROCm session)
were active, so this is a functional pass, not a controlled benchmark; whole-model parity, listening
quality, host stability after exit and Nix packaging of the runtime are not established. Five
diagnostic operations preceded the pass (missing `libz.so.1`, a compute-runtime abort under
`ZE_ENABLE_ALT_DRIVERS`, the Engine's existing-output-directory requirement, oneDNN writing to stdout
with the OpenCL loader unresolved, and a 32-bit OpenCL loader); each fix is a separate commit
(`f0062fc`, `e6884b2`, `ccaacee`). Details: `summary.json` in the evidence root.

Environment findings that shaped the route: the Intel Level Zero loader must discover the GPU driver
through the process library search path (`LD_LIBRARY_PATH` declared in the runtime manifest and
applied by the Engine; `ZE_ENABLE_ALT_DRIVERS` aborted inside the compute runtime), oneDNN opens the
OpenCL ICD loader by name at run time and prints its verbose error report to stdout (the worker now
claims its protocol stream before loading native code), and the packaged GGML libraries still need
the shell library path for `libstdc++` when the development binaries run outside `bash dev.sh`.

## Source references

- Local measured motivation: `docs/ROFORMER_B580_LIBTORCH_XPU.md`.
- Current model inventory and earlier reference evidence: `tasks/remaining-models/STATE.md`.
- Execution provenance and host observation: `docs/ROFORMER_OPERATION_RECORDING.md`.
- PyTorch native C++ interface: https://docs.pytorch.org/cppdocs/
- PyTorch ROCm/CUDA-interface semantics: https://docs.pytorch.org/docs/main/notes/hip.html

Exact runtime/model execution results and remaining blockers are recorded in the linked full-song report; none are inferred from package availability or historical operator measurements.
