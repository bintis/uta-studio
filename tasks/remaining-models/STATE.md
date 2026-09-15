# Remaining Models + Final Feature Closure — State

**Updated:** 2026-09-14 (fusion activity, independent lyric timing and measured regression follow-up)
**Owner:** Rust + upstream-GGML migration

This file stores current effective state only. Historical execution evidence remains in its original records; current source and focused tests override stale historical conclusions. Durable cross-cutting conclusions live in `docs/KEY_CONCLUSIONS.md`.

## Latest-source runtime trimming and reuse — CPU verified, GPU paused (2026-09-11 UTC)

User direction: latest upstream library source and one current implementation; source acquisition,
compilation and CPU tests allowed, **no GPU access/tests**. Current builders no longer select the
historical GGML commit or LibTorch wheel release. Installed runtimes remain unchanged. Two GGML
patch contexts were rebased and SPIRV header propagation fixed; latest GGML CPU and Vulkan libraries
compile (Vulkan never loaded). Native LibTorch app code compiles against the existing runtime, and
its CPU rotary-phase regression matches exactly. Full latest-source XPU LibTorch compilation is
**incomplete**, blocked by missing SYCL compiler/SDK and build-time PyYAML; no fallback qualifies it.

An isolated installed-dependency copy shrinks **2,913,764,174 → 1,991,436,583 bytes** via exact ELF
alias deduplication/static-debug removal. This is not final source-built runtime size. GGML GAME
reuses window-owned graph/immutable conditioning; weighted CPU checks found and fixed gallocr input
reuse, then match fresh graphs exactly. LibTorch reuses rotary phases per layer stack and retains
the shared diffusion adapter. Final focused checks: GGML GAME 28 pass/2 ignored, LibTorch GAME 24,
worker runtime 6, runtime lock 1; weighted medium GAME CPU separately passes, as do CPU rotary and
packaging fixtures. No measured GPU speedup, installation or readiness promotion. The old optional
fixed-wheel Nix runtime derivation is removed; source-based XPU Nix packaging remains release work.
Evidence, failed attempts, timeout provenance and remaining work:
[latest-source trimming](../../docs/design/runtime/LIBTORCH_EXECUTION.md#latest-source-trimming-and-reuse--cpu-verified-xpu-source-build-incomplete-2026-09-11-utc).
Historical pinned/wheel statements below describe prior execution evidence, not current acquisition.

## Current product model set

Studio has one model execution boundary: Rust-owned graphs calling the shared libraries built from upstream `ggml-org/ggml` revision `8c63e70982c95ceb862e3a1073a2c1beef75d60a`. The package is upstream GGML plus exactly the backend patches `native-inference/ggml-worker/runtime-recipe.json` declares, and contains no app-owned C/C++ model graph, shim, model CLI, or model-inference subprocess. The repository contains no model conversion or model-rewrite script; container migration is `cargo xtask gguf`. Vulkan remains the default. CPU is an explicitly selected experimental reference mode; GPU and integrated-GPU requests never fall back to it.

The backend alignment is complete: every authorized model runs through that boundary by default, and the Runtime Manager catalog contains exactly these eighteen models plus two native runtimes: the pinned `ggml_vulkan` shared-library runtime and, since 2026-09-11, the explicitly selectable `libtorch_xpu` native LibTorch runtime executed by the same packaged worker (see the product-route section below).

| Resource | Capability | State | integration_ready | production_ready | Current conclusion |
| --- | --- | --- | --- | --- | --- |
| `bs_roformer_leap_xe90_vocals` | `audio.extract_vocals`, `audio.extract_instrumental` | NEEDS_REVIEW | yes | no | Default separation provider. One invocation emits guide vocals and the instrumental residual. A real AMD 780M run completed; comparison with the former GGML reference measured 60.36 dB SNR and mix reconstruction measured 122.10 dB SNR. Longer-input, performance, and accepted strict-parity coverage remain. |
| `bs_polarformer_public_instrumental` | `audio.extract_vocals`, `audio.extract_instrumental` | NEEDS_REVIEW | yes | no | Explicit experimental separation option. Rust GGML execution completed on AMD 780M with finite outputs and vocal+instrumental reconstruction error at float epsilon. A former-implementation reference comparison and broader listening evidence remain. |
| `melband_roformer_harmony` | `audio.lead_isolate` | NEEDS_REVIEW | yes | no | Rust GGML lead/residual isolation completed on AMD 780M with finite exact-duration output and float-epsilon residual reconstruction. Former-implementation reference vectors and longer-input coverage remain. |
| `melband_roformer_denoise_aufr33` | `audio.denoise` | NEEDS_REVIEW | yes | no | Rust GGML execution completed on AMD 780M with finite 44.1 kHz stereo output. Former-implementation numerical/perceptual comparison and longer-input coverage remain. |
| `melband_roformer_dereverb_anvuew` | `audio.dereverb` | NEEDS_REVIEW | yes | no | Rust GGML execution completed on AMD 780M with finite 44.1 kHz stereo output. Former-implementation numerical/perceptual comparison and longer-input coverage remain. |
| `rmvpe` | `pitch.track` | NEEDS_REVIEW | yes | no | Rust owns the complete RMVPE graph and calls upstream GGML directly. A real AMD 780M run completed. Against the historical same-source fixture, 601 frames aligned, one voiced decision differed, common-voiced F0 RMSE was 0.35158 Hz (maximum 5.27094 Hz), and confidence RMSE was 0.008684. Intermediate-tensor investigation remains before strict parity can be accepted. |
| `fcpe` | `pitch.secondary`, `pitch.secondary.fcpe` | NEEDS_REVIEW | yes | no | Optional secondary continuous-F0 expert, scheduled by default only in Maximum mode. CPU and AMD 780M Vulkan runs each produced 601 frames with zero voiced-state disagreements against the fresh OpenVINO reference. CPU/reference F0 RMSE was 0.10770 Hz (maximum 2.56958 Hz); Vulkan/reference RMSE was 0.17518 Hz (maximum 2.81871 Hz). Layerwise vectors, long-input performance, and broader material remain. |
| `basic_pitch` | `notes.basic_pitch` | NEEDS_REVIEW | yes | no | Optional note challenger. CPU and AMD 780M runs each produced 516 frames with zero contour-class disagreements against the historical same-source CPU reference. Long-input and broader-material coverage remain. |
| `game_1_0_3_small` | `notes.game` | NEEDS_REVIEW | yes | no | Selectable GAME size. A real 6-second AMD 780M worker request produced schema 4 evidence with 17 notes. Reference vectors and long-input coverage remain for this size. |
| `game_1_0_3_medium` | `notes.game` | NEEDS_REVIEW | yes | no | Default note provider. Component graphs have bounded historical parity, and a 32-second AMD 780M run exercised dynamic 30-second chunking with 2-second overlap and 97 ordered notes. |
| `game_1_0_3_large` | `notes.game` | NEEDS_REVIEW | yes | no | Selectable GAME size. A real 6-second AMD 780M worker request produced schema 4 evidence with 17 notes. Reference vectors and long-input coverage remain for this size. |
| `jbm555_cectc_80` | `notes.jbm555` | NEEDS_REVIEW | yes | no | Mandatory mix + prepared-vocal dual input. CPU and AMD 780M runs matched the historical reference's single note, range, and MIDI; all score differences were below `0.0005`. Broader-material coverage remains. |
| `stars` | `notes.stars`, `technique.analyze` | NEEDS_REVIEW | yes | no | Timed-transcript-conditioned note/technique evidence; depends on `rmvpe`. Stage C and Stage E were validated against checkpoint-truth oracles on CPU and AMD 780M. A real 12-second Radeon 780M GGML worker run now completes with 28 notes, 51 technique segments and eight style segments from the installed migrated GGUF. Reference comparison remains. |
| `rosvot` | `notes.rosvot` | NEEDS_REVIEW | yes | no | Timed-transcript-conditioned note evidence; depends on `rmvpe`. CPU, AMD 780M, and B580 worker runs completed. The same-source 12-second Radeon 780M comparison produced 2,250 valid frames and 23 notes from the installed migrated GGUF. Reference comparison and long-input coverage remain. |
| `firered_asr2_aed` | `speech.transcribe.challenger` | NEEDS_REVIEW | yes | no | Optional transcript challenger with a three-file named artifact set (`firered-f32.gguf`, `cmvn.ark`, `dict.txt`). Two defects were found and fixed: the encoder dropped its subsample output projection bias, and the subsampling convolutions used upstream's F16 im2col. With those plus the Vulkan F32 matmul patch, the encoder parity test passes on the CPU lane (`0.000555` from the checkpoint oracle), the AMD 780M (`0.000194`) and the Intel B580 (`0.000197`), and the whole chain transcribes the reference fixture as `你好世界` with tokens `[1202, 2246, 1019, 4710]` on all three, matching the historical implementation exactly. Long-input and broader-material coverage remain. |
| `qwen3_asr_1_7b` | `speech.transcribe` | NEEDS_REVIEW | yes | no | Primary transcription. The pinned `Qwen3-ASR-1.7B-F16.gguf` is installed in the model store through `uta-runtime import`, generation `8719ea947b78b28301a6705c2c480bd7f62da369ea9a4a7520c8a85b3560b179`, and `uta-runtime resolve` reports `production_pinned` with no readiness reasons. Real AMD 780M and Intel B580 runs completed. Broader language/material coverage and performance measurement remain. |
| `qwen3_forced_aligner_0_6b` | `speech.align` | NEEDS_REVIEW | yes | no | Word-level forced alignment with a Rust-owned timestamp decoder. Decoder parity was validated on CPU and AMD 780M against the official reference, and a B580 worker run produced four timed word items. Broader material coverage remains. |

`rhythm.quantize`, Acoustic DSP, the Candidate graph, fusion, FFmpeg decoding, and the GPU probe are local Rust work, not models.

## Reimplementation queue

Empty. Qwen ASR, Qwen Forced Aligner, FireRed, GAME, Basic Pitch, ROSVOT, STARS, and JBM555 all completed their staged Rust/upstream-GGML reimplementation and are catalog resources with typed worker routes, Analysis Engine wiring, workflow schema 7 nodes, and Settings/UI representation. Inst V2 remains permanently retired: no catalog entry, graph, worker route, or fallback.

The former C++/CLI/WGPU/OpenVINO execution code remains deleted; Git history and recorded artifacts are reference evidence only. Historical Qwen and retired-model measurements are evidence about deleted implementations only and do not establish current readiness.

## Vulkan F32 matmul: diagnosed and patched

Upstream builds `pipeline_matmul_f32` from whichever shader family the device selected, and in the
coopmat2, coopmat and fp16 families the matmul shader's `FLOAT_TYPE` is `float16_t`
(`vulkan-shaders-gen.cpp` promotes it whenever `coopmat2 || fp16`). An all-F32 matrix multiply
therefore ran on F16-rounded operands. `firered::encoder::tests::selected_backend_multiplies_f32_matrices_accurately`
measures it against an exact f64 reference:

| Device | before | after the patch |
| --- | ---: | ---: |
| CPU backend | 2.391e-7 | 2.391e-7 |
| AMD Radeon 780M | 2.388e-3 | **4.234e-7** |
| Intel Arc B580 | 2.388e-3 | **4.234e-7** |

Before the patch the error grew with reduction depth (6.328e-3 at k=64, 1.596e-2 at k=1280) and no
runtime option changed it: `GGML_VK_DISABLE_F16`, `DISABLE_COOPMAT`, `DISABLE_COOPMAT2`,
`DISABLE_INTEGER_DOT_PRODUCT`, `DISABLE_DOT2`, `DISABLE_MMVQ`, `DISABLE_FUSION`,
`DISABLE_GRAPH_OPTIMIZE` and an explicit `ggml_mul_mat_set_prec(GGML_PREC_F32)` were each tested
alone and left it unchanged. Only disabling F16 *and* coopmat together reached the exact scalar
shaders, at the cost of every model's fast path.

Upstream is not wrong to ship this: `test-backend-ops` gives `MUL_MAT` an NMSE tolerance of `5e-4`
against `1e-7` for ordinary operations, the pinned revision passes 1002/1002 on Vulkan, and current
master keeps that tolerance. It suits quantised LLM weights. Uta! Studio's audio graphs are F32 end
to end and decode greedily, so the same trade changed output.

`native-inference/ggml-worker/patches/0001-vulkan-keep-f32-matmul-in-f32.patch` rebuilds only
`pipeline_matmul_f32`, from the scalar `_fp32` shaders whose `FLOAT_TYPE` is `float`. Every F16,
BF16 and quantised pipeline keeps the device's fast family. It also clears
`mul_mat_l[GGML_TYPE_F32]`, because the large scalar tile is the one upstream turns off for AMD and
Intel whenever coopmat is unavailable: keeping it cost Leap 71.3 s against 24.1 s.

`runtime-recipe.json` declares the patch and its digest, so the recipe digest changed and a runtime
built without the patch no longer validates. `build-ggml-runtime.sh` applies the declared patches to
the verified checkout and restores it afterwards, and GGML then reports the pinned commit with a
`-dirty` suffix, which `is_pinned_commit` accepts explicitly.

Measured B580 cost, same twelve-second inputs before and after:

| Model | before | after |
| --- | ---: | ---: |
| Basic Pitch | 2.61 s | 1.93 s |
| RMVPE | 2.54 s | 2.28 s |
| FCPE | 1.54 s | 1.68 s |
| GAME medium | 2.20 s | 2.35 s |
| Denoise | 7.73 s | 7.49 s |
| Qwen ASR | 5.42 s | 6.40 s |
| Leap separation | 21.34 s | 24.14 s |
| FireRed | wrong output | 6.08 s |

Only Leap pays a real cost, about 13%. Output change is small where it is not decisive: RMVPE's
1,201 frames moved by at most `0.00167` Hz with no voiced-state flips, and Basic Pitch's activations
are bit-identical, while FireRed went from an empty transcript to the reference text.

## Installed artifact migration

STARS, ROSVOT and FireRed were converted before this runtime existed, by a converter that recorded
native PyTorch dimension order and, for the two conformer models, tensor names at or above GGML's
64-character limit. Upstream GGML refuses to open those containers:

- `rosvot`: `gguf_init_from_reader: tensor name 88 is too long: 64 >= 64`
- `stars`: `gguf_init_from_reader: tensor name 77 is too long: 69 >= 64`
- `firered_asr2_aed`: `tensor shape mismatch: encoder.input_preprocessor.conv.0.weight; expected [3, 3, 1, 32], found [32, 1, 3, 3]`

`cargo xtask gguf <stars|rosvot|firered> SOURCE OUTPUT` performs the container migration in Rust:
payload bytes and offsets are copied verbatim, dimensions are reversed into GGML order, and
structural path components are abbreviated where a name would otherwise be too long. Its output is
byte-identical to the historical Python scripts for all three models, checked against the surviving
migrated containers.

Migrated container identities:

| Model | sha256 | bytes |
| --- | --- | ---: |
| `stars` | `2d845732ce308b8bf89304c0c854557246a76032f593df9f0dc42c447aa9a893` | 201,085,024 |
| `rosvot` | `ae208457b04cc11ef2dcd063dabf2d0864785a17981955366aac888390c148c8` | 48,194,496 |
| `firered_asr2_aed` | `7724d4f01ac8c208670be968cef236b73f4276eddd0de8f85441b56bb6e9d132` | 4,686,918,112 |

The catalog previously pinned FireRed's **pre**-migration digest, so it pinned a file the runtime
cannot load; it now pins the migrated container.

The installed containers were migrated on 2026-09-09 under explicit user authorisation, and the
originals were kept rather than deleted:

- `stars` and `rosvot` gained a new generation directory named by the migrated digest, and only
  `current` plus the two top-level symlinks moved. Their previous generation is untouched, so
  rolling back is a one-line change to `current`.
- `firered_asr2_aed` has a flat directory, so its pre-migration container was moved to
  `~/.local/share/uta-studio/runtime/pre-migration-backup-20260909/firered-f32.gguf`.

All three then resolved and executed on the B580 through their store paths, which is what
`StorePaths::ggml_model_path` reads: STARS produced 24 notes plus technique evidence, ROSVOT 15
notes, and FireRed the reference text `你好世界`. Evidence:
`test-artifacts/b580-store-paths-20260909T1150Z`.

`audio.lead_partition` is the one capability still reported as unimplemented. It is the lead/backing/harmony partition product feature tracked by `tasks/final-features/17_LEAD_BACKING_HARMONY_PARTITION.md`, not a model backend gap.

The per-model migration outcome and its device evidence are summarized in `tasks/remaining-models/NEXT_WINDOW_HANDOFF.md`.

## Current runtime boundary

- `native-inference/ggml-runtime`: Rust GGUF loading, graph construction, CPU/Vulkan execution, WAV/STFT/iSTFT, and the RoFormer, RMVPE, FCPE, Basic Pitch, GAME, JBM555, STARS, ROSVOT, FireRed, and Qwen implementations.
- `native-inference/ggml-worker`: machine-protocol worker calling the Rust graph implementations in-process.
- `native-inference/gpu-probes`: read-only Vulkan enumeration used for diagnostics and device matching; it does not create a device or run inference.
- `native-inference/runtime-lock.json`: schema 3, one upstream shared-library runtime.
- FFmpeg remains an audio codec subprocess boundary; it is not used for model inference.
- `tools/` holds only the operation-recording and host-observation utilities required by `docs/ROFORMER_OPERATION_RECORDING.md`. No model conversion, model rewrite, or model execution script is tracked.

## Verification status

Passing focused suites on the current tree:

- `bash dev.sh --command cargo test --locked -p uta-analysis-engine -p uta-studio-core`
- `bash dev.sh --command cargo test --locked -p uta-runtime-manager`
- `bash dev.sh --command cargo test --locked -p uta-ggml-runtime -p uta-ggml-worker`
- `bash dev.sh --command cargo test --locked -p uta-studio-desktop`

`cargo check --locked -p uta-ggml-runtime -p uta-ggml-worker --all-targets` is warning-free.

Real AMD 780M smoke evidence exists for RMVPE, FCPE, Leap, PolarFormer, Denoise, Dereverb, Harmony, Basic Pitch, GAME small/medium/large, JBM555, ROSVOT, STARS, Qwen ASR, the Qwen aligner decoder, and the FireRed stages and model load listed above.

Real Intel Arc B580 evidence now exists for fifteen of the seventeen models: Basic Pitch, JBM555, FCPE, RMVPE, ROSVOT, STARS, GAME small/medium/large, PolarFormer, Leap, Denoise, Dereverb, Harmony, and the Qwen aligner all executed on the discrete B580 and published their typed artifact with `backend: ggml_vulkan`. Every published audio artifact is FLAC, 44.1 kHz, stereo, exactly 12.000 s, and non-silent. The boot ID stayed `582d1c5a-d3c9-44b6-b2d1-a1dd0f1a0fb6` across the whole sweep, so no host reset occurred during these bounded runs; that does not retire the previously recorded whole-machine power-off failures. Evidence: `test-artifacts/b580-all-models-20260909T0910Z/`.

An invalid explicit Vulkan index failed with `selected Vulkan physical device is unavailable`, produced no model artifact, and did not fall back to CPU. Smoke success, finite output, and bounded parity do not establish broad production qualification.

## Full-song Intel B580 validation (2026-09-09)

One real 354.88-second song, 44.1 kHz stereo, every model in dependency order on the patched
runtime, one worker process each. **All seventeen executions passed**, the boot ID never changed,
and total GPU time was 21.9 minutes.

| Execution | Wall | Output |
| --- | ---: | --- |
| RMVPE | 8.9 s | 35,489 frames to 354.88 s |
| FCPE | 3.3 s | 35,489 frames to 354.88 s |
| Basic Pitch | 9.5 s | 30,566 frames to 354.86 s |
| GAME small | 14.7 s | 705 notes |
| Leap separation | 428.6 s | vocals + instrumental, 354.880 s each |
| GAME medium | 25.7 s | 721 notes |
| GAME large | 46.7 s | 737 notes |
| Denoise | 164.1 s | 354.880 s |
| Dereverb | 82.6 s | 354.880 s |
| Harmony | 164.8 s | lead + residual |
| PolarFormer | 269.5 s | vocals + instrumental |
| JBM555 | 8.5 s | 40 notes, chunked |
| STARS | 18.9 s | 659 notes plus technique evidence |
| ROSVOT | 5.6 s | 666 notes |
| Qwen ASR | 25.1 s | multilingual transcript, 1 unfinished window |
| Qwen aligner | 4.4 s | 8 timed word items |
| FireRed | 35.6 s | transcript over 268 windows, 1 unfinished |

The chain used real upstream outputs: Denoise, Dereverb, Harmony and JBM555 took Leap's published
guide vocals, and STARS and ROSVOT took the RMVPE evidence from the same run. Evidence and ledger:
`test-artifacts/b580-fullsong-final-20260909T1230Z`.

An earlier run of the same sweep failed three executions on fixed budgets that only a full-length
input reaches, and those are now fixed: JBM555 built one graph over the whole song and asked for a
13 GB buffer, while Qwen ASR and FireRed failed the whole track because a decoder with nothing to
transcribe runs to its token budget instead of predicting EOS. Instrumental windows are certain in
a song, so both now drop that window and count it in the published evidence.

### The three long-input limits this exposed, and their repairs

None of them was a device or artifact problem. Every installed artifact loaded, including the three
migrated containers and the newly imported Qwen F16, and every failure was a fixed budget that only
a full-length input reaches.

- **JBM555** built one graph over the whole input with a fixed 16 MiB graph arena
  (`GRAPH_MEMORY_BYTES` in `native-inference/ggml-runtime/src/jbm555/graph.rs`). A 354.88-second
  input needs more graph metadata than that, so `ggml_new_graph_custom` returned null and the
  allocator then asked for 13 GB. It now runs `run_features_chunked` over 1,024-frame chunks with
  64 frames of context, the same shape GAME and the RoFormers already used.
- **Qwen ASR** allowed `DEFAULT_MAX_NEW_TOKENS = 256` per window, and window 9 of the song
  exhausted it. The window was instrumental, so the decoder had nothing to transcribe and ran to
  its budget instead of predicting EOS. Unfinished windows are now dropped and counted in
  `unfinished_windows`; the run fails only when every window is unfinished.
- **FireRed** allowed `MAX_GENERATED_TOKENS = 11` per ~2.3-second window. The reference speech
  fixture needs five, so the value passed the unit fixture and was far too small for sung audio. It
  is now `ENCODER_FRAMES`, and unfinished windows are handled the same way as Qwen's.

## STARS and ROSVOT are not resolvable in production

Every STARS and ROSVOT run recorded above, including the full-song validation, was given a model
path directly out of `test-artifacts/`. Neither model has an installed generation in the managed
store: `~/Documents/uta-studio/models/` holds only `source-models/stars-chinese-.../
model_ckpt_steps_200000.ckpt`, and the only `stars-f32.gguf` and `rosvot-f32.gguf` on this machine
are under `test-artifacts/`. `uta-runtime resolve model:stars` therefore answers `resource_missing`,
so a real production analysis cannot reach either model even though both execute correctly.

The catalog already declares the right filenames (`runtime-manager/src/catalog.rs`), so this is a
missing installation and its provenance, not a code defect. The operator authorised fixing it in
the post-measurement modification phase on 2026-09-09.

## Full-pipeline debug regression (2026-09-10)

**COMPLETED with `ok_degraded`; not production qualification.** This is the full-song Analysis Engine request in
`test-artifacts/ggml-pipeline-validation/request.json`, not the separate seventeen-model sweep.
The current execution is recorded by operation `20260910T065356-cfaece833c9e`, with live logs and
samples in `test-artifacts/ggml-pipeline-validation/debug-execution-observation/` and outputs in
`test-artifacts/ggml-pipeline-validation/fullsong-debug/`. It started on boot
`e56ad005-bfa7-47ea-9891-9b96928ffd25` and completed with exit code 0 after **796.2355 seconds**.
Both observer and recorder completion files exist. The boot was unchanged through the recorded
07:10:25 UTC post-run observation, not a guarantee of future host stability. The observer reports
3,688 process samples, 741 host samples, zero read/observer errors, and only xe device `0000:07:00.0`.
A missing completion record in any other run still means unknown, never a pass or proof of no launch.

- Logging implementation `7e08a69` forwards worker stderr without truncating the live stream,
  logs protocol frames, and enables CLI lifecycle diagnostics with `UTA_STUDIO_DEBUG=1`.
  Build source commit is `f9c4b6f`; the run HEAD `900d2f0` differs only in unrelated documentation.
  Operation `20260910T064749-c4d30143672f` passed targeted formatting, three debug-log tests,
  eight worker-supervision tests, two lifecycle tests, and the release CLI/worker build.
  Operation `20260910T064942-ab439830cba4` passed four recorder and four observer tests.
- Independent runtime build `20260910T064825-5db5f0f3f9f3` succeeded with the pinned upstream
  commit and ten declared patches, `RelWithDebInfo`, `GGML_VULKAN_DEBUG=ON`, and
  `GGML_VULKAN_CHECK_RESULTS=OFF`. No installed runtime/model was replaced. Upstream emitted
  a `ggml_can_fuse` maybe-uninitialized compiler warning; this was not a warning-free build.
  Runtime memory logging, Vulkan loader logging, and stage profiling are enabled for the run.
- Pre-run observations `20260910T065229-673a333b924b` and `20260910T065230-98eacba9bd5c`
  measured about 2.2% total CPU and 2% B580 use; these do not establish isolation. Full debug
  logging changes overhead and this run is not a normal-throughput measurement.
- Prior `complete-observation/` has no completion record; its last process sample is
  `2026-09-10T06:28:08.954889+00:00`. Its boot differs from the current boot. That does not
  identify reset cause. Previous-boot kernel logs were unavailable due to journal permissions.
- `debug-observation/` is a separate setup-only failure (`missing_required_input`: output
  directory did not exist), before worker launch. Operation `20260910T065356-23a470f78cfa`
  then created the isolated output directory before the explicit execution above. No GPU
  failure has been automatically retried; all original evidence is retained.

Verification and limits:

- `debug-summary.json` records parsed, finite output JSON and verified artifact references,
  file sizes and FLAC signatures, without hash verification (operation
  `20260910T070854-b1e1bd5ca53a`). The chart has **616 notes** (362 pitched, 254 spoken),
  pitch evidence has **35,489 frames**, alignment **628 items**, GAME **704 notes**,
  ROSVOT **574 notes**, and singing analysis **21,875 candidates**. Chart inspection operations:
  `20260910T071111-84c4ac06860b`, `20260910T071149-7f4378ba5928`.
- All five published/intermediate FLACs decoded fully with FFmpeg: guide vocals, lead vocal,
  instrumental, denoise and dereverb; all are FLAC, 44.1 kHz, stereo, exactly **354.880 s**
  (`20260910T070910-0c5f5ca227ae`). This is decode/timeline evidence, not listening qualification.
- `debug-trace-summary.json` records **21,278,804 stderr lines / 2,269,804,206 bytes**,
  twelve matching worker ready/done/successful-exit records, 970 progress frames, 14 output
  frames, no worker/node failures and no malformed debug records
  (`20260910T071020-492d4127ed0b`). Source/user media and installed models were not changed.
- This Japanese request executed **twelve models**. Existing `firered_language_applicable`
  and `stars_g2p_language_applicable` conditions skip FireRed and STARS for Japanese;
  they have no execution/artifact evidence from this run despite their workflow declarations.
  Do not describe this as all seventeen models passing or as coverage of those two routes.
- Final quality reasons are `lead_isolation_uncertain`,
  `instrumental_vocal_leakage_uncertain`, and `vocal_topology_ambiguous`. They are retained,
  not suppressed or promoted to clean quality acceptance.

The authorized full-song debug execution and artifact verification are complete. Broader
per-model/linguistic/perceptual qualification and the explicit release pass remain unchanged.

## Super acceleration — IMPLEMENTED, GPU performance validation pending (2026-09-11)

**User correction retained:** multi-GPU assigns different complete model tasks using dependencies,
loading order and predicted total device completion time; it **never splits one model across GPUs**.
The opt-in `turbo_acceleration` request snapshot now activates a three-phase whole-task scheduler.
Measured complete-task seed estimates and per-lane queued finish times place heavy preparation and
speech work on LibTorch XPU/B580 while ready pitch/note work runs through GGML Vulkan on the AMD
integrated GPU. Alignment waits for transcript; STARS/ROSVOT wait for alignment plus shared RMVPE;
fusion waits for joined evidence. One backend/device lane remains serialized through worker exit
and quiescence while the other physical GPU can progress. Parent-linked cancellation, originating
failure preservation, child reaping, exact-format audio reuse, preloading and deterministic result
assembly remain connected.

Settings > Models & runtime owns the mode. Enabling it saves before changing visible state, disables
global/per-model backend and device controls, and disables Processing Studio model/strategy choices.
Saved manual choices are not erased: exact Super requests omit them, and turning the mode off makes
them active again. CPU is never an automatic lane. Diagnostics distinguish requested/predicted
placement from measured dual-device work.

CPU/protocol verification passed: combined operation `20260911T155605-0e383e3b32f1` ran 282
Analysis Engine unit tests, four packaged-boundary tests, 438 app-core tests (one ignored) and 242
desktop tests. No GPU inference or release build was run for this implementation. Targeted clippy
operation `20260911T155349-b726a85e2197` reached the affected crates but stopped on two pre-existing
warnings in `app-core/src/backend_cli/process.rs`; no scheduler warning preceded that blocker.
Corrected dual-GPU wall-time, output comparison, device telemetry and host-stability qualification
remain pending. The historical
chunk-splitting implementation remains removed by `844c016`; its timings do not qualify this
scheduler. Design and boundaries:
[Super acceleration](../../docs/design/runtime/SUPER_ACCELERATION.md).

Historical verification before the task-granularity correction:
- Settings/request propagation and save-failure tests passed (`20260910T072710-605f5879a1fc`).
- Seven RoFormer scheduling tests and two live-budget arithmetic tests passed. The current
  worker's 34 tests, 23 Qwen tests plus one explicit native CPU copy/lifetime test passed
  (`20260910T083251-8fd78b561921`); 13 supervisor tests cover worker reuse, cancelled preparation,
  current-model failure, unused-preload release, source preservation and temporary-cache cleanup
  (`20260910T083645-c46b3c005334`).
- Release CLI/worker built on `a8e082f` (`20260910T083955-2bc52482bec6`). A matched 60-second
  RoFormer pair completed in 32.148910 s ordinary / 28.888136 s super (10.1427% less wall time).
  B580/AMD chunk counts were eight/one, with sixteen sampled simultaneous target-compute
  intervals. Both FLACs fully decoded with equal finite sample counts; vocal relative RMS
  difference was `6.54594e-5`. Evidence: `test-artifacts/super-acceleration/chunk-comparison.json`.
  This is not an overall pipeline, bitwise or perceptual qualification.
- On resumption, boot ID had changed and five Git objects were empty. An isolated reconstruction
  recovered exactly the already recorded `4b007a1` commit/object IDs; repair preserved the branch,
  working files and damaged reflog bytes. Backup: `test-artifacts/git-recovery/20260910T172940/`;
  repair record: `20260910T083136-8bfdafba4c29`. No crash cause or missing execution outcome is
  inferred from this recovery.

- The first instrumented 12-second pipeline pair on `a8e082f` completed in 143.260202 /
  143.958165 s (ordinary/super), with twelve successful models in both modes. Actual weight
  consumption, PCM hits and Qwen retention/injection were observed; total speedup was not.
  Cold-secondary tail and unnecessary-preload refinements are committed as `8069ee4` / `5c0e0a5`;
  fixture correction `a9741bc` keeps existing audio math unchanged. Eight RoFormer, two prediction
  and thirteen supervisor tests passed; release build: `20260910T092805-8cb46841f749`.
- **Restart interruption:** refined ordinary completed in 150.865198 s at 18:32:58 +09:00.
  Refined super **did launch at 18:35:09 +09:00** on `a9741bc`, correcting the initial conversational
  assertion that it had not launched. Last saved progress is the first Leap task at 0/2 chunks;
  no completion exists. New boot began 18:36:16 +09:00. The prior preflight saw saturated CPU
  and a `rustc` process; its command/session is not established. Previous-boot kernel logs are
  inaccessible. Cause and exact failure time remain unknown. Preserve
  `test-artifacts/super-acceleration/refined/pipeline-super-observation/` and operation
  `20260910T093505-99361ad5df8d`; read-only review: `20260910T093840-953484ccee48`.

Global Super audit (source + existing traces only): the recorded ordinary 150.865198 s run contains
105.521 s from native node-start to worker-spawn across twelve models, including inherited
quiescence/queue/host delays—not demonstrated removable time. Qwen's first positive work unit
preceded completion by only 35/29 ms in the historical complete Super trace. Confirmed next
opportunities include Engine/worker PCM and quality-profile reuse, concurrent single-producer
publication, real early preload progress, independent CPU/model tasks, immutable FFT preparation,
RMVPE device-resident handoffs and fixed-shape graph/allocator reuse. Detailed priorities and
numerical/safety caveats are in the design's **Global optimization audit**; evidence:
`test-artifacts/super-acceleration/global-audit.json` (`20260910T095859-63caa23dcca2`). No new model
execution or speedup measurement was performed.

The user subsequently authorized implementation. Landed: shared Engine/worker PCM and validated
facts/profile ownership (`6a5ce26`); task-owned FFT/frontend preparation (`f6275f2`); truthful Qwen
window progress (`5c39192`); owned Acoustic DSP overlap and cancellation (`2fb6d01`); FCPE graph
reuse (`4d8db66`, fixture fix `e1d5eac`); same-device RMVPE handoffs/GRU graph reuse (`c04f079`);
Qwen incremental arena reuse without pinning prefill (`6df2a5e`); shared acceleration ownership
across joined tasks (`9ce129e`). Focused CPU/protocol checks passed,
including explicit native CPU primitive fixtures in `20260910T113607-45fdcc9232e3` and
`20260910T115838-e7354a6bc379`. Preserve the preceding lock-resolution and fixture compile failures;
these did not execute native tests. See the design's **Implemented after the global audit** section.

Weighted reference checks (`dd0074b`, `20260910T123226-4407a8977033`) additionally compared **470,520
finite CPU outputs bit-for-bit**: RMVPE 253,440 values across differing inputs/short GRU tails;
FCPE 217,080 values over differing graph-reuse windows. Model files were read-only. These bounded
weighted CPU windows are not GPU or full-pipeline qualification. Sixteen supervisor/context, four
owner and two prediction tests passed at `20260910T121202-985dcc11e7d1`; targeted CLI/format checks
passed at `20260910T123921-8f13ed9beb0b`.

Next: run an explicitly authorized bounded ordinary/Super dual-GPU comparison with preflight and
both-device telemetry, then revise seed estimates from complete-task phase observations. Queue-aware
multi-pending hot-weight retention/preparation remains. Qwen reduced readback must preserve
first-maximum and all-logit finite checks: the pinned Vulkan argmax lane tie rule cannot be
substituted directly. Full-model numerical checks, cross-song reuse and Studio publication timing
remain incomplete. Safety review follows the linked restart/submission/upload records in the design
document: preserve synchronization/cleanup; do not add arbitrary waits, limits or retries. GPU
experiments stay paused until explicitly authorized; no automatic repeat of the incomplete run.
Whole-pipeline performance/output qualification remains incomplete. No production promotion.

## Native LibTorch XPU — bounded tests resumed (2026-09-10 UTC)

The user explicitly resumed XPU tests; this does not resume the interrupted Vulkan Super run or
hardware-counter pressure groups. Existing AMD evidence was inspected first. Current native
factories contain eighteen resources; this is not an eighteen-resource qualification claim.

`5a46dfc` adds native Rust complete-tensor diagnostics; its focused host test/release build passed.
An initial XPU loader failure for private `libsycl.so.9` was corrected by `912c9f9` using inherited
private RPATH, without system changes. The corrected XPU ABI/synthetic FCPE test passed
(`20260910T172427-1d4e46c7699f`). Real read-only FCPE weights on explicit XPU device 0 then completed
201/97/201-frame synthetic inputs. All **179,640 activation pairs** were finite in full CPU/XPU
comparison: max absolute error **6.29370333627e-10**, NMSE at most **1.58845205696e-11**. Repeated
XPU input after the other shape was not bit-identical (max difference **1.23691279441e-10**).

Evidence: `test-artifacts/libtorch-models/xpu-resume/fcpe-comparison.json`, CPU operation
`20260910T172609-fe134b3ec34c`, XPU operation `20260910T172724-e05fc4d00416`. Earlier loader and
request-preparation failures remain preserved. Host/GPU observations included other UI/test work.

Qwen ASR strict XPU then completed the real 12-second mono fixture with the installed F16 weights
read-only: 156 x 2,048 finite encoder output values, 0.803520399-second cold encoder call, and all
79 synchronization checkpoints through the 24th feed-forward stage. This is not a controlled
speed measurement. Operation `20260910T180636-d09ab6d785e8`; full oneDNN/checkpoint evidence:
`test-artifacts/libtorch-models/xpu-resume/qwen-asr-strict-trace-observation/`. `2023966` adds
trace-only decoder checkpoints. A matching CPU/XPU synthetic-mel encoder, KV-session and two-step
incremental decoder check compared all **305,920 float values and three positions**. Maximum
absolute errors were `1.28523e-7` for encoder output, `1.09673e-4` for prefill logits and
`5.53131e-5` for second-step logits; both complete-logit argmax decisions and every position
matched. XPU operation `20260910T181634-369af609eb70`; comparison
`test-artifacts/libtorch-models/xpu-resume/qwen-decoder-comparison.json`. Both XPU observers sampled
only Intel Level Zero and xe PCI `0000:07:00.0`; no AMD/ROCm/Vulkan target dependency or DRM device
was observed.

The installed forced-aligner F16 container also completed its distinct 1,024-wide encoder and
5,000-class strict classification head on CPU/XPU. All 11,024 float pairs and one position were
compared. Encoder/logit maximum absolute errors were `8.34465e-7` / `9.17912e-6`, both selected-row
argmax decisions matched, and position was exact. The XPU observer again sampled only Intel xe and
reached final logits. XPU operation `20260910T182102-4601fe611d8d`; comparison
`test-artifacts/libtorch-models/xpu-resume/qwen-aligner-comparison.json`. Synthetic mel/token IDs do
not qualify real word timing.

No controlled speedup, real-audio Qwen output parity, real transcription/alignment, all-model XPU
readiness, fused-SDPA availability or later host stability is established. See
[LibTorch execution](../../docs/design/runtime/LIBTORCH_EXECUTION.md) for details and next checks.
Super whole-task scheduling remains incomplete and is not qualified by these results.

## LibTorch RoFormer speed optimization — 64.53 seconds; precision-first selection (2026-09-10 UTC)

The user authorized optimizing the native XPU RoFormer family toward 60 seconds
for the existing 354.88-second song. Historical native XE90 inference completed
in 112.060496322 seconds / 38 chunks; this is not a matched new control.
`9898de9` fuses ordinary XPU rotary arithmetic through complex views; `fa51ee0`
retains head-interleaved SDPA operands; `b7cc13f` fuses RoFormer FP32 RMS norm.
Precision policy, complete context, default chunk/overlap, synchronization,
cancellation and installed assets are preserved. No Vulkan Super/counter stress
resumption or production promotion.

Earlier CPU primitive/ABI checks and 54 Rust library tests passed. Full-axis XPU primitive
checks pass for rotation and attention layout; the latter compares every output
exactly with native packed SDPA. A complete 12-second real-audio rotation+layout
comparison has maximum sample difference 4.97698783875e-6 and SNR 112.6107 dB.
The final fused-normalization XPU primitive and real-weight XE90 checks also
pass: full waveform maximum difference 8.35955143e-6, SNR 111.2070 dB. These are
bounded numerical and synchronized diagnostic results, not full-song performance.
Although a later pre-run snapshot was quiet, continuous observation captured
other GGML workers during the final checks; those timings are not comparable.
Other B580 activity (again 98% total busy at 19:24:51 UTC) paused further GPU
benchmarks. At that point other RoFormer real-audio geometries and the matched
whole-song pair remained unexecuted; the later resumed results are below.
Details and operation evidence:
[Native XPU comparison](../../docs/ROFORMER_B580_LIBTORCH_XPU.md#native-roformer-optimization--in-progress-2026-09-10-utc).

The user subsequently authorized stopping worker PIDs 86316/93935 and then their
analysis dispatcher 83448. All were sent TERM and confirmed absent. The first
new full-song control completed in 134.104695026 seconds but a newly dispatched
worker 94469 competed with it. After stopping the dispatcher, a separate control
observer recorded exit 0, 38 chunks and 122.580421927 seconds inference in
`test-artifacts/libtorch-roformer-speed/fullsong-control-resumed/`. Its outer
operation `20260910T194129-71a332632928` lacks a completion record; do not invent
one or equate observer completion with whole-operation completion/host stability.
The user then explicitly cancelled and requested waiting for a new instruction.
No owned test process remained at the cancellation inspection. At that point the
normalized full-song candidate and TF32 experiment had not executed, and the user
required waiting for explicit authorization, subsequently given below.
Pause receipt: `test-artifacts/libtorch-roformer-speed/PAUSED.txt`, operation
`20260910T194410-ffe5f8febca8`. Unrelated working-tree changes remain intact.

The user explicitly resumed testing after that cancellation. Operation
`20260910T194530-561f36b751e3` records this new authorization and the fresh pair /
separate TF32 diagnostic plan. The preceding pause is historical, not an active
blocker; do not overwrite its files or reinterpret missing completion records.

The fresh full-song pair completed: **114.231050341 → 72.537182110 seconds**
inference (38 chunks), **36.50% less time / 1.575× speedup**, not yet 60 seconds.
All 31,300,416 waveform samples are finite; maximum difference `0.0006777942`,
SNR **83.26265 dB**, not bit-identical or listening-qualified. Continuous samples
found no other CCS compute clients; both observers report zero read errors and
unchanged boot IDs. Evidence: `current-fullsong-comparison.json` in the speed root.

An isolated QKV/FFN TF32 diagnostic measured 73.695609837 versus 73.182877546
seconds with differing CPU load (and one candidate observer read error): **no
useful speedup established**. Full waveform SNR is 144.15589 dB. oneDNN accepted
the TF32 attribute but actual reduced-precision hardware execution was not
proven. The user explicitly **retired TF32**; `8483b41` removes its build switch,
projection helper and dedicated tests. Historical artifacts remain; no more TF32
experiments. The Rust metadata forwarding check brings the library suite to
55 passing tests; the suite and CPU ABI/primitive checks passed again after
retirement (`20260910T202405-573055382379`).

Family 12-second regression executed XE90 vocals/instrumental and mel-band
denoise/dereverb, comparing every retained waveform sample. Comparisons against
the original control include earlier rotary/layout/normalization changes:
SNR 111.21 / 67.62 / 136.84 / 84.53 dB, respectively; no listening/parity claim.
Original PolarFormer control failed inside native SDPA requesting 8.23 GiB.
Original Harmony control emitted nonfinite masks
(`20260910T215111-9b8cb57be73d`). Neither failed control was retried or used for
candidate acceptance. Those two geometries remain blocked, not family-wide
acceptance. Evidence: `final-active-comparison.json` and the recorded cases.

Current authorization: optimize speed **while preserving the existing precision
policy**, full context/chunk/overlap, serial execution, cancellation and sync.
Four further XPU changes were investigated; only the first two remain active: `e2674d9` merges
ordinary FP32 rotary with existing half writeback; `d8fa55c` promotes SDPA output
inside FP32 gating; `6382318`/`45db8e1` fuses FP32 projection + erf GELU; `dafc2a7`
fuses output projection + FP32 residual. No TF32 allowance or in-place input
mutation. CPU primitives/ABI pass. XPU writeback matches every element exactly
on both 79,349,760-element axes (about 3.6–3.8 ms → 1.54 ms per isolated call;
some control samples are slower). Gating checks match explicit promotion exactly.
Native oneDNN verbose output confirms erf-GELU and binary-add matmul post-ops.
Every two-chunk XE90 waveform sample was compared: all four fusions versus
`conversion-profile` have max difference `7.688999176e-6`, SNR **111.75176 dB**;
not bit-identical or listening-qualified. Evidence: `precision-fusion-comparisons.json`,
`residual-comparison.json`. Small projection oracle bounds are documented in the
comparison document; failed CPU attempts remain recorded, not erased.

The subsequent full-song pair and ablations used tracing/oneDNN verbose off
and no warm runs; these supersede the earlier pending measurement:

- Fresh control: **74.567869923 s** inference / 76.898180361 s process.
- All four fusions: **71.421349470 s**, SNR 79.33864 dB; max waveform difference
  `0.00114057213`. Memory peak drops, but this is not adequate evidence to retain
  every fusion under the user's precision-first direction.
- **Selected conversion-only path:** **64.525976343 s** / 66.647711604 s process;
  every one of 31,300,416 samples compared, SNR **144.17758 dB**, max difference
  **3.576278687e-7**. No compiler/other CCS samples or observer read errors;
  mean CPU busy 11.86%, unchanged boot. Not bit-identical or listening-qualified.
- GELU without residual post-op: **62.112326177 s**, but still **79.33864 dB** and
  CPU compiler contention (mean CPU busy 24.45%). It is **not selected** merely
  to save about two seconds. `a87814e` removes both post-op implementations and
  their dedicated tests. No test bounds from those experiments remain active.

Evidence: `precision-fullsong-comparison.json`, `fusion-ablation-comparison.json`.
The brief 78.68% CPU/compiler observation deferred an ablation; no process was
killed or automatically retried. A later independent source review and quiet
snapshot preceded the conversion-only run.

`00d0cc1` / `2b45ab2` add an arithmetic-free paired value copy, not a multiply:
all half **storage bits** match scalar conversion on both full 79,349,760-element
axes, plus negative zero, halfway values, subnormals, odd widths and shifted
storage. Local synchronized time is about **1.85 → 1.50 ms**. Native scalar
conversion on the same device handles nonrepresentable views; no accepted
shape is restricted and no backend/precision fallback is introduced. However,
its full-song trial measured **65.16225 s** versus 71.93590 s control under
compiler contention in both runs, with one candidate observer read error.
Waveform SNR was 126.81178 dB, max difference 2.65240669e-6. This does **not**
establish an incremental model gain over the cleaner selected 64.52598 s path.
`71c0ad0` therefore leaves paired copy **diagnostic-only**, not model-routed.
The axis-view normalization probe also found no material gain; no model axis
copy was removed. Evidence: `selected-fullsong-comparison.json` and
`norm-axis-full-check/`.

**Prior handoff source:** `71c0ad0`; fresh private build `selected-build`.
The later CPU optimization below supersedes its no-further-execution plan.
The active model operations match retained `gating-build`; no GELU/residual
post-op, TF32 trial or paired value-copy routing remains. Current CPU primitives,
ABI and **55 Rust tests** pass (`20260910T221000-8f26c62c9a55`). A 68.10%
CPU/compiler snapshot deferred current-build GPU smoke during source/docs
review. After that independent work, CPU was 7.23% but desktop graphics active;
four serial **numerical-only** 12-second model smokes completed
(`20260910T222555-40b409a2bc8d`), not throughput measurements. Every retained
sample is finite and compared; XE90 vocals/instrumental versus the pre-conversion
normalized controls give **143.205 / 140.000 dB**, distinct from original-control
comparisons. Observer read errors were 0/1/2/0, with unchanged boot IDs; no reset
or host-stability guarantee. See `final-active-comparison.json`.
The matching active path also has the retained full-song and exact XPU primitive
evidence. No more GPU execution is planned in this handoff. The
**60-second goal remains unmet**; no installed runtime/model/source media changed.
Next technical blockers are the two original model failures and further
precision-preserving bottleneck work, not TF32 or unqualified post-op promotion.

The user's 2026-09-11 CPU/XMX question was investigated **read-only**. Saved
64.53-second-path samples show inference at **98.86% of one logical CPU**
(6.18% of this 16-logical-CPU machine), with 11.86% mean whole-host busy and no
compiler samples. CPU stack attribution remains unknown; do not equate busy
submission/synchronization with transferable DSP work. STFT, packing, mask
reconstruction, iSTFT and OLA remain on CPU; learned graph operations are on GPU.
Historical diagnostics executed 64 FP16 native SDPA calls and matching source
identifies the systolic-capable route, but no all-XMX coverage/occupancy evidence
exists. First expose existing native upload/compute/readback timings and host
frontend phases; GPU migration benefit is **not measured**. Preserve numerical
ordering and all safety/precision scope. See the CPU/XMX review section of the
comparison document and `cpu-accounting-review.json` / `dispatch-frontend-review.json`.

The subsequent optimization authorization implements frontend profiling (`02cf2e5`),
exact-bit tiled packing (`6ac6ae3`/`84c9588`) and native process-CPU attribution
(`bd00534`). The 38-chunk baseline spends 68.628 of 73.457 seconds inside native
compute. CPU-only ABBA packing improves roughly 12.5 → 9.1 ms; regressed mask
strips are withdrawn (`7ca004b`). 59 LibTorch tests, four GGML frame tests and
CPU ABI/primitive checks pass. Existing per-operator fences are unchanged.

**Current model source `0b76748`, private `event-build`:** bounded measurements
locate 1.968515 CPU seconds inside the original 1.97310-second final wait of a
steady chunk. A tested non-profiling XPU completion event now permits short
CPU sleeps before the still-required full-device synchronization. Only
RoFormer is routed; query errors propagate, with no retry, fallback, concurrent
execution or changed arithmetic/cancellation checks. The bounded candidate uses
0.045115 CPU seconds in 1.93321 s forward wall; full-output SNR is 143.13503 dB
against the matched packed frontend, max 2.384185791e-7 over 1,058,400 finite
samples. The full-song pair now measures **75.73 → 11.51 process CPU seconds
(84.80% less)**, or **98.34% → 14.25% of one logical CPU** over sampled process
wall time. Inference is **74.58065 → 77.56927 s**, about 4% worse in this pair;
compiler/render contention prevents clean attribution, **not proof of a speedup
or proof that contention explains all regression**. The low-CPU path is retained,
but its uncontended latency still needs qualification. No current-path 64.53 s
or under-60 claim is made. Full waveform SNR is **137.84049 dB**, max
**1.102685928e-6** over all 31,300,416 finite samples. Both exit 0, with zero
process sample read errors and no sampled other CCS clients. Raw render-counter
aggregates exceeding 100% are not occupancy evidence.

Three more numerical-only active-family checks pass (XE90 instrumental,
denoise, dereverb: SNR 139.99986 / 144.56970 / 144.03378 dB against retained active
controls; 1,058,400 finite values each). Their observer errors are 1/1/0. Boots
are unchanged, not proof of GPU-reset absence or post-exit stability. Harmony
and PolarFormer failures were not retried. Final format-only commit `a0a3fe6`
and 59 LibTorch + four GGML frame + four stage-profile tests pass. No installation,
new GPU execution queue, listening or family-wide qualification. Next work must
separate wait-marker latency from contention and optimize the GPU critical path;
pure-GPU frontend speedup is still unmeasured. See `event-full-comparison.json`,
`event-family-comparison.json`, and the CPU optimization section of
`docs/ROFORMER_B580_LIBTORCH_XPU.md`, including native-build/CPU-check outer-timeout
provenance.

Kernel journal review is unavailable due to permissions
(`20260910T205622-b1bff200266f`); do not claim absence of GPU reset from boot IDs.
Do not alter GPU clocks/power settings or resume counter stress tests. Prior
power-loss cause remains unresolved; process success does not prove later host
stability. Observe bounded runs and inspect anomalies rather than automatically
retrying. Installed assets and unrelated user changes remain untouched.

## All-resource real full-song LibTorch XPU — EXECUTION COMPLETE (2026-09-11)

**18/18 resources completed the real 216.88-second Chinese song on B580 XPU.**
This includes independent XE90 vocal/instrumental checkpoints, all six separators,
RMVPE/FCPE/Basic Pitch, GAME small/medium/large, real dual-input JBM555, full Qwen and
FireRed transcription, real forced alignment, and actual conditioned STARS/ROSVOT.
Older seventeen-resource counts and bounded-only XPU descriptions above are
historical. Authorization: `20260911T063344-cba0c64793b8`.

Evidence: `test-artifacts/libtorch-xpu-fullsong-real/summary.json`; durable
[per-model results, fixes and limitations](../../docs/LIBTORCH_XPU_FULLSONG_RESULTS.md).
Native speech wrappers and diagnostics were implemented; real RMVPE unvoiced
conditioning, complete STARS Chinese readings, PolarFormer unequal-width attention
and FLAC clipping/effective-depth issues were repaired in separate commits.
Final current publications are under `publications/`: 12 FLACs (ten full-song stems
plus two Harmony regression stems), all actually 32-bit and fully decoded/compared;
193,404,960 values, max absolute encoding error 2.3283064365386963e-10, zero clipping.
Focused final Rust checks: 123 passed, seven explicitly ignored; native CPU/XPU
primitive checks and three offline lexicon tests also pass. No inference was
repeated just to republish saved audio.

The original PolarFormer full-song run returned DEVICE_LOST after two chunks;
its child observer records exit 1 but its outer recorder completion is absent.
`58f05b5` preserves full attention context/scale while zero-extending V for fused
SDPA; corrected full-song execution completes in 46.626 s with 3,421,512 KiB peak
sampled resident VRAM. Kernel logs are permission-blocked, not proof of no reset
or later host stability. Harmony passes this song and its recorded old 12-second
failure input under current code. Prior failures and sample-read gaps remain.

**Remaining:** transcription accuracy/listening and whole-model numerical parity;
128 unresolved Qwen word timings; STARS uses an explicit FireRed Chinese alignment
branch with 46 unresolved words, not fabricated conditions or product substitution.
Runtime Manager/Analysis Engine still have no formal LibTorch product route.
Do not promote `integration_ready`/`production_ready` from diagnostic execution.
Source media, installed assets and unrelated user changes remain untouched;
no CPU/GGML inference fallback, Vulkan stress, workspace release checks or Nix
packaging were performed. See [execution design](../../docs/design/runtime/LIBTORCH_EXECUTION.md).

## Scoped LibTorch AMD ROCm 10 — LIGHTWEIGHT MODELS 9/9 PASSED (2026-09-11)

**The user-scoped lightweight lane passed nine of nine resources; full-song execution was not started.** The isolated Nix
shell and official **ROCm 10.0.0 + PyTorch 2.13.0** packages, including the Radeon 780M `gfx1103`
device package, were installed only under ignored test evidence. They did not replace the system
driver, global Python, installed model store, source media or prior runtime. Authorization operation:
`20260911T094255-176d942db7f9`; evidence root: `test-artifacts/amd-libtorch-rocm10/`.

The linked native runtime and a synthetic FCPE GPU contract passed. AMD's documented experimental
AOTriton switch initially made both Leap-width fused-attention oracle shapes execute; the PolarFormer
unequal-width shape remained finite but missed the existing NMSE threshold. The first
real twelve-second resource, `bs_roformer_leap_xe90_vocals`, then failed repeatedly without fallback:
first with `SIGBUS`, then a synchronized diagnostic reported an unspecified launch failure after the
first frequency feed-forward and sampled about 4.70 GiB GTT. Commit `074ea9e` bounded the wide
feed-forward hidden intermediate; all six complete projection/feed-forward GPU-to-double-CPU oracle
cases passed before the changed model was executed.

The changed real-audio run did not publish a result or advance beyond `completed: 0 / total: 2`.
The operator observed the AMD-connected display go black briefly and recover. Passive samples show
`kworker/...amdgpu-reset-dev` active from 10:11:59 through 10:12:02 UTC, reaching about 94–100% of
one CPU, while the target held about 3.81 GiB sampled GTT. After reset it made no protocol progress
and consumed nearly one CPU until the command timeout left it orphaned. Only the task-owned target
process group was sent `SIGTERM`; its observer then recorded exit `-15` after 1,841.906 seconds.
The outer operation has no completion record and remains unknown, while the child result and samples
are complete. Evidence: `bounded/observations/leap-vocals-feed-forward/`,
`bounded/diagnostics/leap-display-reset-sample-summary.json`, and operation
`20260911T104236-afa6881e15ef`.

After reviewing the incident, the user explicitly authorized one fix and one single-chunk test.
Commit `37eef2a` bounds normalization, QKV, rotary, SDPA, gate and output intermediates along the
independent RoFormer batch axis while every query retains its complete key/value sequence. A real
9.000-second excerpt contains 396,900 frames, below the Leap overlap-add step, and therefore emitted
`total: 1` then `completed: 1`. The run passed in 32.614 seconds with 1,823,576 KiB peak sampled GTT.
Both guide-vocal and instrumental publications decode as 9.000-second, 44.1 kHz stereo signed-32-bit
FLAC: 793,800 finite, non-silent values each. Exact float-stem reconstruction maximum error is
`5.960464477539063e-8`. Neither in-run nor immediate post-run passive samples list
`amdgpu-reset-dev`; the boot ID stayed unchanged.

This one success does not qualify the original twelve-second resource: that sweep remains zero of
eighteen, and the full-song phase remains unstarted. Pre-run, in-run and post-run AMD sysfs busy
readings stayed abnormally high at 69–99%, despite no visible compute-engine owner before launch, so
the timing is not a clean performance measurement and reset recovery is not established. Evidence:
`single-chunk/observation/result.json`, `single-chunk/case/evidence.json`,
`single-chunk/flac-verification.json`, and `single-chunk/observation-summary.json`.

The user subsequently authorized resuming the original twelve-second sweep; receipt:
`20260911T110231-4ad310e0db43`. Three resources passed with explicit `libtorch_rocm` and no fallback:
Leap vocals completed two chunks in 64.153 seconds at 1,823,576 KiB sampled peak GTT; the independent
Leap instrumental checkpoint completed two in 64.035 seconds at the same sampled peak; PolarFormer
completed in 45.717 seconds at 1,489,372 KiB. Their complete twelve-second output stems are finite and
the residual reconstructions remain at float epsilon.

The fourth resource, Denoise over the actual Leap guide-vocal output, completed five of six chunks
then received `SIGBUS` after 34.978 seconds. It published no result. Peak sampled GTT was only
1,807,832 KiB, so the former approximately 4 GiB allocation peak is not this failure's explanation.
The operator again observed the AMD-connected screen go black briefly, and the final passive host
sample records `kworker/u64:3+amdgpu-reset-dev`; AMD busy remained 76–99% throughout. Evidence:
`bounded-resumed/observations/denoise/` and
`bounded-resumed/diagnostics/denoise-failure-summary.json`.

At the user's request, the shared attention risk was reviewed before one Denoise retry. Six
RoFormer resources, three GAME sizes and two Qwen resources could select fused SDPA on ROCm.
Commit `0fa8110` replaces that production ROCm route with query- and batch-bounded GPU mixed
attention while retaining every key/value row, masks and grouped-query semantics, and removes the
gfx1103 AOTriton opt-in. Three complete-context RoFormer geometries plus GAME additive-mask, Qwen
boolean-mask/GQA and causal oracles passed **6/6** against complete rounded-input double references;
no experimental fused kernel was used.

That result did not fix Denoise. With preflight AMD use at 2%, the authorized retry loaded the real
model and failed during its first of six chunks with `SIGBUS` after 5.619 seconds. It published no
result. The observer sampled the AMD device and ROCm libraries, 1,818,668 KiB peak GTT, and
`kworker/u64:6+amdgpu-reset-dev` in the final host sample. No model process remained. This third
reset falsifies AOTriton as the sole cause; it does not establish that the experimental fused path
was safe. Evidence: `repair-review/attention-check/` and `repair-review/denoise-retry/`.

Offline GGUF inspection then identified an uncovered Denoise mask-estimator contraction: each band
could submit one `801 x 1536` by `1536 x 1536` GEMM, about 1.89 billion multiply-accumulates, because
that private path bypassed projection tiling. Commit `8268ab9` routes those layers through the shared
ROCm projection path, initially bounded each submitted GEMM to 268,435,456 multiply-accumulates, and
added per-layer trace checkpoints plus the actual square-projection oracle.

The user explicitly directed same-boot continuation after preflight showed the driver attached and
AMD use at 2%. The actual Denoise square projection passed all 1,230,336 values with row tile 113,
NMSE `3.41814423231e-13` and maximum error `3.38207630932e-5`. The next large projection case then
received `SIGBUS` after 4.371 seconds. It had been over-partitioned from the previously passing row
tile 1024 to 455, increasing submissions from about 59 to 132; its sampled 688,644 KiB GTT was close
to the prior passing run's 692,616 KiB. The final sample records
`kworker/u64:3+amdgpu-reset-dev`. Denoise was not launched. Evidence:
`mask-projection-resume/projection-check/`.

Commit `4fc2739` corrected that scheduling regression. It uses the already passed
`1024 x 384 x 1536` contraction as the work bound: transformer projections retain tile 1024 while
the private `1536 x 1536` mask projection remains split at tile 256. The complete projection/FFN
oracle then passed **7/7**, including 92,160,000 values for the restored large projection and
1,230,336 values for the square mask case, with no active reset worker sampled.

A synchronized Denoise run next completed band split and first-layer normalization but surfaced an
unspecified launch failure at the first time-attention QKV synchronization point; no active reset
worker was sampled in that attempt. Commit `e0da4a8` replaced temporary linear outputs plus
asynchronous slice copies with direct `mm_out`/`addmm_out` writes. Its expanded oracle passed
**8/8**, including all 9,842,688 values of the exact synthetic `6408 x 384 -> 1536` QKV geometry.
The real Denoise trace nevertheless failed at the same QKV synchronization point and its final host
sample recorded `amdgpu-reset-dev`.

Commit `b4bdb20` added trace-only per-projection-tile synchronization. The next Denoise run failed
before any such tile checkpoint: the exception surfaced while `stack/cat` allocated the combined
band representation after sixty asynchronous band-split projections. Its final host sample records
a separate `amdgpu-reset-dev`. This proves the earlier QKV label was only the next synchronization
surface, not a stable causal stage.

Commit `7934e08` removes that band-split lifetime pattern on ROCm. It preallocates one contiguous
`[band,time,channel]` destination, writes every band projection directly into its own slice, removes
the sixty retained projection outputs and final stack allocation/copy, and gives trace mode a
per-band synchronization point. In the authorized trace, all **60/60** band projections and the
combined band-split checkpoint completed.

The same trace then completed the first two 1024-row QKV projection tiles and failed on the third,
`start=2048, rows=1024`; the final sample records `amdgpu-reset-dev`. The actual input is
`[8,801,384]`, so commit `b26ca72` instead kept each complete 801-row sequence together. Its exact
three-dimensional projection/FFN oracle passed **8/8**, including all 9,842,688 batch-aligned QKV
values at NMSE `9.1871552793e-14` and maximum error `4.57103271057e-6`; no reset worker was sampled.

Real Denoise nevertheless completed all 60 band projections and then reset on the first 801-row QKV
tile. This falsifies cross-batch tile boundaries as the cause. Commit `e8b06e9` now bounds each
ROCm attention subproblem itself to at most 1024 projected sequence rows. The 801/1722-row time axes
run one independent batch at a time, while short frequency axes retain up to eight batches. This
reduces simultaneous normalized/QKV/attention intermediates without removing any context or value.
The native build passed. Its real trace completed 44 long-axis attention batches before the next
QKV failure, and splitting Q/K/V moved later failures to normalization rather than eliminating them.
Valid serialization settings, quiet stage fences, pacing, smaller projection rows and reduction
channel tiles likewise moved the synchronization surface without making the workload repeatable.
One fenced run completed, but its immediate reproduction reset and therefore was not accepted.

Commits `b4de134a` and `458bea82` then introduced an in-process HIPRTC F32 contraction kernel with
arbitrary two-dimensional strides. RoFormer projection and FFN paths use it on ROCm without CPU
fallback, weight conversion or arithmetic truncation. Production-scheduled projection/FFN oracles
passed **10/10**, including the exact strided QKV geometry, a 60,000-row projection and complete
FFNs against double CPU references. Commit `3ac97dfa` generalized the same kernel to independent
head/batch groups and replaced both mixed-attention contractions, `Q x K^T` and
`softmax(scores) x V`; all complete-context, mask, causal and GQA attention oracles passed **6/6**.
Every custom contraction is synchronized and paced. The rejected `AMD_SERIALIZE_KERNEL=3` shell
setting was removed; PyTorch had treated it as an invalid boolean.

The first fully custom run still reset after excessive single-batch dispatches. Commit `2809b81`
removed the obsolete rocBLAS-era single-batch workaround and groups at most eight independent
attention batches, while projections remain internally bounded to 256 rows and every sequence keeps
its full K/V context. The real 12-second Denoise request then completed all **6/6 chunks twice** on
Radeon 780M `gfx1103`, once with synchronization trace and once without it. Execution times were
504.331 and 501.916 seconds; sampled peak resident GTT was 1,778,396 KiB in both runs. Each exact
output contains 1,058,400 finite samples with peak `0.8360749483`, RMS `0.1319012501`, zero out-of-range
or clipped samples and reconstruction error zero. The two complete F32 outputs are byte-identical.
Read-only hwmon samples measured maximum edge temperatures of 36 and 39 degrees Celsius. Evidence:
`grouped-attention/denoise/` and `grouped-attention/reproduction/`.

Denoise is therefore a reproduced bounded functional pass, not the former one-off success. It does
not establish driver root cause, broad host stability, throughput acceptability, whole-model parity,
listening quality or production readiness.

The user then narrowed further AMD execution to lightweight models, explicitly excluding all six
audio separation/cleanup resources and the two Qwen resources plus FireRed. Nine real twelve-second
runs completed with `libtorch_rocm` and no fallback: FCPE `0.403 s`, RMVPE `0.408 s`, Basic Pitch
`0.306 s`, GAME small/medium/large `7.137 / 10.114 / 19.122 s`, dual-input JBM555 `0.972 s`,
ROSVOT `0.712 s` and STARS `1.387 s`. Every process exited zero with an unchanged observed boot,
and every numeric evidence value was finite. Outputs contain 1,201 FCPE frames, 1,201 RMVPE frames,
1,033 Basic Pitch frames, 24/23/22 GAME notes, three JBM555 notes, 23 ROSVOT notes, and 28 STARS
notes plus 51 technique and eight style segments.

ROSVOT and STARS used the current AMD RMVPE result and a retained alignment from the previous XPU
execution of the byte-identical decoded source; Qwen was not executed in this sweep. The first
ROSVOT setup attempt rejected the product `items` representation before inference. A derived native
checker representation retained every measured interval and timing issue; the corrected isolated run
passed, with 18 resolved words and five unresolved words intentionally unused. This is an input-shape
setup record, not a model or GPU failure. Postflight AMD use was 0% and no task process remained.
Evidence: `test-artifacts/amd-libtorch-rocm10/lightweight-sweep/summary.json`.

The active requested scope is **nine passed of nine**. The six audio resources and three excluded
speech resources are not pending in this scope, no full-song execution was launched, and no result
promotes driver stability, whole-model parity, listening quality or production readiness. CPU/GGML
fallback remains prohibited. See [LibTorch execution](../../docs/design/runtime/LIBTORCH_EXECUTION.md).

## Same-device Radeon 780M GGML Vulkan comparison — 9/9 passed (2026-09-11)

The same nine 12-second workloads subsequently completed through the pinned GGML Vulkan worker on
physical Vulkan index 1, explicitly resolved as `Vulkan1`, AMD Radeon 780M, RADV and
`integrated_gpu`. Every model was resident before its measured run; no CPU, B580 or LibTorch route
was selected. JBM555 used the same real mix and guide-vocal pair. ROSVOT and STARS used the same 18
resolved words, excluded the same five unresolved words, and consumed this sweep's Radeon GGML
RMVPE evidence without executing Qwen.

Prepared-worker run wall times were FCPE `0.201 s`, RMVPE `0.503 s`, Basic Pitch `0.370 s`, GAME
small/medium/large `0.648 / 1.211 / 2.282 s`, JBM555 `0.331 s`, ROSVOT `0.281 s`, and STARS
`0.811 s`. Against the prior same-machine ROCm measurements, GGML was faster for FCPE (`2.01x`),
all GAME sizes (`11.01x / 8.35x / 8.38x`), JBM555 (`2.94x`), ROSVOT (`2.53x`) and STARS
(`1.71x`); ROCm was faster for RMVPE (`1.23x`) and Basic Pitch (`1.21x`). The GGML measurement
includes canonical-WAV decode/copy and evidence publication, whereas the retained ROCm execution
clock excludes initial resampling, so these are practical same-device comparisons rather than an
identical-boundary kernel benchmark.

All nine GGML outputs were finite and reported `backend: ggml_vulkan`. Frame, voiced-decision,
note, technique and style counts match the prior ROCm evidence in all nine rows; this count/shape
agreement is not strict numerical parity. The first orchestration completed seven models before
ROSVOT rejected a native-checker-only RMVPE JSON shape before inference. Replacing it with the full
worker evidence from this sweep let ROSVOT and STARS pass; this was an ordinary input-shape correction,
not a GPU failure. The boot stayed unchanged, sampled AMD utilization reached 90%, sampled edge
temperature reached 52 degrees Celsius, postflight utilization was 0%, and no task process remained.

For the unexecuted 216.88-second song, linear extrapolation for non-GAME models and eight real GAME
windows estimates `78.25 s` for all three GAME variants together, versus `366.68 s` from the ROCm
measurements. Selecting GAME medium only estimates `54.81 s` versus `156.61 s`. Runtime/model load
is excluded, and these are estimates rather than AMD full-song executions. Evidence:
`test-artifacts/amd-libtorch-rocm10/lightweight-sweep/amd-ggml-comparison/summary.json`,
`fullsong-estimates.json`, and `output-shape-comparison.json` in that directory.

## LibTorch XPU production route — IMPLEMENTED (2026-09-11)

The user authorized promoting native LibTorch XPU execution to production level after the recorded
full-song results measured it faster than the GGML route. Commits `c9c2138` (Runtime Manager +
Engine), `995dd07` (worker route and Python-free installer), `8e33711` (Studio settings backend
selection) and `822eecf` (readiness hint) implement it; details are in
[LibTorch execution](../../docs/design/runtime/LIBTORCH_EXECUTION.md#product-route--implemented-and-selectable-2026-09-11).

- `runtime:libtorch_xpu` is a catalog runtime with a production-pinned `libtorch_xpu` capability;
  every model advertises the capability beside its pinned `ggml` default. Readiness requires
  `lib/libuta_libtorch.so` under the installed runtime directory (`native_library_missing` otherwise).
- The Engine dispatches `backend: libtorch_xpu` on the discrete GPU only; the worker opens each model
  through the native C ABI with the qualified precision policy and publishes the same typed artifacts.
- Studio's Settings > Models & runtime offers *Compute backend* (pinned default / GGML Vulkan / LibTorch
  XPU) and lists LibTorch in every per-model runtime menu. Selection never falls back.
- Focused checks: runtime-manager 35 + 10, analysis-engine 278, worker 38, app-core 434 (four
  real-CLI tests need a built debug `uta-analyze`), desktop settings 9 tests pass.
- The previously used isolated XPU torch tree under `test-artifacts/` no longer exists; the runtime is
  reinstalled Python-free into `~/.local/share/uta-studio/runtime/libtorch-xpu` by
  `install-libtorch-xpu-runtime.sh` (curl + unzip + CMake).

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

## Fusion activity and lyric timing follow-up — 2026-09-14

Current evidence supersedes the historical `d5d1df49` snapshot below. The same
two CSD recordings remain calibration; the three previously scored recordings
are regression checks, **not a new heldout sample**. Native note-expert credit
is deduplicated and located at observed vocal recovery; explicit unpitched
states represent unsupported time without weakening exact path coverage.
Independent `LyricTiming` keeps successive words on their selected melody note
without fabricating attacks at word boundaries. Short notes are not filtered,
all model candidates remain available, and installed runtimes are unchanged.

Versus `d5`, calibration extra cuts fall **13 → 9**, onset/pitch F1
**84.79% → 85.92%**, and F1 with offsets **51.15% → 58.69%**. Prior-validation
regressions fall **35 → 25** cuts, with F1 **85.21% → 85.68%** and offset F1
**59.38% → 64.43%**. Matched onsets decline 184 → 183 and 386 → 383,
respectively. Standalone GAME still leads calibration onset/pitch F1 at
**90.56%**. Vocadito A1 declines to **64.62% / 30.77%** onset/offset F1, with
9 → 11 cuts and 0.160 → 0.359 seconds uncovered. Aggregate improvement does
not mean every recording improves. Calibration-only utility simulation supports
retaining 0.6; no regression annotation selected parameters.

The original Japanese song preserves **26 original timed LRC scopes and all
361 characters**. Lexical segmentation changes 357 character units to 236 word
units; unresolved units 179 → 98 are not comparable denominators. The fair
unresolved-character comparison is **181 → 144**. Long repeated-vowel Korean
alignment still regresses **169 → 186** unresolved units. The reviewed timestamp
postprocessor agrees with the official implementation; long-context numerical
and singing-quality equivalence remain unestablished.

Retaining the prior six non-alignment outputs with new Qwen evidence produces
**499 pitched notes / 12 below 100 ms**, versus `d5` **505 / 20**. A separate
fresh **B580 seven-model** execution produces **493 / 11**; all 361 characters
and original caller scopes remain. Evidence:
`test-artifacts/singing-boundary-alignment-followup/results-summary.json`,
`fresh-execution-summary.json`, and the Qwen context reports in that root.

Final-source CPU replays now preserve all supplied text in order, including
kr002a's 231 characters, without changing selected note IDs/geometry, canonical
data or any lyric token/time. All human-note evaluation values remain identical.
Engine 387/Core 522/Desktop editor 14 tests pass at `230020b1`, with strict
all-target Clippy and debug builds passing. The corresponding ignored tests
remain two/one/zero. Subsequent evidence-validity synchronization at `edf8954e`
passes Core 524 tests (one ignored), strict Core/Desktop Clippy and a final editor
build, matching the Engine's existing rules. This is not a whole-workspace release
pass.

The final save-path repair at `e2ae98f0` keeps UTZ-valid empty lyric slots as
warnings rather than blocking saving. Core **527 passed, one ignored**, Desktop
editor **14 passed**, strict Core/Desktop Clippy and the final editor build pass.
A B580 editor session atomically saves a 10 ms independent-lyric shift in a
separate fixture; readback preserves all 71 note geometries, 44 text tokens and
12 empty slots. The UI shows zero errors and 25 warnings. True format errors
still block saving. See `test-artifacts/public-unpitched-ui/save-validation.json`
and the measurement report for final receipts.

B580 Wayland captures verify the two original lyric regions and the public
unpitched interval. Native public-WAV playback preserves pause/play intent through
lyric jumps, with 26.697 seconds observed running and unmuted. Application-stream
ERR is zero; HDMI-driver ERR is seven from first active observation without later
growth, but its earlier suspended row was zero. Startup error attribution and
human listening qualification remain open. No build/inference ran during audition.
Receipts and exact limits are in the measurement report and
`test-artifacts/public-unpitched-ui/validation-summary.json`.
21J remains `NEEDS_REVIEW`, with model integration/production readiness unchanged.
See [current fusion follow-up](../final-features/followups/21J_MELODY_PATH_SCORE_COHERENCE.md#fusion-activity-and-independent-lyric-timing--2026-09-14-jst)
and [measurement method](../../docs/NOTE_TRANSCRIPTION_EVALUATION.md).

## Imported lyric fidelity and note boundaries — 2026-09-14

**Historical d5 snapshot.** The current follow-up above supersedes these
measurements and next actions; this section retains the earlier evidence.

Fresh seven-model native inference plus same-evidence fusion/projection replay
now verifies reduced fragmentation. Five public recordings total 383.671 seconds
and 642 human notes; two are calibration and three were scored only after final
source selection. On the three validation recordings, extra internal cuts fall
**308 → 35**, onset/pitch F1 **61.0% → 85.2%**, offset F1 **35.0% → 59.4%**, and
recall **87.5% → 89.6%**, with missing coverage unchanged.

The source combines resolved Basic Pitch peaks, shared acoustic attacks,
physical-event reward deduplication, continuous-pitch duration scoring and
ordered lyric ownership projected onto existing local note edges. Imported text
remains intact even when timing fails; the reported held-note word split and
empty LRC prefix are repaired. Asphodelos now has **505 pitched notes / 20 below
100 ms**, versus **588 / 52** for the same fresh evidence under the control.
All 361 characters, word measurements, caller scopes and raw F0 remain intact;
179 alignment units are still marked unresolved.

At source `d5d1df49`, Engine **364 tests pass, two ignored**, strict Engine Clippy
passes, and debug CLI/example builds. The final Vocadito chart exports to both
UTZ and UltraStar with all 68 notes and 129 characters; both full FLAC decodes
and focused export regressions pass. Long tails, some displaced expert onsets,
Japanese unresolved timing and final continuous audition remain open. Standalone
GAME is still stronger on calibration onset/pitch F1. 21J stays `NEEDS_REVIEW`;
model integration/production readiness and installed user charts are unchanged.
See [current fusion evidence](../final-features/followups/21J_MELODY_PATH_SCORE_COHERENCE.md#public-data-fusion-repair--2026-09-14-jst)
and [public-data measurements](../../docs/NOTE_TRANSCRIPTION_EVALUATION.md#public-singing-measurements--2026-09-14).

## Note-fusion regression — source repaired, real-song quality review open (2026-09-11)

The current Studio Japanese-song run reached `singing-fusion` after its six pitch/note
experts completed, then failed with `one duration state exceeds the bounded pitch-proposal
limit`. Its temporary evidence had already been cleaned by the application; the lifecycle
log alone cannot reproduce that exact pool. `06430702` replaces per-overlapping-note pitch
expansion with one overlap-duration-weighted median per expert and duration, retaining raw
notes and exact fractional pitches. `d9e0e842` shares the corroborated acoustic onset detector
with both F0-consolidation checks, so flux-only fluctuations no longer veto stable-note
consolidation. Existing limits, caller/word/gap protections and continuous F0 are unchanged.

Five new regressions reproduce and repair these mechanisms. All 71 fusion tests, 287 Engine
unit tests plus four packaged-boundary tests (serial), and targeted all-target clippy pass.
An earlier parallel suite had one unchanged acoustic-cache fixture failure; its isolated and
serial executions pass, but the intermittent cause remains unknown. App-core's 93-test export
filter includes nine UltraStar chart/publication tests; no real audio bundle export occurred.

Read-only cached comparison: a matched 12-second multi-expert sample drops from 577 to 520
candidates (9.88%), with maximum pitch states per duration 11 to 5. Both select twenty pitched
notes with identical times/MIDI/lyrics/F0; two cents values change. A separate 216.88-second
F0-derived song remains chart-identical with 527 pitched notes, 231 under 100 ms. **Real final-note
fragmentation reduction is not established.** Card 21J is `NEEDS_REVIEW` for this current quality
regression; model integration/production readiness is unchanged. No new GPU inference, user-data
mutation or installed build. Evidence and next action:
[21J current follow-up](../final-features/followups/21J_MELODY_PATH_SCORE_COHERENCE.md#current-regression-follow-up--2026-09-11-utc),
`test-artifacts/note-fragmentation/comparison.json`.

## Caller word alignment — repaired input granularity; new measured output pending (2026-09-11)

The user's subsequent Japanese-song analysis completes and publishes a chart, confirmed by its
retained lifecycle log. The new complaint is real: alignment has 47 whole-line `word` items, of
which 16 are unresolved, instead of character-level measurements. `qwen_alignment_words` passed
nonempty caller tokens through without segmentation; those tokens are LRC lines/search scopes.
`d8031076` now applies language-aware lexical segmentation to caller lines and generated text,
retaining the original caller scopes without dividing audio time by character/note counts.
`6136aa20` additionally prevents unresolved words from shifting later words into earlier lyric
lines: measured words are grouped by actual caller-scope overlap, not compressed text offsets.

Five new regressions reproduce these defects and pass after repair. The latest serial suite
passes 292 Engine tests plus four packaged-boundary tests, and targeted all-target clippy passes.
An explicitly invoked CPU diagnostic over the retained real lyrics produces **507 lexical input
units in the same 47 caller scopes**, preserving all text and unique word IDs. These are prepared
model inputs, **not newly measured word timestamps or a corrected real-song chart**. Nine
UltraStar tests and Engine UTZ chart finalization tests pass; no real export bundle was created.

Card 21J remains `NEEDS_REVIEW`. Next: fresh alignment plus conditioned note/fusion/finalization
stages with the corrected backend, then inspect actual character/note correspondence. Existing
47-line alignment cannot be repaired by evenly splitting it. Source and user caches/models remain
untouched; no new GPU inference or installed build. Details and operation receipts:
[caller word alignment](../final-features/followups/21J_MELODY_PATH_SCORE_COHERENCE.md#caller-word-alignment-follow-up--2026-09-11-utc),
`test-artifacts/word-note-alignment/word-request-summary.json`.

## Sony MIMO separation — feasibility reviewed, native integration open (2026-09-11)

The user requested an opt-in slower/higher-quality method and SDR-style evidence in DAG Inspect.
Sony's trained MIMO graph is not an inference wrapper for existing XE90 weights: it needs four
input channels, cross-source masks, time conditions and value residuals plus its own checkpoint.
The official matched large-model museval result improves vocals/accompaniment by 0.22/0.56 dB;
no comparison with our XE90 or local speed/quality benefit has been measured. Native graph/import,
Runtime Manager resource, workflow routing and the requested default-off Analysis switch remain
**open**, not integration-ready/production-ready. No dummy switch or Python model route was added.

Implemented independently: Engine separation-output decode measurements, exact-run/node read API,
and DAG Inspect signal statistics with units/provenance. No-ground-truth SDR/SI-SDR/SIR/SAR are
explicitly unavailable. No cached or later cleanup audio is substituted. Focused tests pass;
no new model/GPU execution, model installation, source mutation or release checks. Full details,
verification receipts and next steps: [MIMO separation](../../docs/design/audio-analysis/MIMO_SEPARATION.md).

## Model quality controls — IMPLEMENTED, bounded verification complete (2026-09-11)

The user required real per-model quality controls, explicitly **Overlap**, after upstream research.
Settings → Analysis now lists all eighteen resources and exposes only implemented native parameters:
independent overlap for all six RoFormer/PolarFormer separation/cleanup models; GAME steps and
boundary/voicing thresholds; RMVPE/FCPE voicing; JBM555 onset/offset; STARS/ROSVOT boundaries;
Qwen ASR and FireRed window token budgets. Basic Pitch's raw-activation route and Qwen timestamp
classifier explain why upstream MIDI/generation controls do not apply. Upstream batch support is
reported separately; no fake native batch slider or script/network fallback was added.

Both native routes consume the controls; all audio preparation branches are wired. Super preserves
quality settings, and exact request snapshots plus existing Step 1 cache recipes include them.
Changing overlap cannot reuse a stem produced under another setting; live publication uses the running
request snapshot. Model-file overlap defaults are displayed as **Default**, not guessed. Factor-one
no-overlap processing no longer fades seams to zero. Settings provide minus/editable-value/plus,
Apply/Enter, per-model reset and visible transactional save errors; source media and installed assets
are untouched. Source/search ledger: [Model quality settings](../../docs/MODEL_QUALITY_SETTINGS.md).

Final focused operation `20260911T190236-c2f2a2b4c21f` passed 293 Engine tests (one ignored), 40 worker,
two parameter, 466 app-core (one ignored), 257 desktop, 134 GGML host (38 explicit native tests ignored)
and 79 LibTorch host tests (one ignored), plus the desktop debug build. CLI/worker debug builds passed
`20260911T190742-1ad670dc5764`; four packaged CLI tests passed `20260911T190836-14230df22400`.
Targeted formatting and docs check passed `20260911T190752-a01ed826fe55`. Earlier bad shell invocation,
fixture-shape/default assumptions, Bevy focus API compile error and UI-source inspection failures remain
recorded; they were repaired before these passes.

Isolated Weston/Wayland + lavapipe smoke `20260911T190432-34a5e04cce0e` completed 42 dispatch steps:
all eighteen models selected, sixteen independently saved, overlap adjust/Apply/reset/error/clamp checked,
Super still enabled. Persisted JSON matches the report. Fresh-process 1000×900 reload smoke
`20260911T190543-3e438c424b08` retained GAME steps and wrapped controls; 1440×1000 and narrow screenshots,
reports and machine-reviewed summary are under `test-artifacts/model-quality-settings/ui/`.
This is command-dispatch/rendering coverage, not physical keyboard/mouse or real inference evidence.

Clippy is **not clean**: strict operation `20260911T190549-8b3413d9d182` stopped at existing
`backend_cli/process.rs` and `debug_logging.rs` warnings. Advisory review `20260911T190630-a5638229fc5f`
also found an existing denied permission-literal lint in `log_storage.rs:369` plus Settings models/widgets
warnings; none were in this task's changed logic. Unrelated work remains intact. No GPU/model run,
real-song quality measurement, listening, Windows, installed executable replacement or release packaging;
`integration_ready`/`production_ready` model qualifications are unchanged.

## Next actions

1. Install the STARS and ROSVOT GGUF generations into the managed store with their manifests and
   digests, and confirm `uta-runtime resolve` answers for both.
2. Act on `docs/PERFORMANCE_DIRECTION_2026-09-09.md`, in its measured order: overlap host work with
   GPU execution, then make the CPU frontend fast, then cross-GPU chunk splitting, session reuse,
   input caching, weight prefetch.
3. Revisit the historical GPU-versus-reference differences (Basic Pitch `0.00344658`, RMVPE's
   `282.36` Hz frame) now that Vulkan F32 matmul is exact; they were most likely that rounding.
4. Add reference comparisons still missing per model: STARS/ROSVOT note evidence, PolarFormer,
   Denoise, Dereverb, and Harmony former-implementation vectors, and FCPE layerwise vectors.
5. Compare RMVPE frontend/CNN/GRU/decoder intermediate tensors around the remaining low-confidence
   onset difference.
6. Complete workspace formatting/check/test/clippy, product build, Nix packaging, identity scan, and
   model-subprocess scan during the explicit release pass.

## Release status

| Scope | State | Current conclusion |
| --- | --- | --- |
| Model backend alignment to upstream GGML | DONE | All seventeen catalog models execute through Rust-owned graphs on the pinned upstream shared libraries. No app-owned C/C++ model source and no tracked model script remain. |
| Current Rust GGML integration | NEEDS_REVIEW | Focused control-plane, runtime, worker, and desktop suites pass, and fifteen of seventeen models executed on both AMD 780M and Intel B580. FireRed's whole-chain decoder, the installed/pinned GGUF container mismatch, the missing Qwen ASR F16 artifact, strict numerical/perceptual parity, and runtime-manifest hardening remain. |
| Final repository/package acceptance | PENDING | Whole-workspace, packaged-product, and Nix release checks are reserved for the explicit release pass. |
| Production model release | PENDING | Do not infer production readiness from historical implementations or current smoke runs. |
| LibTorch XPU product route | IMPLEMENTED | Runtime Manager, Engine, worker and Studio settings expose `libtorch_xpu` as an explicit production-pinned second route; verification status is recorded in the LibTorch XPU production route section. |

## Operation provenance

Per the user's 2026-09-07 direction, each independent change and subsequent execution is committed and recorded with `tools/record-operation.py`. A missing completion record means unknown outcome. See `docs/ROFORMER_OPERATION_RECORDING.md`.

## Qwen XPU power-off follow-up — unverified source candidate (2026-09-15)

Earlier inspected production evidence reaches Qwen ASR **strict XPU** decoder layer
zero, with QKV/cache completion followed by `decoder.attention_tile` await at
16:55:45.462 JST. The actual native library reports source `bc587e29` plus dirty
changes; this is not solely a stale-worker/build mismatch. Nearby integrated-GPU
FCPE progress and buffered-detail tail loss prevent attribution to one operator
or an exact power-off instant. No prior-boot kernel evidence was obtained.

Candidate `a7dd3508` replaces only Qwen strict XPU attention's repeated GQA cache
and broadcasted matmul chain with per-physical-KV-head FP32 matrix contractions,
explicit operator completion and durable operator diagnostics. Complete causal
and acoustic-window context, masks, precision, cancellation and error propagation
are retained by design; no backend substitution, retry or CPU fallback was added.
The CPU-only Qwen check now invokes this production helper against independent
double references and covers spare-cache poisoning, strided views, masks, tails,
output ownership and completion failures. **New C++ checks are not compiled or
executed.** Source whitespace and canonical product identity checks passed;
no model/GPU execution, installation or settings changes occurred in this task.

Next: the user's rebuild must include the native `libuta_libtorch.so`, not only
the Rust worker; CPU oracle execution and actual stability verification remain.
`integration_ready`/`production_ready` are unchanged. Full evidence, source
analysis, operation receipts and uncertainty are in
[the current LibTorch incident record](../../docs/design/runtime/LIBTORCH_EXECUTION.md#qwen-xpu-power-off-follow-up--code-candidate-not-stability-acceptance-2026-09-15).

## Latest production replay — Qwen query-view candidate (2026-09-15)

The user's next run loaded clean native source `203c026a`, including the prior
strict attention change. Leap strict XPU completed all 24 mask invocations and
unloaded; RMVPE, FCPE and Basic Pitch also completed their Engine nodes. Qwen
completed its first window (1/31), encoded the second, and advanced to decoder
`dec.blocks.13.`. The retained tail is now `qwen.strict.query_pack` await at
**17:42:41.992 JST**, not the earlier first-layer attention boundary. Its 16,064
completed operator triplets pair correctly; only that final operator is pending.
Nearby integrated-GPU GAME execution and absent incident-time kernel errors
prevent assigning the power loss to one kernel. The kernel capture retained
100 boot-time records, not a diagnosis. Whole-machine stability still failed.

Candidate `cb3eda6` removes the app-owned cross-head Q gather by using per-head
`select`/`narrow` views and shared physical KV data. FP32/full-context attention,
mask mapping, completion, error propagation and cancellation remain; there is
no retry, backend substitution, configuration change or GPU run. Per-head work
adds completion calls; neither speed nor a stability improvement is measured.
CPU-only regression source now covers narrowed 122-row prefill (64/58), inferred
378-row cache capacity with poisoned spare rows, Q view alias/stride/offset
properties and independent double numerical references. **No C++ build or CPU
oracle execution occurred.** Source diff/identity checks passed. The user must
rebuild the native DSO as well as Rust before testing this candidate.

Detailed evidence and operation receipts:
[latest LibTorch production replay](../../docs/design/runtime/LIBTORCH_EXECUTION.md#latest-production-replay--qwen-query-view-candidate-2026-09-15).
No `integration_ready` / `production_ready` promotion.
