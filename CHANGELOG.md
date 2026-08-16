# Changelog

## 0.3.0 — 2026-08-16

This release focuses on richer musical analysis, an authoring safety net for locking in timing, and more complete song metadata.

- Added a rhythm analyzer alongside key detection: BPM, tempo confidence, and absolute-second beat timestamps, backed by Essentia when it's installed (no Windows wheel) and a dependency-free spectral-flux/autocorrelation estimator otherwise. Key detection is now structured (`tonic`/`scale`/`confidence`) and never silently defaults to C major on failure or silence.
- Key/rhythm/descriptor analysis now caches independently to its own `{file_hash}_music_analysis.json`, written atomically. Realigning lyrics no longer repeats BPM/key detection, and re-running that analysis alone no longer repeats stem separation or transcription.
- Decoupled the stem cache's identity from the detected key and tempo — an analyzer update no longer forces existing libraries to re-run stem separation; pre-update caches are still recognized and reused, never deleted.
- Added an optional, viewport-culled beat grid to the editor timeline, toggleable from the dock toolbar, shown only when the song has confident beat data.
- The chart's analyzer pitch evidence is now a single static background reference spanning the full timeline instead of a per-note overlay.
- Added right-click context menus for notes, lyrics, and the waveform (pitch/vocal audition, split/merge/duplicate/quantize, bind/unbind, waveform source and style), a keyboard shortcut cheat sheet, a chart-checks panel with precise problem locations, and a whole-song lyrics editor.
- Note context menu gained "Play vocal" alongside "Play pitch," auditioning the isolated vocal stem for the clicked note's range instead of a synthesized tone.
- "Bind" (merging an unpitched lyric onto a lyric-less pitch note) now offers a choice of which side's timing wins — the MIDI note's or the lyric's — instead of always keeping the pitch note's.
- Added a Lock mode that blocks accidental note/lyric dragging while leaving keyboard nudging, selection, panning, and zoom untouched.
- Added a shared song settings panel (composer, country, BPM override, background video, and analysis descriptors), opened from both the library detail page and the editor.
- UTZ export now writes BPM and key metadata (previously never populated) plus the new composer/country fields; extended the UTZ format itself with `composer`/`country` on song metadata.
- Fixed a native audio seek race where clicking the timeline could make the playhead jump to the clicked position and then flicker back to the old one before the pipeline settled.

## 0.2.1 — 2026-08-15

This release focuses on direct UTZ 0.2 authoring and editor workflow maturity.

- Switched the editor model to write directly against the UTZ 0.2 vocal chart.
- Refactored editor command handling behind a single action registry and improved undo visibility by naming each edit step and showing recent actions.
- Added lyric and note authoring affordances for line retype, playback-tap timing, tone audition, language-aware syllabization, and chart problem reporting with precise locations.
- Added multi-track authoring and UltraStar duet export for vocal tracks.
- Export now includes analyzer pitch evidence in packaged outputs.
- Refined analysis configuration for MMS/Karaoke alignment and updated analyzer script plumbing.
- Continued desktop restructuring into route modules; added Windows release toolchain pinning in the release workflow.

## 0.2.0 — 2026-08-14

Native desktop refactor and authoring-workflow restoration.

- Replaced the legacy web/Tauri shell with a pure Wayland Rust/Bevy desktop UI.
- changelog: Bevy/Tauri UI refresh
- Restored library covers, search, activity and analysis views, song pages,
  settings controls, version information, and contextual file actions.
- Rebuilt the editor with native GStreamer audition, waveform and pitch guides,
  direct lyric editing, multi-selection, resizing, note operations, and safe
  UTZ/UltraStar authoring.
- Added a Roon-inspired library transport with queue, previous/next, shuffle,
  repeat, seeking, volume, and unchanged-source playback.
- Added collision-safe batch export for every authoring-ready chart.
- Added an optional Japanese MMS Karaoke forced-alignment backend with
  FA-Kara-style pronunciation mapping, silence-aware timing and explicit model
  installation/licensing confirmation.
- Added a GitHub Actions release workflow that publishes a self-contained
  x86_64 Linux binary, DEB and RPM packages, plus a Windows x86_64 ZIP.
- The Windows build provides the native Bevy authoring UI, but editor audio
  audition remains Linux-only in 0.2.0.
- Improved narrow-window wrapping, card/side-navigation clipping, title-bar
  integration, canonical branding, and light/dark visual hierarchy.

## 0.1.0 — 2026-08-13

Initial Uta Studio release.

- Local music-library browsing with multiple folders and contextual actions.
- Configurable analysis pipeline with explicit runtime and model setup.
- Dedicated chart editor with native audio audition, waveform, lyric timing,
  pitch-note authoring, collision-free lyric lanes, and smooth playhead motion.
- Atomic UTZ and UltraStar export with configurable output locations.
- Native Bevy command API catalogue and safe feature diagnostics.
- Nix package for the Linux desktop application.
