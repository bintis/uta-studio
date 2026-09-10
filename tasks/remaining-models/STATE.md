# Remaining Models + Final Feature Closure — State

**Updated:** 2026-09-09 (after the Intel B580 sweep)
**Owner:** Rust + upstream-GGML migration

This file stores current effective state only. Historical execution evidence remains in its original records; current source and focused tests override stale historical conclusions. Durable cross-cutting conclusions live in `docs/KEY_CONCLUSIONS.md`.

## Current product model set

Studio has one model execution boundary: Rust-owned graphs calling the shared libraries built from upstream `ggml-org/ggml` revision `8c63e70982c95ceb862e3a1073a2c1beef75d60a`. The package is upstream GGML plus exactly the backend patches `native-inference/ggml-worker/runtime-recipe.json` declares, and contains no app-owned C/C++ model graph, shim, model CLI, or model-inference subprocess. The repository contains no model conversion or model-rewrite script; container migration is `cargo xtask gguf`. Vulkan remains the default. CPU is an explicitly selected experimental reference mode; GPU and integrated-GPU requests never fall back to it.

The backend alignment is complete: every authorized model now runs through that one boundary, and the Runtime Manager catalog contains exactly these seventeen models and one `ggml_vulkan` shared-library runtime.

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
| `stars` | `notes.stars`, `technique.analyze` | NEEDS_REVIEW | yes | no | Timed-transcript-conditioned note/technique evidence; depends on `rmvpe`. Stage C and Stage E were validated against checkpoint-truth oracles on CPU and AMD 780M. A full B580 worker run completed with 24 notes plus technique evidence. It required the migrated GGUF; the installed container fails to load. Reference comparison remains. |
| `rosvot` | `notes.rosvot` | NEEDS_REVIEW | yes | no | Timed-transcript-conditioned note evidence; depends on `rmvpe`. CPU, AMD 780M, and B580 worker runs completed; the B580 run produced 15 notes. It required a regenerated migrated GGUF; the installed container fails to load. Reference comparison and long-input coverage remain. |
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

Real AMD 780M smoke evidence exists for RMVPE, FCPE, Leap, PolarFormer, Denoise, Dereverb, Harmony, Basic Pitch, GAME small/medium/large, JBM555, ROSVOT, Qwen ASR, the Qwen aligner decoder, and the STARS/FireRed stages and model loads listed above.

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

## Super acceleration — IN_PROGRESS, GPU validation paused (2026-09-10)

**User correction:** multi-GPU means assigning different complete model tasks using dependencies,
loading order and predicted total device completion time, **not splitting one model across GPUs**.
Retain the existing opt-in `turbo_acceleration` snapshots, memory-budgeted actual weight hot loading,
exact-format decoded-audio reuse, shared separation outputs and useful Qwen encoder-to-decoder
residency. Each model remains on one device with its precision, chunks, synchronization and output
semantics unchanged. Complete-model task scheduling is not implemented yet; the linear Engine
orchestrator and global foreground lease require an ownership-aware redesign, not lock removal.
`844c016` removes the old dual-chunk implementation; its timings do not qualify the corrected goal.
The correction passed **53 CPU / isolated protocol tests** (four RoFormer, 34 worker, thirteen
supervisor and two prediction tests; `20260910T095055-5df40fd72c78`). Identity scan passed
(`20260910T095215-585b34c730c4`). No GPU inference or new release build was run for this correction.
Design and boundaries: [Super acceleration](../../docs/design/runtime/SUPER_ACCELERATION.md).

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

Next: dependency-aware complete-model/device queues, observed task
phase/cost accounting and queue-aware hot-weight retention/preparation. Qwen reduced readback must
preserve first-maximum and all-logit finite checks: the pinned Vulkan argmax lane tie rule cannot be
substituted directly. Full-model numerical checks, cross-song reuse and Studio publication timing
remain incomplete. Safety review follows the linked restart/submission/upload records in the
design document: preserve synchronization/cleanup; do not add arbitrary waits, limits or retries.
GPU experiments stay paused after the user's restart report; no automatic repeat of the incomplete
run. Whole-pipeline performance/output qualification for the corrected design is incomplete.
No production promotion.

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

## Operation provenance

Per the user's 2026-09-07 direction, each independent change and subsequent execution is committed and recorded with `tools/record-operation.py`. A missing completion record means unknown outcome. See `docs/ROFORMER_OPERATION_RECORDING.md`.
