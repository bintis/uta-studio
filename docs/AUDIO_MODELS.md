# Native audio model catalog

Uta! Studio keeps model identity, Rust graph implementation, and the upstream GGML runtime recipe separate.

## Current catalog

Runtime Manager exposes seventeen model resources. Every one of them runs on the same Rust/upstream-GGML boundary:

| Model | Capability | Product role |
| --- | --- | --- |
| `bs_roformer_leap_xe90_vocals` | `audio.extract_vocals`, `audio.extract_instrumental` | Default one-invocation vocal/instrumental separation |
| `bs_polarformer_public_instrumental` | `audio.extract_vocals`, `audio.extract_instrumental` | Explicit experimental separation strategy |
| `melband_roformer_harmony` | `audio.lead_isolate` | Lead vocal plus vocal residual |
| `melband_roformer_denoise_aufr33` | `audio.denoise` | Optional cleanup |
| `melband_roformer_dereverb_anvuew` | `audio.dereverb` | Optional cleanup |
| `rmvpe` | `pitch.track` | Primary continuous F0 evidence |
| `fcpe` | `pitch.secondary`, `pitch.secondary.fcpe` | Maximum-mode secondary continuous F0 evidence |
| `basic_pitch` | `notes.basic_pitch` | Optional onset/activation note challenger |
| `game_1_0_3_small` | `notes.game` | Selectable GAME size |
| `game_1_0_3_medium` | `notes.game` | Default note and boundary evidence |
| `game_1_0_3_large` | `notes.game` | Selectable GAME size |
| `jbm555_cectc_80` | `notes.jbm555` | Japanese mix-and-vocal conditioned note evidence |
| `stars` | `notes.stars`, `technique.analyze` | Transcript-conditioned note, technique, and style evidence |
| `rosvot` | `notes.rosvot` | Transcript-conditioned note evidence |
| `firered_asr2_aed` | `speech.transcribe.challenger` | Optional transcript challenger |
| `qwen3_asr_1_7b` | `speech.transcribe` | Primary singing transcription |
| `qwen3_forced_aligner_0_6b` | `speech.align` | Word-level forced alignment |

There is one runtime resource, `ggml_vulkan`, and it lists every model above as supported.

`stars` and `rosvot` additionally depend on `rmvpe` because they are conditioned on tracked F0. `firered_asr2_aed` declares a complete named artifact set — `firered-f32.gguf` as `model`, `cmvn.ark` as `cmvn`, and `dict.txt` as `tokens` — so its sidecars are catalog-pinned peers rather than paths the worker guesses.

A catalog entry is not proof of production readiness. Every model revision and runtime recipe retains separate integration, numerical, performance, and perceptual evidence. Current readiness is recorded in `tasks/remaining-models/STATE.md`.

## Runtime boundary

`native-inference/ggml-runtime` implements GGUF loading, audio frontend/postprocessing, and every model graph — RoFormer, RMVPE, FCPE, Basic Pitch, GAME, JBM555, STARS, ROSVOT, FireRed, and Qwen — in Rust. It calls only the C ABI of shared libraries built from upstream `ggml-org/ggml` revision `8c63e70982c95ceb862e3a1073a2c1beef75d60a`.

The package contains:

- `libggml.so.0`
- `libggml-base.so.0`
- `libggml-cpu.so`
- `libggml-vulkan.so`
- `runtime-manifest.json`

It does not contain an app-owned C/C++ model graph, C shim, model CLI, or inference subprocess, and no model conversion, model rewrite, or model execution script in any scripting language. It is upstream GGML plus exactly the patches `native-inference/ggml-worker/runtime-recipe.json` declares, which today is one Vulkan backend fix and no model code; the recipe digest covers the patch set, so an undeclared build stops validating. The GGML CPU backend is an explicitly selected experimental reference lane. Vulkan device selection fails closed and never falls back to CPU.

`native-inference/gpu-probes` performs read-only Vulkan enumeration for diagnostics and device matching; it does not create a device or run inference.

FFmpeg may be launched for audio decode/encode. This is an audio codec boundary, not a model inference route.

## Separation behavior

Leap XE90 is the default strategy. One model invocation publishes both outputs:

1. `guide_vocals`
2. `instrumental`, computed as the mixture residual

This avoids a second independent instrumental model pass and preserves a reconstruction relationship between outputs. PolarFormer remains selectable only as an explicit experiment.

Lossless outputs are FLAC. Output bytes, extension, MIME, channel count, sample rate, and canonical timeline are validated before publication.

## RMVPE behavior

The Rust RMVPE implementation owns:

- 16 kHz audio frontend
- 1024-point FFT, 160-sample hop, 128-bin HTK mel transform
- reflection padding
- U-Net/CNN graph
- bidirectional GRU
- output head and local weighted F0 decoding
- bounded windows with overlap and canonical 10 ms output frames

A real AMD 780M run completed. The remaining historical-reference difference is documented in `tasks/remaining-models/STATE.md`; smoke success is not strict parity.

## FCPE behavior

FCPE is a separate optional evidence node rather than an RMVPE fallback. The default workflow runs it only in Maximum mode. Rust owns its 32,000-sample windowing, exact Slaney-mel frontend, GGML graph, and centroid decoder. Any CPU or Vulkan execution failure is reported under the explicitly selected device route; FCPE does not trigger a device fallback.

The isolated 6-second CPU and AMD 780M Vulkan checks each produced 601 frames and matched every voiced/unvoiced decision from the fresh OpenVINO reference. The bounded F0 differences are recorded in `tasks/remaining-models/STATE.md`; broader songs, layerwise vectors, and performance evidence remain before production qualification.

## Transcription and alignment behavior

Qwen3-ASR 1.7B owns `speech.transcribe`; Rust owns its mel frontend, tokenizer, sampling, and decoder. Qwen3 Forced Aligner 0.6B owns `speech.align` with a Rust-owned timestamp decoder, and keeps an identity separate from ordinary ASR.

FireRedASR2-AED is an optional challenger on `speech.transcribe.challenger`. It never replaces the primary transcript and is never baseline-required: it is scheduled only when the request language makes it applicable, its output must carry the `firered_asr2_aed` expert identity, and a failure degrades the run with a recorded reason and removes its partial output rather than failing the analysis or substituting another provider.

Caller-provided canonical lyrics still bypass generated transcription entirely.

## Note and technique experts

GAME 1.0.3 medium is the default note provider; small and large are interchangeable choices on the note/boundary card in **Processing Studio**. Only that card exposes variants, so an independent Basic Pitch, JBM555, STARS, or ROSVOT expert cannot be silently repurposed into a duplicate GAME execution. Basic Pitch, JBM555, STARS, and ROSVOT are optional challengers that contribute evidence to fusion rather than replacing GAME. STARS also owns `technique.analyze`. STARS and ROSVOT are conditioned experts: each run consumes the word-level timed transcript produced by forced alignment plus the shared RMVPE pitch evidence, so the plan carries them only when those inputs are part of the run.

## Retired models

Inst V2 is permanently retired: it has no catalog entry, graph, worker route, or fallback. Historical artifacts and measurements for deleted C++/CLI/WGPU/OpenVINO backends do not qualify the current runtime.

## Container migration

STARS, ROSVOT and FireRed have historical GGUF containers that record native PyTorch dimension order,
and the two conformer models also carry tensor names at or above GGML's 64-character limit. Upstream
GGML refuses to open them. `cargo xtask gguf <stars|rosvot|firered> SOURCE OUTPUT` rewrites the
container in place of the retired conversion scripts: tensor payload bytes and offsets are copied
verbatim, dimensions are reversed into GGML's fastest-varying-first order, and structural path
components are abbreviated where required. The output is byte-identical to what the historical
scripts produced.

The command refuses to overwrite an existing file and publishes atomically, so it can never damage
an installed model. Installing the result is a separate, explicit user action.

## Installation and user data

Models and runtime components are installed only after confirmation in **Settings > Models & runtime**. Startup, page rendering, status checks, diagnostics, and workflow compilation are read-only and never download artifacts.

Configured model directories are user data. Tests use isolated roots and must not delete or replace user models. Workflow nodes store catalog IDs rather than arbitrary checkpoint paths. Digests remain provenance/cache metadata; they are not software-license or hash-verification gates.
