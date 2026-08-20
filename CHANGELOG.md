# Changelog

This changelog is generated from git history.

## 0.5.1 — 2026-08-20

- Refined library navigation by grouping charts and video under My Library and removing artist/album sidebar entries.
- Restored entity-bound song and DAG pointer interactions, including reliable node context-menu dismissal.
- Moved DAG context menus to a lightweight overlay so opening or closing one does not rebuild the graph.
- Removed per-frame UI API registry reconstruction and normal-use whole-tree rebuild instrumentation that caused interaction lag.

## 0.5.0 — 2026-08-18

### Audio model catalog

- Added a fixed, offline UVR/Demucs audio Model Catalog with stable model IDs, full SHA-256 integrity, and recorded licenses.
- Added typed public/architecture/device parameters with units. Bare `overlap` is no longer a shared setting.
- Frozen an immutable audio-processing plan when analysis is queued so later Settings changes cannot rewrite an in-flight job.
- Added semantic runner contracts for MDXC/RoFormer, Demucs, and MDX ONNX. Outputs bind by metadata, not `"(Vocals)"` filenames.
- Added whole-model XPU/OpenVINO-to-CPU fallback with requested/actual backend telemetry. Intermediate audio stays lossless WAV/float.
- Added Settings controls to choose vocal, accompaniment, karaoke, independent denoise/dereverb (with order), and compute backends, plus per-model install/remove in Models & runtime.
- Routed the existing default karaoke RoFormer through an in-repo offline adapter around [audio-separator](https://github.com/nomadkaraoke/python-audio-separator) 0.44.5. Production separation no longer matches `"(Vocals)"` filenames.
- Catalogued the production karaoke checkpoint as `melband_roformer_karaoke_aufr33_viperx` and import existing `models/audio_separator` weights instead of downloading them again.
- Pinned `audio-separator==0.44.5` and bumped the managed runtime marker to `runtime-v5`. Older `runtime-v4` markers still discover the user data directory but require an explicit rebuild.
- Wired stem DAG children (`stems.vocals`, denoise, dereverb, accompaniment, karaoke, multistem, bind) and recorded requested/actual backend on each step.
- Kept legacy `karaoke` / `demucs` / `openvino_demucs` configuration and stem cache readable.
- Clarified Models & runtime status: a usable older runtime stays ready to analyze; an outdated contract is shown as optional rebuild, not as a missing component.

### Analysis page

- Dedicated the Analysis page to the live DAG: the song title sits in the header, progress is a hairline on the top-bar boundary, and Inspect opens as its own page from the node context menu.
- Default DAG zoom now fits the window and centers the graph. During a run the current node stays in view. Source labels on edges appear only while Source is on.
- Node context-menu items are left-aligned. The inspect page can be scrolled with the mouse wheel.
- Models & runtime no longer rebuilds the settings page on every setup log line, so scrolling stays usable during an upgrade.

### Documentation and artifacts

- Added an embedded, generated three-language Documentation Center with native Markdown rendering, search, history, semantic links, and responsive navigation.
- Documented the Documentation Center and Artifact Workbench in the English, Simplified Chinese, and Japanese user guides.
- Added content-addressed immutable Artifact revisions and migration of legacy cache inventory without removing compatibility files.
- Added structured analyzer output-commit events, exact attempt/input/output bindings, explicit produced/reused/frozen/bypassed states, and transactional revision/relation persistence.
- Expanded the Artifact Workbench with run-specific resolution, functional inspector tabs, health validation, provenance, pinning, impact previews, and bounded semantic revision diff panels.
- Added revision-specific LyricsInput and lossless TimedTranscript drafts with validation, concurrency detection, explicit Active policy, Save Only, and confirmed downstream execution.
- Added a bounded segment/word timing surface with native-audio jumps and fine boundary adjustment while retaining the complete lossless JSON working copy.
- Made CandidateChart a distinct validated UTZ chart Artifact and added exact Candidate/Authored comparison, revision-specific editor loading, and phrase/range/lyrics-timing/pitch merge primitives.
- Open PitchTrack and PitchNoteCandidates from the selected immutable revision, merge a candidate into the current editor phrase or note selection, and confirm Replace/Keep Authored with Pin refusal.
- Kept source media read-only and added guarded deletion for Active, pinned, or historically consumed revisions.
- Made chart editor saves immutable revisions before atomically updating the Active compatibility file.
- Added explicit one-shot or persistent capture of real preprocessed audio as lossless FLAC; ordinary runs retain no additional intermediate audio.
- Made analysis-graph edges first-class: click selects the selected-run Artifact binding, and edge color shows produced, reused, frozen, bypassed, missing, or invalidated.
- Wired export-graph nodes to validate, re-export, and reveal the last recorded destination without treating packages as Artifact revisions.
- Highlighted lineage on the main analysis DAG, including MINI compute-only view, and built impact groups from one frozen analysis plan that confirmation queues unchanged.

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
