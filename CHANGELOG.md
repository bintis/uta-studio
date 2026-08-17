# Changelog

This changelog is generated from git history.

## 0.4.0 — 2026-08-17 (unreleased)

- Workspace crate versions were bumped to `0.4.0` for the desktop/app-core/native-audio/studio-diagnostics/utz-export/xtask packages in this branch.
- Documentation and changelog entries are now being aligned to `0.4.0`; complete release notes are pending official release freeze and tag.

## 0.3.0 — 2026-08-16

- Refined the editor authoring model:
  - Kept the core editor model as an UTZ 0.2 vocal chart.
  - Added a single action-registry command path for editor operations.
  - Named undo steps and added a recent-edit visibility panel.
  - Added contextual chart-problem reporting with precise location metadata.
- Added key chart creation/editing workflows:
  - Roll and retype lyric lines, split lyrics into language-aware syllables.
  - Tap timing directly against playback.
  - Audit note tones through pitch audition.
  - Split/merge/duplicate/quantize note and lyric interactions via richer context actions.
- Expanded multi-track support:
  - Authored multi-track charts.
  - Exported multi-track UltraStar duets.
  - Included analyzer pitch evidence in packaged outputs.
- Improved rhythm/key/beat analysis behavior:
  - Separated music-analysis caching for repeatable, atomic re-runs.
  - Avoided cache invalidation triggered by key/tempo updates.
  - Added beat-grid rendering tied to confident beat metadata.
- Strengthened safety and export metadata:
  - Added Lock mode to prevent accidental note/lyric dragging.
  - Added composer/country/BPM metadata to UTZ export and song model.
  - Fixed a native-seek race that could flash playhead position.
- Release/packaging work:
  - Bumped `lofty` dependency.
  - Kept Windows release workflow on a stable Rust toolchain.

## 0.2.1 — 2026-08-15

- Added the release-note and docs set for 0.2.1.
- Added multi-track charting foundations in editor workflows.
- Improved note/lyric editing and analysis integration:
  - Lyric line retype tool.
  - Lyric syllable splitting.
  - Tap-based timing and pitch audition for notes.
- Added chart problem reporting and more undo visibility.
- Added UltraStar duet exports and analyzer pitch evidence in package outputs.
- Refined MMS/Karaoke analysis configuration and analyzer plumbing.
- Continued desktop decomposition into route modules.

## 0.2.0 — 2026-08-14

- Replaced legacy shell with native Wayland Bevy UI.
- Restored core browsing flow, activity, settings, versioning, and contextual
  media actions.
- Rebuilt editor interaction model with waveform/pitch guides and safe lyric/note
  editing.
- Added native audition and transport controls with queue/play/seek/volume flow.
- Added safer exports with collision-free batch behavior.
- Added optional Japanese MMS Karaoke alignment backend and explicit model setup path.
- Added CI/CD release packaging for desktop artifacts.
- Added initial Roon-inspired visual structure and branding integration.

## 0.1.0 — 2026-08-13

- Initial release cut and first native desktop milestone.
- Introduced local folder-based library browsing.
- Added configurable analysis pipeline and explicit runtime/tool setup.
- Added chart editor with waveform, lyric timing, pitch-note mapping, and chart
  rendering improvements.
- Added first-party UTZ and UltraStar export paths.
- Added native command API and feature diagnostic safety checks.
- Added initial Nix package/build pipeline.
