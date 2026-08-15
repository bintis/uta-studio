# Changelog

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
