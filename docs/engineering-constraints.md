# Engineering constraints

This document records Uta! Studio's durable product and engineering decisions. It is the human-readable counterpart to the mandatory AI instructions in [`AGENTS.md`](../AGENTS.md).

## Identity and storage

Runtime directories, environment variables, frontend preload globals, logs, protocols, CSS, documentation, and package metadata use only the Uta! Studio name. The default settings directory is `~/.uta-studio`; generated data may be placed in user-selected cache/model/vendor paths.

Source music is read-only. Removing a watched folder only disconnects it from the index. Cache deletion must describe its scope and require an explicit user action.

## Runtime acquisition

Desktop and Nix builds package `uta-analyze`, `uta-runtime`, native backend workers, and system/packaged `ffmpeg`. Studio receives only `UTA_STUDIO_ANALYSIS_CLI_PATH`, `UTA_STUDIO_RUNTIME_CLI_PATH`, and `UTA_STUDIO_FFMPEG_PATH`; component worker variables remain backend-owned. Production inference has no script runtime or package environment.

Generic production models consume source-verified native artifacts and fail closed when the exact model/backend/runtime combination is not validated. All non-Qwen OpenVINO IR models also expose an explicit CPU-only diagnostic route; CPU is never selected as an automatic fallback. Exact legacy RoFormer GGUF artifacts use a separate GGML/Vulkan worker. All five RoFormer resources—BS-RoFormer, Inst V2, Harmony, Denoise and Dereverb—expose only user-selected GGML/Vulkan `ProductionPinned` routes and must never launch OpenVINO. Every invocation forces batch size 1, no async submission and a serial pipeline. Their durable default root is `<runtime-store>/ggml-models`; `UTA_STUDIO_GGML_MODELS_DIR` may explicitly override it, and `/tmp` is never a production model location. Qwen3-ASR-1.7B and Qwen3 Forced Aligner keep their independent pinned GGML/Vulkan product runtimes defined by `native-inference/runtime-lock.json`. **AI-agent development/acceptance on this host may not execute non-Qwen Vulkan inference without separate explicit user authorization** after the 2026-08-22 black-screen/reboot incident. Vulkan-only paths may be statically audited and may reuse exact prior evidence; any new Vulkan execution requires the exact model-specific validation procedure. Manifest-pinned CPU islands inside a CPU/GPU OpenVINO topology remain required execution rather than fallback.

The app must not download tools, packages, or models on launch. Analysis setup lives in **Settings > Models & runtime**, reports each native component/model separately, and downloads only after explicit confirmation. That page exposes only Runtime Manager-advertised backends per model; explicit choices persist in `model_backend_overrides`, while no entry means the pinned default. Unavailable choices fail in exact Plan Preview without fallback. The JSON `model_backend_note` is human guidance only and records the Intel XPU recommendation for the tested RoFormer GGML serial/no-async route. Analysis controls stay disabled with a direct Settings explanation until setup is ready. Existing analyzed charts remain editable without forcing setup.

## Audio policy

Avoidable lossy generations are forbidden:

| Input/derived audio | Stored or exported form |
| --- | --- |
| Lossless (FLAC, WAV/PCM, AIFF, ALAC, etc.) | FLAC |
| Lossy (MP3, AAC, Opus, Vorbis, etc.) | MP3 |

Editor audition uses native playback through the in-process desktop command boundary and plays the selected source unchanged. Linux uses GStreamer with the Wayland/PipeWire audio session; Windows uses the system WASAPI output through the embedded Rust decoder. Waveform visualization may read authorized local media while playback is stopped. An unsupported container may produce a cached compatibility preview, but it must follow the table and carry the correct MIME type. Exported UTZ/UltraStar assets must be real FLAC or MP3 data, never a renamed file with mismatched bytes.

## Feature APIs and diagnostics

Every app feature is represented by a local in-process command contract. `api_capabilities` is the discoverable manifest and records the area, command name, access class, automation coverage, and description. Contract tests keep command names unique and access classes valid; the Bevy shell calls the same `app-core` operations described by that manifest.

`run_feature_diagnostics` verifies configuration, cache accounting, the SQLite library, navigation facets, folder browsing, runtime status, song loading, chart readiness/loading, audio decoding, and optional real UTZ/UltraStar exports. Export smoke tests use a unique temporary folder and remove it. Diagnostics never clear caches, remove library roots, download models, save charts, or enqueue analysis.

Destructive and external commands still appear in the manifest so API coverage is complete, but they are tested with contracts and isolated fixtures rather than the user's live data.

## Visual and interaction direction

The interface is informed by Roon's calm music-library hierarchy and navigation, not copied at pixel level. Uta! Studio adds translucent surfaces, chart-production controls, and its own logo. The key principles are:

- one-second comprehension: a clear primary action and no competing chrome;
- consistent typography, color, spacing, buttons, menus, and back behavior;
- dedicated song detail and editor pages;
- quiet hover/selected/focus states that remain accessible;
- multiple browsable folders with context menus;
- optional, closed-by-default inspector;
- collapsible lyrics so the timeline/spectrum can fill the editor;
- robust pointer capture and manual-scroll priority;
- independent chart navigation: time and pitch panning, horizontal time zoom, vertical pitch-range zoom, and access to high/low pitches not already occupied by notes;
- visible warnings for lyric words without note guidance;
- sufficient contrast, keyboard navigation, readable type, semantic labels, and comfortably sized targets.
- a consistent right-aligned settings control column; bounded model numbers use a compact minus / editable value / plus control;
- model settings based on actual runtime ownership: primary-engine-only controls may change, while compatibility fallback, alignment, pitch, and shared controls remain visible whenever their paths can still run; separation, transcription, alignment, pitch, and shared preprocessing stay conceptually distinct.
- a strict settings split: **Models & runtime** manages installed runtimes, acceleration, and model files, while **Analysis** owns the parameters that shape generated results. The song production page may offer a compact mirror of those analysis defaults, clearly explaining that they take effect on the next analysis.
- native audio remains the authoritative editor clock. Lyric/note jumps seek accurately without dropping the current play intent, Space toggles transport once outside editable fields, and the visible playhead interpolates on animation frames between low-frequency native status checks.
- collision-free timed lyrics at every window size: coincident or short word ranges occupy separate visual lanes and long words wrap within their own controls rather than covering neighboring text.

## Definition of done — final repository acceptance only

The commands below are **not** per-card acceptance. The full suite runs only during the later explicit release pass reserved by `AGENTS.md`, after focused feature closure converges.

The final repository checks are:

```sh
bash dev.sh -c cargo fmt --all -- --check
bash dev.sh -c cargo check --workspace --all-targets --locked
bash dev.sh -c cargo test --workspace --all-targets --locked
bash dev.sh -c cargo clippy --workspace --all-targets --locked -- -D warnings
bash dev.sh -c cargo xtask docs check
nix build path:.#uta-studio --print-build-logs
```

The development-shell commands above use the small independently locked
`nix/dev-shell` flake rather than the repository root. Do not replace them with
`nix develop path:.` during routine development: the root working tree contains
large mutable build output and is intentionally not the dev-shell flake source.
The pinned shell closure is stable across Git commits and dirty working-tree
changes, so already-realized store paths are reused. `UTA_STUDIO_NIX_OFFLINE=1`
is available only for an already-realized shell; normal bootstrap should retain
binary substitutes rather than forcing source builds.

The repository must contain no tracked script-runtime source files, and a packaged analysis run must have no script-runtime process in its process tree. Native worker stdout is protocol-only NDJSON; cancellation, timeout, crash cleanup, runtime-lock identity, and fail-closed routing are release gates.

Without separate current user authorization, GPU inference tests during agent and final-acceptance work are OpenVINO-only: do not run Vulkan smoke, benchmark, stress, full-track, or intentional Vulkan-context commands. Historical RoFormer validation records document machine-level failures and make clear that even passing configurations are graph-specific; there is no general safe Vulkan mode. The explicitly authorized 2026-08-24 scope covered fresh isolated serial/no-async full-song runs for the five exact legacy RoFormer GGUFs, plus two earlier 12 s BS checks. Those clean results do not authorize benchmarks, repeat/stress sequences, concurrent runs, another checkpoint, or another graph; each requires new explicit scope.

In addition, use an analyzed fixture to decode editor audio with ffmpeg and perform real UTZ and UltraStar exports. Validate the UTZ ZIP/manifest/hash metadata, parse the UltraStar chart, decode both exported audio assets, confirm temporary cleanup, and smoke-launch the wrapped Nix executable.

For editor playback, use a real chart for a sustained audition. Confirm the PipeWire stream is running and unmuted, and inspect `pw-top` for quantum errors/xruns during playback. A stream that exists but stutters is a failure. Run this check without a concurrent high-parallelism Rust/Nix build.
