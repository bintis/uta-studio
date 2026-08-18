# Uta Studio repository rules

These rules are mandatory for AI coding agents and apply to the whole repository.

## Product identity

- The project is named **Uta Studio**. Keep this identity consistent in code, copy, paths, environment variables, CSS, docs, package metadata, and protocols.
- Before handing off a change, run a case-insensitive repository scan for disallowed project-name terms. The result must be empty outside Git metadata and generated dependency/build folders.
- `icon.png` at the repository root is the canonical logo. Generated platform icons must derive from it.

## Runtime and downloads

- Prefer the host or packaged `ffmpeg`, `uv`, and Python through `UTA_STUDIO_FFMPEG_PATH`, `UTA_STUDIO_UV_PATH`, and `UTA_STUDIO_PYTHON_PATH`, with normal `PATH` discovery as the fallback.
- Never download tools, Python packages, or AI models merely because the application launched, a page rendered, or a diagnostic ran.
- Model/runtime installation requires an explicit user action and confirmation in **Settings > Models & runtime**. When unavailable, analysis controls must be disabled and explain where setup lives.
- Existing configured model directories are user data. Do not delete or replace them in tests. Destructive cache operations require an explicit user action.

## Audio and export

- Do not introduce avoidable lossy generations. Lossless source or generated audio is stored/exported as FLAC; lossy audio is stored/exported as MP3.
- Never label lossy bytes as FLAC or a MIME type that does not match the actual payload.
- The editor auditions supported sources through the local in-process command boundary with native GStreamer playback. It plays the chosen source unchanged; unsupported containers may get a cached FLAC/MP3 compatibility preview following the rule above. Waveform visualization may read authorized local media separately while playback is stopped.
- Every export is atomic, validates its target extension, does not silently overwrite user files, and cleans failed temporary output.
- UTZ and UltraStar are both first-class outputs. Changes to chart authoring must be covered by both exporters where relevant.

## API and verification

- Every app-owned feature must have a local in-process command API or be represented by one. Keep `api_capabilities` synchronized with the operations exposed by the native desktop shell.
- Classify each endpoint as `read`, `mutation`, `destructive`, `external`, or `temporary`.
- `run_feature_diagnostics` must remain safe for user data. It may create verified exports only in a uniquely named temporary directory and must remove that directory. It must never run cache deletion, library disconnection, model installation, chart saves, re-analysis, or other destructive/mutating workflows.
- Mutation endpoints are tested through unit/contract tests and isolated fixtures. Do not prove that a destructive API works by using the user's actual library, models, or settings.
- A feature is not complete until its error path is handled in the UI and its relevant automated and smoke tests pass.

## UI and interaction

- The visual direction is Roon-inspired, not a pixel copy: clean information hierarchy, quiet controls, familiar navigation, cover-forward content, softened separators, and restrained translucent surfaces.
- Preserve Uta Studio's own identity through modern transparency, authoring-focused controls, and the canonical logo.
- Avoid bright focus boxes and loud hover fills. Focus, hover, pressed, disabled, and selected states must remain perceptible and accessible through subtle contrast, opacity, type weight, or a restrained indicator.
- Do not add a duplicate Settings control in the top-right. Settings uses the left navigation and the top-left back action; no bottom-right Close button.
- Song selection opens a dedicated route/page, not a permanent right-side detail pane. The chart inspector is optional and closed by default. Lyrics can be hidden so the timeline/spectrum expands.
- Folders support multiple roots, browsing, and authorized context-menu actions. Song and file context menus must expose the relevant edit/open/reveal actions.
- Editor pointer interactions must use pointer capture with global release/cancel cleanup. Manual scrolling temporarily wins over playhead auto-follow.
- The chart viewport must pan independently in time and pitch. Keep note dragging distinct from viewport panning, support mouse/trackpad time and pitch navigation, and allow horizontal and vertical zoom so users can reach pitches beyond the current notes.
- Lyrics without overlapping note guidance must be visibly marked without preventing editing.
- Settings rows keep their description on the left and align selects, switches, sliders, and actions to one consistent right-hand column. Only narrow layouts may wrap the whole control below the description.
- Show model-specific controls only while their owning engine is selected. Keep separator, transcription, alignment, and shared preprocessing concepts distinct. Bounded numeric model controls use minus / editable value / plus and clamp invalid input.
- **Models & runtime** owns installed tools, acceleration, and downloadable artifacts. **Analysis** owns separator, transcription, alignment, pitch, batching, and sensitivity parameters. Song detail may expose the same analysis defaults for convenient tuning, but must state that existing chart data changes only after re-analysis.
- Editor lyric and note jumps use an immediate accurate native-audio seek and preserve the current play/pause intent. Space toggles transport once per key press whenever focus is outside an editable field.
- Treat native audio position as the clock source, but interpolate the visible playhead on animation frames between lightweight status syncs. Timed lyrics with overlapping or very short ranges must use collision-free visual lanes; long lyric controls wrap instead of overlapping adjacent content.
- The Linux desktop is Wayland-only. Do not enable an X11 window backend or use XWayland as a fallback.

## File size

- A single source file (`.rs`, `.py`, or other application code) must stay at or under 2000 lines. Larger files must be split or refactored along existing module boundaries before handoff.

## Safety and completion checks

- Never expose an unauthenticated HTTP control server. Feature APIs stay inside the local process unless the user explicitly requests and approves a different security design.
- Keep user source media read-only. Opening, revealing, scanning, editing cached chart data, and exporting must never move or delete source songs.
- Use the repository's Nix dev shell for Rust/Node tools when they are not on `PATH`. Do not treat Nix environment realization as a request to download models.
- Before final handoff, run Rust formatting/checks/tests, Python compile checks, native UI tests/build, the API registry contract test, real audio decode, real UTZ and UltraStar smoke exports, project-name scan, and a Nix package build.
- Editor audio is not verified merely because a PipeWire stream exists. Audition a real chart continuously, confirm the stream is running/unmuted, inspect PipeWire quantum errors/xruns, and keep waveform/timeline rendering from blocking playback. Do not judge playback while a high-parallelism build is saturating the machine.
- The final packaged artifact must be produced by `nix build path:.#uta-studio` and smoke-launched from its wrapped executable.

See `docs/engineering-constraints.md` for the human-readable rationale and test matrix.
