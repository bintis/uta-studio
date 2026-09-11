# Uta! Studio — Key Technical Conclusions

**Updated:** 2026-09-11

## Authority

- `AGENTS.md` defines repository-wide execution, architecture, safety, and handoff rules.
- `docs/design/README.md` links the current architecture documents.
- `tasks/remaining-models/STATE.md` is the current model/task index.
- Historical evidence remains scoped to the exact source, model bytes, backend, device, and command that produced it. It does not override current source.

## Runtime architecture

- Studio communicates with packaged `uta-analyze` and `uta-runtime` machine protocols. `app-core/**` and `desktop/**` do not import backend implementation crates.
- On 2026-09-11 the real production request over the 12-second excerpt completed end to end on the LibTorch XPU route outside the development shell (`test-artifacts/libtorch-xpu-production-12s/summary.json`): identical transcript and alignment counts to the GGML reference, pitch within 0.155 Hz, 55.8 s cold / 54.3 s warm engine wall versus 48.5 s GGML on that short slice (separation faster, Qwen/RMVPE nodes slower, other processes active). Functional pass only; not a controlled benchmark or parity qualification.
- The packaged product has two explicit native routes. The pinned default uses Rust-owned graphs over upstream GGML shared libraries; since 2026-09-11 the native LibTorch XPU implementation is a second production-pinned route (`runtime:libtorch_xpu`, worker backend `libtorch_xpu`) selectable globally or per model in Studio settings and executed by the same packaged worker. Selection is explicit and never falls back; a missing installed LibTorch runtime fails closed. See [execution scope and product route](design/runtime/LIBTORCH_EXECUTION.md).
- The GGML runtime package contains upstream `libggml.so.0`, `libggml-base.so.0`, `libggml-cpu.so`, and `libggml-vulkan.so` plus its declared backend patches and manifest. The independent LibTorch implementation uses native ATen C++ plans; it is not a GGML ABI shim or a product-time script runtime. The offline STARS lexicon exporter handles linguistic data only; container migration remains the Rust `cargo xtask gguf` command.
- CPU is an explicitly selected experimental reference lane. Vulkan remains the default; a GPU or integrated-GPU request fails closed if its requested device cannot initialize or execute and never falls back to CPU.
- `native-inference/gpu-probes` performs read-only Vulkan enumeration for diagnostics and descriptor matching. It does not create a Vulkan device or execute inference.
- `uta-ggml-worker` executes Rust model implementations in-process. Its FFmpeg calls are audio codec operations, not model inference subprocesses.
- Runtime and model directories are user data. Tests use isolated fixtures and do not delete or replace configured assets.

## Super acceleration scope

The user's clarified goal is **whole-model task scheduling across GPUs**, based on dependencies,
loading order and predicted total execution time, not splitting one model's chunks across cards.
Keep actual model hot loading, exact-format audio decode reuse, shared separation outputs and
useful final-consumer device residency. The earlier chunk-splitting measurements do not qualify
this corrected design. Task-level orchestration remains incomplete and Vulkan Super validation
remains paused after the reported restart. Existing synchronization, resource lifetimes and cleanup remain;
process success and available VRAM do not establish host safety. See
[Super acceleration](design/runtime/SUPER_ACCELERATION.md) and the task index.

The authorized implementation now shares validated audio/PCM ownership, CPU frontend preparation,
real Qwen window progress and joined task context; it also reuses FCPE/GRU graphs, RMVPE resident
handoffs and Qwen incremental arenas. Read-only weighted CPU fixtures compared 470,520 finite
RMVPE/FCPE values bit-for-bit across differing windows. This is bounded CPU numerical evidence,
not automatic dual-GPU scheduling, GPU qualification or a measured whole-analysis speedup.

A later explicit request resumed separate LibTorch XPU testing. The native ABI fixture and bounded
real-weight FCPE tensor calls completed; 179,640 full CPU/XPU activation pairs were finite, with
maximum absolute error 6.29370333627e-10. Repeated XPU outputs were not bit-identical. Qwen ASR
strict XPU also completed a real 12-second encoder call (156 x 2,048 finite values) with every traced
stage synchronized. A separate synthetic-mel encoder/KV-cache/two-step decoder check compared all
305,920 CPU/XPU float values and three positions; complete-logit argmax decisions matched, with
maximum logit difference 1.09672546387e-4. The forced aligner's separate encoder/classification
core also compared all 11,024 float values; both selected-row argmax decisions matched and maximum
logit difference was 9.17911529541e-6. The XPU observers sampled only Intel Level Zero and xe PCI
`0000:07:00.0`. This does not qualify real-audio output parity, a real transcription/alignment, all
models, Super scheduling or speedup; see [LibTorch execution](design/runtime/LIBTORCH_EXECUTION.md).

Subsequent authorized native RoFormer optimization fuses XPU ordinary rotary
arithmetic and FP32 RMS normalization, and preserves head-interleaved SDPA
operands to avoid copies. Full-axis primitive comparisons and a complete
12-second XE90 waveform comparison pass; final waveform max difference is
8.35955143e-6, SNR 111.2070 dB, not bitwise equality. Isolated rotation improves
from roughly 14–15 ms to 2.2 ms per call, **not a whole-model speedup**. Other
GGML work contaminated later timings; the matched full-song pair and other
RoFormer real-audio geometries are prepared but unexecuted. The 60-second
whole-song goal is unverified. Installed assets and precision/context/chunk/
synchronization semantics remain unchanged. See the final section of
[Native XPU comparison](ROFORMER_B580_LIBTORCH_XPU.md).

## Current executable models

The Runtime Manager catalog contains eighteen models and two native runtimes. Every model pins the `ggml` backend, depends on both `ggml_vulkan` and `libtorch_xpu`, and additionally advertises a production-pinned `libtorch_xpu` capability that is used only when explicitly requested.

| Model | Capability |
| --- | --- |
| `bs_roformer_leap_xe90_vocals` | `audio.extract_vocals`, `audio.extract_instrumental` |
| `bs_roformer_leap_xe90_instrumental` | `audio.extract_vocals`, `audio.extract_instrumental` |
| `bs_polarformer_public_instrumental` | `audio.extract_vocals`, `audio.extract_instrumental` |
| `melband_roformer_harmony` | `audio.lead_isolate` |
| `melband_roformer_denoise_aufr33` | `audio.denoise` |
| `melband_roformer_dereverb_anvuew` | `audio.dereverb` |
| `rmvpe` | `pitch.track` |
| `fcpe` | `pitch.secondary`, `pitch.secondary.fcpe` |
| `basic_pitch` | `notes.basic_pitch` |
| `game_1_0_3_small` / `game_1_0_3_medium` / `game_1_0_3_large` | `notes.game` |
| `jbm555_cectc_80` | `notes.jbm555` |
| `stars` | `notes.stars`, `technique.analyze` |
| `rosvot` | `notes.rosvot` |
| `firered_asr2_aed` | `speech.transcribe.challenger` |
| `qwen3_asr_1_7b` | `speech.transcribe` |
| `qwen3_forced_aligner_0_6b` | `speech.align` |
| runtime `ggml_vulkan` | shared-library execution |

`stars` and `rosvot` declare an `rmvpe` dependency because they are conditioned on tracked F0. FireRed declares a three-file named artifact set (`firered-f32.gguf`, `cmvn.ark`, `dict.txt`) rather than letting the worker guess sidecar paths.

Leap XE90 is the default separation strategy. One invocation emits guide vocals and computes the instrumental as the mixture residual. PolarFormer is an explicit experimental strategy, not an independently scheduled default Instrumental pass.

Public MVSep Multisong context at the time of selection:

| Model | Vocals SDR | Instrumental SDR |
| --- | ---: | ---: |
| Leap XE90 | 11.7615 | 18.0689 |
| PolarFormer Public | 11.7575 | 18.0650 |
| MelBand-RoFormer Inst V2 | 10.5374 | 16.8448 |
| BS-RoFormer 124-band | 12.3339 | 18.6414 |
| BS PolarFormer 124-band | 12.0230 | 18.3304 |

The 124-band leaderboard entries do not have an accepted public runnable artifact, so benchmark rank alone does not create a product resource.

## Current execution evidence

- **AMD ROCm 10 twelve-second validation remains halted at three passed, one failed and fourteen not run after eight code-triggered GPU resets; full-song was not started.** Leap vocals, independent Leap instrumental and PolarFormer passed. AOTriton removal, band preallocation and corrected projection scheduling pass their complete GPU oracles but have not prevented Denoise resets. The batch-aligned `[8,801,384]` QKV oracle passed all 9,842,688 values; real Denoise still reset on its first 801-row QKV tile, falsifying cross-batch boundaries as the cause. Commit `e8b06e9` now bounds each complete ROCm attention subproblem to at most 1024 projection rows, reducing simultaneous long-axis intermediates while retaining complete K/V context; it builds but is not GPU-verified. Further AMD model, oracle or stress execution requires a new explicit human decision; no CPU/GGML fallback is allowed. See [execution scope and incident](design/runtime/LIBTORCH_EXECUTION.md) and `test-artifacts/amd-libtorch-rocm10/batch-aligned-projection/`.
- **Latest all-resource result (2026-09-11): 18/18 completed real 216.88-second full-song LibTorch XPU execution on B580**, including actual full transcription/alignment and real conditioned STARS/ROSVOT. Native speech adapters, STARS lexicon completeness, RMVPE unvoiced conditioning, PolarFormer fused-attention geometry and actual 32-bit FLAC publication are implemented and tested. All 193,404,960 current publication values decode correctly with zero clipping. Final focused Rust checks: 123 passed, seven ignored. Original PolarFormer DEVICE_LOST and missing outer completion remain recorded; corrected full-song execution succeeds, but driver root cause/host stability are not established. Harmony also passes its recorded earlier failure input on current code. ASR accuracy and unresolved word timings, whole-model parity/listening, Studio routing and production qualification remain open. This supersedes the earlier bounded-only XPU/family execution gaps below, not their precision or historical performance limitations. See [full-song report](LIBTORCH_XPU_FULLSONG_RESULTS.md) and `test-artifacts/libtorch-xpu-fullsong-real/summary.json`.
- Authorized CPU follow-up now measures frontend/native boundaries without additional per-operator fences. Native compute dominates the 73.457-second profiled song (68.628 s); transferring all CPU DSP cannot reclaim the roughly one-core process CPU consumption. Bounded CPU clocks locate 1.968515 CPU seconds in the original 1.97310-second completion wait. Current `0b76748` waits on a non-profiling XPU event with brief sleeps, retaining the original full-device synchronization and error/cancellation/precision semantics. The steady candidate chunk uses 0.045115 CPU seconds in 1.93321 s forward wall. All 1,058,400 waveform values are finite and compared (143.13503 dB, max 2.384185791e-7). Exact-bit CPU packing improves about 12.5 → 9.1 ms locally; regressed mask tiling is withdrawn. CPU ABI/primitives, 59 LibTorch tests and four GGML frame tests pass. The complete-song pair reduces sampled process CPU **75.73 → 11.51 s (84.80%)**, or **98.34% → 14.25% of one logical CPU**. Inference is **74.58065 → 77.56927 s** under differing compiler/render contention: no throughput gain is demonstrated, and uncontended wait latency remains open. All 31,300,416 samples compare at **137.84049 dB**, max **1.102685928e-6**. Three additional family numerical checks pass against retained active outputs; original Harmony/PolarFormer failures were not retried. Final formatting and 59 + four + four targeted Rust tests pass. No pure-GPU migration, sub-60, listening, counter-utilization or host-stability claim; nothing installed or further queued. See `docs/ROFORMER_B580_LIBTORCH_XPU.md`.
- Earlier read-only RoFormer CPU/XMX review (2026-09-11): the retained full-song process used **98.86% of one logical CPU**, not the whole 16-logical-CPU machine (host average 11.86%). No stack profile separates host DSP from submission/synchronization. STFT, packing, mask reconstruction, iSTFT and OLA are still on CPU; learned graph operations already run on GPU. Historical logs confirm 64 FP16 native SDPA calls, while matching source identifies the systolic-capable route—not per-XMX execution/occupancy measurement. First retain existing native transfer/compute timings and measure host phases; pure-GPU frontend benefit remains unmeasured. No new GPU execution or counter stress. Details: `docs/ROFORMER_B580_LIBTORCH_XPU.md`.
- Further precision-first RoFormer ablation selects **64.52598 s** inference for the 354.88-second song (66.64771 s observed process), versus a fresh 74.56787 s control. All 31,300,416 waveform values are compared: **144.17758 dB** SNR, max difference **3.576278687e-7**. This retains FP32 projection/normalization/residual and existing FP16 attention, not bitwise or listening qualification. FP32 GELU/residual post-op trials are removed: the GELU variant reached 62.11233 s under CPU compiler contention but its full-song difference was much larger (79.33864 dB); the residual variant was slower. An arithmetic-free paired value-copy candidate passes full-axis exact half-storage-bit checks, but its 65.16225-second full-song run was CPU-contended and did not establish an incremental model win; it remains diagnostic-only. Axis-view normalization showed no material gain and model copies remain. Current source builds and passes CPU primitives/ABI plus 55 Rust tests; original Harmony nonfinite masks and PolarFormer OOM still block family-wide acceptance. No under-60 or production/safety qualification. See `docs/ROFORMER_B580_LIBTORCH_XPU.md` and `fusion-ablation-comparison.json` in the speed evidence root.
- The resumed native RoFormer XE90 full-song pair completes 354.88 seconds / 38 chunks in **114.23105 → 72.53718 seconds**: **36.50% less inference time, 1.575× speedup**, still above the 60-second target. Existing precision policy and complete context/chunk/overlap are retained. Every waveform sample was compared: SNR **83.26265 dB**, max difference **0.0006777942**, not bit-identical or listening-qualified. A separate TF32 allowance showed no useful speedup (73.69561 / 73.18288 seconds under differing CPU load), and actual low-precision hardware execution was not proven. The user retired TF32; its experimental entry point is removed. Further work targets precision-preserving fusion/layout while retaining synchronization and bounded serial observation; prior GPU power-loss cause is unresolved. XE90 vocal/instrumental family executions completed; original PolarFormer control hit native-SDPA OOM, so family-wide acceptance remains open. Details and evidence: `docs/ROFORMER_B580_LIBTORCH_XPU.md`, `test-artifacts/libtorch-roformer-speed/`.
- An isolated native LibTorch XPU 2.13.0 operator comparison on B580, without production runtime/model changes, executes the full XE90 attention shapes at 34.29–38.17 effective TFLOPS for FP16 time attention under synchronized host timing. The final pair is 51.1056 ms for copied GGML versus 14.9838 ms for LibTorch; strict FP32 time-projection GEMMs improve by about 2.3–2.5x with sampled full-contraction FP64 NMSE around 1e-13. LibTorch's FP16 outputs are rounded before conversion to F32, unlike GGML's direct F32 output, so this is not identical arithmetic or model/audio parity. Two GGML noncontiguous F32 diagnostic cases expose an internal half-conversion route; explicitly timed F32 packing restores their precision. All 80 native process records are retained: 76 succeed, two expose that precision issue, and two earlier SDPA failures were resolved by exposing the existing OpenCL loader privately. Contended confirmation runs are not used for speedup claims. No model migration, torch.compile or AOTInductor result is established. Details: `docs/ROFORMER_B580_LIBTORCH_XPU.md`; evidence: `test-artifacts/libtorch-xpu-isolated/acceptance-summary.json`.

- RMVPE completed real AMD 780M execution. Against the historical same-source fixture, 601 frames aligned, one voiced decision differed, common-voiced F0 RMSE was 0.35158 Hz (maximum 5.27094 Hz), and confidence RMSE was 0.008684.
- FCPE completed isolated CPU and AMD 780M Vulkan execution. Both matched all 570 voiced decisions in a fresh OpenVINO reference over 601 frames. CPU/reference F0 RMSE was 0.10770 Hz (maximum 2.56958 Hz); Vulkan/reference RMSE was 0.17518 Hz (maximum 2.81871 Hz). A Maximum-profile Analysis Engine smoke also completed with status `ok`, 601 FCPE frames, and both RMVPE/FCPE resources in provenance.
- Basic Pitch has a Rust-owned upstream-GGML graph, typed worker route, catalog entry, and optional product route. Explicit CPU and AMD 780M runs each produced 516 frames with zero contour-class disagreements against the historical same-source CPU reference. AMD/reference maximum absolute differences were 0.00344658 for note activation, 0.00023550 for onset activation, and 0.00027374 for contour score.
- GAME 1.0.3 small, medium, and large share one Rust-owned upstream-GGML graph and typed worker route, and all three are selectable catalog resources. All three completed AMD 780M execution, and medium completed a 32-second two-chunk run with 97 ordered notes. Medium remains the default note provider.
- JBM555 has a Rust-owned dual-input frontend, upstream-GGML graph, decoder, and typed worker route. CPU and AMD 780M runs matched the historical reference's single note, range, and MIDI. AMD/reference maximum absolute score differences were 0.0001272 onset, 0.0001751 offset, and 0.00017691 pitch.
- Leap completed real AMD 780M execution. Against the former GGML reference, SNR was 60.36 dB, RMSE 0.00012209, and maximum absolute difference 0.00080027. Mixture reconstruction SNR was 122.10 dB.
- PolarFormer completed real AMD 780M execution with finite output and float-epsilon vocal+instrumental reconstruction.
- Harmony, Denoise, and Dereverb completed real AMD 780M execution with finite, exact-duration 44.1 kHz stereo output. Harmony's lead+residual reconstruction error was at float epsilon.
- STARS has a Rust-owned frontend, upstream-GGML graph, host decoders, and typed worker route. Stage C and Stage E were validated against checkpoint-truth oracles on CPU and AMD 780M, and the migrated GGUF loads on AMD 780M. End-to-end worker runs are recorded on CPU; a full AMD 780M worker run remains.
- ROSVOT has a Rust-owned graph and typed worker route, and completed both CPU and AMD 780M worker runs (`test-artifacts/rosvot-worker-cpu-20260908T2145`, `test-artifacts/rosvot-worker-amd-hardened-20260908T2153`). A reference comparison remains.
- Qwen3-ASR 1.7B completed a real AMD 780M full-transcription run and worker smoke. The Qwen3 Forced Aligner decoder was validated against the official reference on CPU and AMD 780M.
- Every current model except FireRed and Qwen ASR completed a real Intel Arc B580 run on 2026-09-09: fifteen of seventeen published their typed artifact with `backend: ggml_vulkan`, every published audio artifact was FLAC/44.1 kHz/stereo/12.000 s and non-silent, and the boot ID was unchanged across the sweep. Qwen ASR could not run because only the Q4_K_M artifact is installed and the catalog pins the F16 one. Evidence: `test-artifacts/b580-all-models-20260909T0910Z/`.
- The packaged Vulkan backend computed F32 matrix multiplication with F16-rounded operands, because upstream builds `pipeline_matmul_f32` from the coopmat or fp16 shader family whose `FLOAT_TYPE` is `float16_t`. `native-inference/ggml-worker/patches/0001-vulkan-keep-f32-matmul-in-f32.patch` rebuilds only that pipeline from the scalar `_fp32` shaders: a plain F32 `ggml_mul_mat` went from `2.388e-3` to `4.234e-7` on both the AMD 780M and the Intel B580, against `2.391e-7` on the CPU backend. Every F16, BF16 and quantised pipeline keeps the device's fast family. The runtime recipe declares the patch and its digest, so an unpatched runtime no longer validates. Upstream is not wrong to ship the original behaviour: its own `test-backend-ops` allows `MUL_MAT` an NMSE of `5e-4`, which suits quantised LLM weights but not F32 audio graphs that decode greedily.
- The earlier B580 XE90 H64 attention baseline introduced query-owned subgroups through backend patch `0007-vulkan-query-owned-attention.patch`: four native SIMD32 subgroups each own eight query rows, retain F32 matrix output fragments, and share coalesced K/V. A rebuilt seven-patch default measures 8.56 TFLOPS on the real time-attention shape. The final same-library six-second model pair measures 5.29 to 8.12 TFLOPS, 34.77% lower time-attention latency and an observed 7.24% lower second-pass model latency. Complete outputs repeat identically within a configuration, but old/new outputs are not bit-identical (72.05 dB SNR). Full context and existing precision are preserved; small-query/GQA capacity and other device/type paths remain unchanged. Twenty-seven explicit attention fixtures and 108 runtime/30 worker ordinary tests pass. An externally contended 130-ms return run is retained separately. No installed runtime was replaced; 20-TFLOPS, same-shape XPU, whole-song, cross-platform and release qualification are not claimed. Details: `docs/ROFORMER_B580_QUERY_OWNED.md`; evidence: `test-artifacts/xe90-subgroup-handoff-study/`.
- Follow-up backend patches `0008-vulkan-attention-fragment-workgroups.patch` and `0009-vulkan-attention-floating-storage.patch` extend that result: compact eight-subgroup K/V sharing and reused probability operands bring real-model XE90 time attention from 63.1003 to 43.0719 ms (8.66 to 12.69 TFLOPS), with bit-identical complete output against the seven-patch baseline. F32/mixed K/V staging also enables non-XE query-owned attention without graph casts or global conversion tensors: Harmony improves from 20.8147 to 8.2211 ms and Denoise from 20.6635 to 8.1390 ms; complete-output SNR is 67.04/74.42 dB, respectively, not bit-identical. The combined runtime passes 61 explicit numerical fixtures plus 110 runtime/30 worker ordinary tests. All nine recipe patches apply to the pin and reproduce the measured source byte-for-byte. Installed assets are untouched; 20 TFLOPS, whole-song, Windows, AMD/NVIDIA and Nix release acceptance remain unclaimed. Details: `docs/ROFORMER_B580_FLOATING_STORAGE.md`; evidence: `test-artifacts/attention-query-residency-study/`. The separately rebuilt eight-patch comparison (12.667 TFLOPS isolated, 12.664 TFLOPS in the real XE90 graph), complete byte comparisons and rejected register-handoff experiments are recorded in `docs/ROFORMER_B580_FRAGMENT_HANDOFF.md` and `test-artifacts/xe90-fragment-handoff-study/`.
- Backend patch `0010-vulkan-attention-compact-row-scales.patch` publishes eight independent F32 row scales per subgroup, removes one hot-loop score-overwrite barrier, and preserves floating/mixed K/V eligibility without model-name special cases. The nine-patch shape controls measure 43.916/43.010 ms versus 42.688/42.474 ms; the real XE90 model pair measures 43.4468 to 43.1572 ms (12.6644 TFLOPS for the candidate), not the isolated 12.8680-TFLOPS result. XE90, Harmony and Denoise retain byte-identical complete WAVs in both passes; small attention gains do not establish a consistent whole-model speedup. All 61 numerical fixtures pass under both default and four-subgroup selection, and 110 runtime/30 worker ordinary tests pass. Ten declared patches reproduce the measured source byte-for-byte. Spills/fills change from 20/112 to 18/122, not zero. Padded score/probability rows regress to about 53.8 ms and are not promoted. Installed assets and release qualification remain untouched. Details: `docs/ROFORMER_B580_COMPACT_RESCALE.md`; evidence: `test-artifacts/attention-compact-rescale-study/`.
- The accepted nine-patch snapshot is now installed at `~/.local/share/uta-studio/runtime/ggml-vulkan`, with the application's existing entry resolving there and the preceding runtime backed up. The installed Worker selects eight-subgroup query-owned attention without a runtime-directory override; both six-second FLAC stems are byte-identical to pre-installation output, and 61 explicit numerical cases pass (maximum NMSE `3.1445e-7`). The later ten-patch source recipe is not the installed snapshot. Continued key-width probes do not establish 20 TFLOPS: 64-key blocks regress, and the 16-key comparison has competing compute clients in its controls. A disjoint K/V scratch candidate (`80135be`) is compiled and SPIR-V-valid but has no established GPU numerical/performance result because its test invocation was blocked by the tool; it is not installed. An intervening host restart has unknown cause. Details: `docs/ROFORMER_B580_INSTALLED_THROUGHPUT.md`; evidence: `test-artifacts/attention-installed-throughput/phase-status.json`.
- FireRedASR2-AED now works end to end. Three defects had to be fixed: a dropped bias on the encoder's subsample output projection, upstream's F16 im2col in the subsampling convolutions, and the Vulkan F32 matmul above. It transcribes its reference fixture as `你好世界` on the CPU lane, the AMD 780M and the Intel B580, matching the historical implementation's tokens exactly.
- STARS, ROSVOT, and FireRed cannot load the GGUF containers currently installed for them: those are the pre-migration containers with PyTorch dimension order and, for STARS and ROSVOT, tensor names above GGML's 64-character limit. FireRed's installed file matches the `manifest_sha256` the catalog pins, so the catalog pins a container the runtime cannot open. The migrated containers exist only outside the model directories, and the repository has no Rust-owned way to produce them.

These are real execution and bounded comparison results. They are not broad listening qualification or strict whole-model parity. Current models remain `integration_ready=yes`, `production_ready=no` until the gaps in `tasks/remaining-models/STATE.md` are closed.

## Capability availability

Generated transcription and forced alignment are available again through Rust/upstream-GGML providers: `speech.transcribe` (Qwen3-ASR 1.7B), `speech.transcribe.challenger` (FireRedASR2-AED, optional and never baseline-required), and `speech.align` (Qwen3 Forced Aligner 0.6B). `notes.basic_pitch`, `notes.game`, `notes.jbm555`, `notes.stars`, `notes.rosvot`, and `technique.analyze` are likewise executable.

- `audio.lead_partition` is the one capability still reported as unimplemented. It is the lead/backing/harmony partition product feature, not a model backend gap.
- Caller-provided canonical lyrics remain valid input and still bypass generated transcription.
- An optional challenger that fails degrades the run with a recorded reason instead of failing it or substituting the primary provider.
- Inst V2 remains permanently retired: no catalog entry, graph, worker route, or fallback.
- Former C++/CLI/WGPU/OpenVINO implementations and catalog entries remain removed. Historical measurements for deleted routes remain historical only and must not be described as current readiness.

## Workflow contract

- Current Studio workflow schema is 7.
- Migration from schemas 1–6 retains executable model providers, removes retired-expert nodes, and restores the transcript, optional transcript challenger, alignment, and note-expert nodes that the GGML providers now implement.
- The default graph contains RMVPE, Maximum-only FCPE, Acoustic DSP, singing evidence fusion, Candidate graph, and canonical track, plus separation/lead/cleanup stages according to source and explicit workflow policy.
- Lyrics and alignment inputs remain optional for the Candidate graph. Asking the Engine to generate transcription or alignment now runs the pinned GGML provider; a request for a model that is not installed or whose device is unavailable still fails closed.
- Every separation execution invocation has typed dual outputs (`vocal`, `instrumental`) and one progress/failure identity.
- A capability with one executable provider has no fake model selector. Settings expose only the owning concepts and current provider facts.

## Audio and export

- Source media is read-only.
- Lossless audio is FLAC; lossy audio is MP3. Bytes, extension, and MIME must agree.
- Export remains atomic, validates extensions, never silently overwrites, and removes failed temporary output.
- Native audition remains the clock source. Cached compatibility previews are permitted only for unsupported source containers under the documented playback boundary.

## Verification boundary

Focused suites currently pass:

- Analysis Engine
- App Core
- Runtime Manager
- Desktop in the Nix development shell
- GGML runtime and worker focused suites from the current shared-library migration

Real AMD model smoke is recorded separately from tests. Final whole-workspace checks, product build, Nix packaging, identity scan, and packaged acceptance remain the explicit release pass.

## Remaining active closure

Model backend alignment itself is closed. What remains is qualification, not migration.

1. A full AMD 780M worker run for STARS and a full AMD 780M transcription run for FireRed.
2. Reference comparisons still missing per model: STARS/ROSVOT note evidence, PolarFormer, Denoise, Dereverb, and Harmony former-implementation vectors, and FCPE layerwise vectors.
3. RMVPE intermediate-tensor comparison around the remaining low-confidence onset difference.
4. Long-input performance and broader singing-material coverage for the newly promoted note and speech models.
5. Runtime manifest provenance reconciliation and final package payload validation.
6. Removal of retired model-specific artifact/schema code not intentionally retained for compatibility reads.
7. Intel B580 final smoke, then explicit release-pass checks and packaging.

## Full-pipeline debug observation (2026-09-10)

The 354.880-second Japanese full-song request completed on Intel B580 with exit 0 in
796.2355 seconds under Vulkan/worker debug logging. Twelve executed workers each emitted
ready/done and exited successfully; the 616-note chart, 35,489-frame pitch evidence,
628 alignment items and five fully decodable exact-duration FLACs were verified. The
2,269,804,206-byte live stderr log fixes the prior in-memory-only diagnostic blind spot.
Evidence is in `test-artifacts/ggml-pipeline-validation/debug-execution-observation/`,
`debug-summary.json`, and `debug-trace-summary.json`; detailed provenance is in
[`STATE.md`](../tasks/remaining-models/STATE.md#full-pipeline-debug-regression-2026-09-10).

This result is **`ok_degraded`**, retaining lead-isolation, instrumental-leakage and vocal-topology
uncertainty. Existing language applicability conditions skipped FireRed/STARS for Japanese;
this is not a seventeen-model sweep. Same-boot completion/post-run observation does not prove
future host stability, debug wall time is not normal throughput, and production readiness is
unchanged. The earlier zero-output full-song run still has unknown completion.

## Operation recording

Each independent change and subsequent execution is committed and recorded before launch with `tools/record-operation.py`, per `docs/ROFORMER_OPERATION_RECORDING.md`. A process exit alone does not establish post-exit host stability, and a missing completion record means the result is unknown.
