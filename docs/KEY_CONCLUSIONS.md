# Uta! Studio — Key Technical Conclusions

**Updated:** 2026-09-09

## Authority

- `AGENTS.md` defines repository-wide execution, architecture, safety, and handoff rules.
- `docs/design/README.md` links the current architecture documents.
- `tasks/remaining-models/STATE.md` is the current model/task index.
- Historical evidence remains scoped to the exact source, model bytes, backend, device, and command that produced it. It does not override current source.

## Runtime architecture

- Studio communicates with packaged `uta-analyze` and `uta-runtime` machine protocols. `app-core/**` and `desktop/**` do not import backend implementation crates.
- Model inference uses one boundary for every model: Rust-owned graphs call shared libraries built from upstream `ggml-org/ggml` revision `8c63e70982c95ceb862e3a1073a2c1beef75d60a`. The backend alignment is complete; no model executes through any other route.
- The runtime package contains upstream `libggml.so.0`, `libggml-base.so.0`, `libggml-cpu.so`, and `libggml-vulkan.so` plus a manifest. It is upstream GGML plus exactly the backend patches the runtime recipe declares, and contains no app-owned C/C++ model graph, shim, model CLI, or inference executable. The repository tracks no model conversion, model rewrite, or model execution script in any language; container migration is the Rust `cargo xtask gguf` command.
- CPU is an explicitly selected experimental reference lane. Vulkan remains the default; a GPU or integrated-GPU request fails closed if its requested device cannot initialize or execute and never falls back to CPU.
- `native-inference/gpu-probes` performs read-only Vulkan enumeration for diagnostics and descriptor matching. It does not create a Vulkan device or execute inference.
- `uta-ggml-worker` executes Rust model implementations in-process. Its FFmpeg calls are audio codec operations, not model inference subprocesses.
- Runtime and model directories are user data. Tests use isolated fixtures and do not delete or replace configured assets.

## Current executable models

The Runtime Manager catalog contains exactly seventeen models and one shared-library runtime. Every model pins the `ggml` backend and depends on the `ggml_vulkan` runtime.

| Model | Capability |
| --- | --- |
| `bs_roformer_leap_xe90_vocals` | `audio.extract_vocals`, `audio.extract_instrumental` |
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

## Operation recording

Each independent change and subsequent execution is committed and recorded before launch with `tools/record-operation.py`, per `docs/ROFORMER_OPERATION_RECORDING.md`. A process exit alone does not establish post-exit host stability, and a missing completion record means the result is unknown.
