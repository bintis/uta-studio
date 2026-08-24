# Uta! Studio repository rules

These rules are mandatory repository-wide.

## Scope and architecture

- `tasks/remaining-models/STATE.md` is the durable model/task index; `docs/KEY_CONCLUSIONS.md` summarizes accepted conclusions. Do not recreate deleted historical logs or reopen completed work without a current source/test blocker.
- Post-model cards 15–21 and 20A run serially under `tasks/final-features/` and must follow `PROCESS_BOUNDARY_RULES.md` and `STUDIO_BACKEND_UI_PARITY.md`. Cards 15–17 are `READY`, 17A is `SKIPPED_ALREADY_CLOSED`, and 18 is `NEEDS_REVIEW`.
- `docs/design/README.md` and its current linked architecture documents are authoritative over earlier monolithic/refactor assumptions.
- Studio communicates with packaged `uta-analyze` / `uta-runtime` machine protocols only. Never import `uta_analysis_engine::` or `uta_runtime_manager::` into `app-core/**` or `desktop/**`.
- Reserve `docs/agent-tasks/FINAL_REPOSITORY_ACCEPTANCE.md`, whole-workspace checks, and Nix packaging for the later explicit release pass.

## Identity

- Use **Uta! Studio** consistently in code, copy, paths, environment variables, styles, docs, package metadata, and protocols.
- `icon.png` is the canonical logo; derive platform icons from it.
- Before handoff, scan case-insensitively for disallowed project names outside Git metadata and generated dependency/build directories; the result must be empty.

## Runtime

- Prefer packaged native workers and host/packaged `ffmpeg`, using `UTA_STUDIO_NATIVE_ANALYZER_PATH`, component worker variables, and `UTA_STUDIO_FFMPEG_PATH`; use normal executable discovery only where supported.
- Production inference is native-only. CPU is a reference/diagnostic lane, not an automatic production fallback; do not add script-runtime or network-service fallbacks.
- Treat configured model directories as user data: tests must not delete or replace them, and destructive cache operations require explicit user action.

## Audio and export

- Avoid unnecessary lossy generations. Store/export lossless audio as FLAC and lossy audio as MP3; bytes, extension, and MIME must agree.
- Audition supported sources unchanged through the local command boundary with GStreamer on Linux and WASAPI on Windows. Unsupported containers may use cached FLAC/MP3 compatibility previews; waveform reads are allowed only for authorized media while playback is stopped.
- Exports must be atomic, validate extensions, never silently overwrite, clean failed temporary output, and cover both UTZ and UltraStar where chart changes apply.
- User source media is read-only; opening, revealing, scanning, cached-chart editing, and exporting must never move or delete it.

## API and verification

- Remove hash-verification code; hash verification is not required.
- By default, do not add frozen contracts, baselines, or gates. Add one only for a concrete failure scenario where Git, versions, primary keys, transactions, unique constraints, types, and ordinary tests are demonstrably insufficient. Do not remove existing non-hash safety measures merely to simplify code. Put gates only at irreversible, cross-system, security, or formal-release boundaries. Preflight checks must not replace real execution, simulation, or measurement.
- Every app-owned feature needs a local in-process command API or representation. Keep `api_capabilities` synchronized and classify endpoints as `read`, `mutation`, `destructive`, `external`, or `temporary`.
- `run_feature_diagnostics` may create verified exports only in a unique temporary directory that it removes. It must not delete caches, disconnect libraries, install models, save charts, re-analyze, or run other mutations.
- Test mutations/destructive APIs with isolated fixtures, never user libraries, models, or settings. A feature is incomplete until UI errors are handled and relevant automated/smoke tests pass.

## UI and interaction

- Follow a Roon-inspired but distinct Uta! Studio direction: cover-forward hierarchy, quiet controls, softened separators, restrained translucency, and subtle accessible focus/hover/pressed/disabled/selected states.
- Settings lives in left navigation with top-left back: no duplicate top-right Settings or bottom-right Close. Song selection opens a dedicated page; the chart inspector defaults closed, and lyrics may be hidden to expand the timeline/spectrum.
- Support multiple folder roots, browsing, and authorized context menus with relevant edit/open/reveal actions.
- Editor pointer operations require pointer capture plus global release/cancel cleanup. Manual scrolling temporarily defeats auto-follow. Keep note dragging separate from independent time/pitch panning and horizontal/vertical zoom.
- Mark lyrics lacking overlapping note guidance without blocking edits. Use collision-free lanes for overlapping/short timed lyrics and wrap long lyric controls.
- Settings rows keep descriptions left and controls in one right column, wrapping only on narrow layouts. Show controls only for their owning engine; keep separator, transcription, alignment, pitch, batching, sensitivity, and preprocessing concepts distinct. Clamp minus/editable-value/plus numeric controls.
- **Models & runtime** owns installed tools, acceleration, and downloadable artifacts; **Analysis** owns analysis parameters. Song-detail defaults must state that existing chart data changes only after re-analysis.
- Lyric/note jumps seek native audio immediately while preserving play/pause state. Space toggles once per press outside editable fields. Use native audio as the clock and interpolate the visible playhead between lightweight status syncs.
- Linux is Wayland-only; never enable X11 or XWayland fallback.

## Engineering and handoff

- Keep every application source file at or below 2000 lines; split larger files along existing module boundaries.
- Use `bash dev.sh` for Rust/Node/native-library work. Do not routinely use `nix develop path:.`. Use `UTA_STUDIO_NIX_OFFLINE=1 bash dev.sh` only when the shell is already realized.
- Task handoff is not release handoff; preserve `integration_ready` versus `production_ready` distinctions from `STATE.md`.
- Verify editor audio with a real chart and continuous audition, a running/unmuted stream, and PipeWire quantum/xrun inspection. Do not judge playback during a high-parallelism build.

See `docs/engineering-constraints.md` for rationale and the test matrix.
