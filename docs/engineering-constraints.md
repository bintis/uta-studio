# Engineering constraints

This document records Uta Studio's durable product and engineering decisions. It is the human-readable counterpart to the mandatory AI instructions in [`AGENTS.md`](../AGENTS.md).

## Identity and storage

Runtime directories, environment variables, frontend preload globals, logs, protocols, CSS, documentation, and package metadata use only the Uta Studio name. The default settings directory is `~/.uta-studio`; generated data may be placed in user-selected cache/model/vendor paths.

Source music is read-only. Removing a watched folder only disconnects it from the index. Cache deletion must describe its scope and require an explicit user action.

## Runtime acquisition

Desktop and Nix builds use system/packaged `ffmpeg`, `uv`, and Python. The preferred explicit variables are:

- `UTA_STUDIO_FFMPEG_PATH`
- `UTA_STUDIO_UV_PATH`
- `UTA_STUDIO_PYTHON_PATH`

The app may discover those tools on `PATH`. It must not start downloading tools, packages, or models on launch. Analysis setup lives in **Settings > Models & runtime**, shows what is missing, lets the user choose the compute backend and model family/size, and downloads only after explicit confirmation. Every selected model family has its own status and download/reinstall action; this includes the RMVPE frequency-analysis model even while it is the only pitch option. The shared runtime remains a separate setup action. Analysis controls stay disabled with a direct Settings explanation until setup is ready. Existing analyzed charts remain editable without forcing setup.

## Audio policy

Avoidable lossy generations are forbidden:

| Input/derived audio | Stored or exported form |
| --- | --- |
| Lossless (FLAC, WAV/PCM, AIFF, ALAC, etc.) | FLAC |
| Lossy (MP3, AAC, Opus, Vorbis, etc.) | MP3 |

Editor audition uses native GStreamer playback controlled through binary Tauri IPC and plays the selected source unchanged. Waveform visualization may read authorized local media while playback is stopped. An unsupported container may produce a cached compatibility preview, but it must follow the table and carry the correct MIME type. Exported UTZ/UltraStar assets must be real FLAC or MP3 data, never a renamed file with mismatched bytes.

## Feature APIs and diagnostics

Every app feature has a local Tauri IPC command. `api_capabilities` is the discoverable manifest and records the area, command name, access class, automation coverage, and description. A test keeps it exactly synchronized with `tauri::generate_handler!`.

`run_feature_diagnostics` verifies configuration, cache accounting, the SQLite library, navigation facets, folder browsing, runtime status, song loading, chart readiness/loading, audio decoding, and optional real UTZ/UltraStar exports. Export smoke tests use a unique temporary folder and remove it. Diagnostics never clear caches, remove library roots, download models, save charts, or enqueue analysis.

Destructive and external commands still appear in the manifest so API coverage is complete, but they are tested with contracts and isolated fixtures rather than the user's live data.

## Visual and interaction direction

The interface is informed by Roon's calm music-library hierarchy and navigation, not copied at pixel level. Uta Studio adds translucent surfaces, chart-production controls, and its own logo. The key principles are:

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

## Definition of done

The expected checks are:

```sh
nix develop path:. -c cargo fmt --all -- --check
nix develop path:. -c cargo check --workspace --all-targets
nix develop path:. -c cargo test --workspace --all-targets
python3 -m py_compile app-core/analyzer/*.py
nix develop path:. -c pnpm --dir client run lint
nix develop path:. -c pnpm --dir client run test:editor
nix develop path:. -c pnpm --dir client run build
nix build path:.#uta-studio --print-build-logs
```

In addition, use an analyzed fixture to decode editor audio with ffmpeg and perform real UTZ and UltraStar exports. Validate the UTZ ZIP/manifest/hash metadata, parse the UltraStar chart, decode both exported audio assets, confirm temporary cleanup, and smoke-launch the wrapped Nix executable.

For editor playback, use a real chart for a sustained audition. Confirm the PipeWire stream is running and unmuted, and inspect `pw-top` for quantum errors/xruns during playback. A stream that exists but stutters is a failure. Run this check without a concurrent high-parallelism Rust/Nix build and without test-only WebKit compositing overrides.
