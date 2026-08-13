# Changelog

## 0.2.0 — 2026-08-14

Native desktop refactor and authoring-workflow restoration.

- Replaced the legacy web/Tauri shell with a pure Wayland Rust/Bevy desktop UI.
- Restored library covers, search, activity and analysis views, song pages,
  settings controls, version information, and contextual file actions.
- Rebuilt the editor with native GStreamer audition, waveform and pitch guides,
  direct lyric editing, multi-selection, resizing, note operations, and safe
  UTZ/UltraStar authoring.
- Added a Roon-inspired library transport with queue, previous/next, shuffle,
  repeat, seeking, volume, and unchanged-source playback.
- Added collision-safe batch export for every authoring-ready chart.
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
