# Uta Studio — Documentation Center & Artifact Workbench Design

**Target baseline:** Uta Studio 0.5.x working tree after the Documentation Center / Artifact Workbench import  
**Design revision:** 2026-08-18  
**Status:** partial production implementation — not release-complete  
**Handoff status:** `docs/UTA_STUDIO_REMAINING_DEVELOPMENT_AGENT_GUIDE.md` §0.1 is the source of truth for phase completion

## 1. Goals

This change turns two previously separate ideas into one explainable workflow surface:

1. **Documentation Center** — an offline, native Bevy route for the English, Simplified Chinese, and Japanese user guide, with table of contents, search, deep links, F1 context help, and runtime-locale selection.
2. **Artifact Workbench** — a typed Node I/O inspector for the analysis DAG. It distinguishes declared inputs/outputs from concrete run-time bindings, exposes revisions and provenance, supports lineage and impact analysis, and routes compatible artifacts into existing lyrics/chart editors without mutating historical analyzer output.

The design deliberately reuses the existing `AnalysisGraphSpec`, `AnalysisPlan`, `analysis_node_attempts`,
`ArtifactRevision`, `NativeLyricsEditor`, `NativeEditor`, app-core API catalogue, cache-root boundary checks,
and current i18n mechanism.

## 2. Non-negotiable safety rules

- Source media remains read-only.
- Analyzer-produced revisions are immutable from UI semantics. Editing creates/updates authoring/user inputs, never historical model output in place.
- Authored charts are preserved unless the user explicitly confirms replacement.
- New UI actions identify artifacts by `ArtifactRef { file_hash, kind, revision_id }`; paths are re-resolved from the revision inventory.
- Pinned revisions cannot be deleted.
- Missing/legacy lineage is shown as `LegacyUntracked`; it is never fabricated from a filename.
- Ephemeral artifacts are shown as ephemeral instead of "missing".
- Documentation text bypasses runtime source-string localization; the correct language document is selected before rendering.
- All new app-owned commands are included in `API_CAPABILITIES`.

## 3. Documentation Center

### 3.1 Navigation

`StudioRoute::Documentation` is a first-class route.

Entrypoints:
- Settings → General → Open user guide
- Settings navigation → Documentation
- `F1` opens context help
- DAG node context menu → Open node documentation
- Artifact inspector → About this artifact

Deep links are stable semantic IDs:
- `guide:getting-started`
- `guide:analysis`
- `guide:editor`
- `node:lyrics.align`
- `artifact:TimedTranscript`

### 3.2 Source and build

Canonical sources are the single-language files:

- `docs/user-guide/en.md`
- `docs/user-guide/zh-CN.md`
- `docs/user-guide/ja.md`

`cargo xtask docs build` generates the GitHub-facing combined guide
`docs/USER_GUIDE.md` and the embedded bundle `desktop/assets/docs/docs.bundle.json`.
`cargo xtask docs check` rejects locale drift, duplicate anchors, broken internal
links, missing pages, and stale hard-coded release names.

The desktop application embeds the generated bundle. Packaged builds do not need
runtime Markdown files or network access.

User-facing chapters for this feature live at `guide:documentation` and
`guide:artifacts`. Node help still opens the matching workflow chapter
(`guide:analysis`, `guide:lyrics`, `guide:editor`). Artifact help
(`artifact:{Kind}`) opens `guide:artifacts`.

### 3.3 Rendering

The viewer parses a controlled Markdown AST and renders native Bevy text:

- H1–H4
- paragraphs
- ordered/unordered lists
- fenced code blocks
- tables and callouts when present
- inline code and ordinary text
- internal semantic anchors

It never executes HTML, JavaScript, remote images, `file://` links, or iframes.
Wide layouts use contents, article, and search/history columns. Narrow layouts
stack those columns. Search is exact-block substring matching, including CJK.
History has real back/forward stacks. F1 from a dirty editor asks before leaving.

### 3.4 Search

Search is offline and page-local:
- Unicode lowercase substring match
- headings receive a higher score
- CJK works without tokenization because substring matching is character based
- results jump to semantic anchors

### 3.5 Localization boundary

`NoRuntimeLocalization` excludes document body text from `localize_ui_text`.
Viewer chrome remains localized by the existing UI catalogue.

## 4. Artifact Workbench domain model

### 4.1 Stable artifact reference

```rust
ArtifactRef {
    file_hash,
    kind,
    revision_id,
}
```

No new editor/open/preview action carries an untrusted user-provided path.

### 4.2 Node I/O inspection

`NodeIoInspection` exposes both:
- `expected_inputs` / `expected_outputs` from `AnalysisNodeSpec`
- `resolved_inputs` / `resolved_outputs` from per-run bindings or revision inventory

Binding states:
- `Resolved`
- `Source`
- `Ephemeral`
- `FrozenReuse`
- `Bypassed`
- `Missing`
- `LegacyUntracked`
- `Invalidated`
- `NotApplicable`

If exact attempt→artifact rows do not exist for an older run, the inspector may show the current active/newest
revision only as `LegacyUntracked`, with an explicit explanation.

### 4.3 Attempt-to-artifact relation

Schema v10 is current. v8 added `analysis_node_artifacts`; v9 binds every new
relation row to a concrete `attempt_id`; v10 stores explicit intermediate-capture
requests.

Relation fields:

- `run_id`
- `attempt_id`
- `node_id`
- `direction`
- `slot`
- `artifact_kind`
- `revision_id`
- `binding_kind`

New runs write attempt-specific Produced / Reused / Frozen / Bypassed / Source /
Ephemeral / Missing / NotApplicable relations at the execution boundary. The
store is content-addressed. Immutable storage and DB relation writes are
transactional. Legacy history without relation rows remains readable and is
shown as `LegacyUntracked`.

### 4.4 Pinning

Schema v7→v8 adds `analysis_artifacts.pinned INTEGER NOT NULL DEFAULT 0`.

Pin is independent of Active and Freeze:
- Active = current selected revision
- Pin = protected from deletion/cleanup
- Freeze = one run deliberately reuses an existing output

`delete_artifact_revision` refuses pinned revisions.

## 5. Typed artifact capabilities

Capabilities are derived from artifact kind and state:
- preview text/JSON/audio/metadata
- open lyrics editor
- open chart editor
- compare
- set active
- reveal
- pin
- invalidate
- delete

The UI does not show actions guaranteed to fail.

## 6. Typed preview and validation

Artifact health:
- `Valid`
- `Warning`
- `Invalid`
- `NotChecked`

Checks include:
- file existence/size
- JSON parse validity
- transcript segment/timing shape
- pitch MIDI/confidence ranges when recognizable
- authored chart parse path delegated to the existing editor boundary

Typed diff:
- byte-identical fast path by content hash
- recursive JSON paths, pitch-note movement/transposition, and chart structure summaries
- text line added/removed counts
- binary/audio metadata fallback

## 7. Editing semantics

### LyricsInput / LRC
`Open in compatible editor` routes to the existing Lyrics Editor.
Saving follows existing lyrics safety behavior; analyzer revisions remain untouched.

### RecognizedText
Shown as analyzer evidence. `Use as lyrics draft` is represented as editor routing, not in-place mutation.

### TimedTranscript
Preview is available immediately. The compatible editor shows bounded segment and word/token rows, native-audio jumps, and fine start/end adjustment alongside the complete structured JSON working copy. Validation rejects overlap, non-monotonic, non-finite, negative, and out-of-segment timings before save while unknown extension fields round-trip untouched.
Historical transcript revision bytes are never overwritten by the workbench.

### PitchTrack / PitchNoteCandidates
Remain evidence. They can route to the chart editor for contextual editing but the evidence file itself is read-only.

### AuthoredChart / CandidateChart
Both kinds open the selected immutable revision, not a current-file substitute. CandidateChart is materialized as a distinct validated UTZ vocal chart at the Rust schema boundary. The context workflow supports semantic comparison and validated in-memory merge modes for full replacement, phrase/range replacement, candidate lyric timing, and candidate pitch. Saving creates a new AuthoredChart revision with the exact source revision in provenance; Candidate/analysis evidence never silently replaces the authored chart.

## 8. Lineage mode

`artifact_lineage` walks `input_revisions` recursively and returns revision nodes plus producer relationships.

A dedicated Lineage panel supports upstream-only, downstream-only, and full
scope. Revisions are selectable. Missing legacy links appear as explicit gaps,
not invented edges. Downstream consumers are listed.

The main analysis DAG emphasizes lineage edges/nodes and de-emphasizes
unrelated graph content when Lineage is on. MINI view keeps compute nodes
only and still highlights producer/consumer compute nodes. Missing legacy
links remain explicit gaps.

## 9. Impact preview

`preview_frozen_downstream_impact` builds one `AnalysisRequest` from the song
profile, staged Freeze / Bypass / Disable intents, and the mutation trigger,
then classifies groups from that frozen `AnalysisPlan`. Confirmation calls
`run_analysis_request` with the same request. Authored-chart preservation is
explicit except for Candidate Replace. The preview itself does not mutate.

## 10. Intermediate capture

`CaptureIntermediateRequest` persists a one-shot or recurring, per-song opt-in.
`PreprocessedAudio` remains ephemeral by default. The confirmation discloses estimated storage and vocal
privacy implications. The request is frozen when a job joins the queue; the Python boundary atomically
materializes the actual processed float-audio array as FLAC, emits a structured commit, and Rust copies it
to the immutable store before removing the compatibility temporary. A successful one-shot capture clears
its request; failed capture leaves it armed for a later run.

## 11. Documentation + DAG bridge

Node help links:
- `node:preflight`
- `node:music.analysis`
- `node:stems.separate`
- `node:pitch.extract`
- `node:lyrics.preprocess`
- `node:lyrics.transcribe`
- `node:lyrics.align`
- `node:lyrics.import_timed`
- `node:chart.build_candidate`

Artifact help links use `artifact:{DebugName}`.

## 12. API additions

Read:
- `inspect_analysis_node_io`
- `inspect_artifact`
- `preview_artifact`
- `artifact_lineage`
- `preview_artifact_downstream_impact`
- `preview_node_downstream_impact`
- `preview_artifact_edit_impact`
- `preview_frozen_downstream_impact`
- `resolve_graph_edge_binding`
- `inspect_export_node`
- `validate_export_node`
- `compare_artifacts_typed`
- `resolve_artifact_for_run`
- `begin_artifact_edit`
- `merge_chart_revisions`

Mutation:
- `set_artifact_pinned`
- `capture_analysis_run_artifacts`
- `commit_artifact_edit`
- `run_analysis_request`
- `record_last_export`

Navigation remains a desktop-shell concern. Downstream execution is queued only
after an explicit Save and Run Downstream confirmation.

## 13. UI layout

DAG inspector tabs:
- Overview
- Inputs
- Outputs
- Attempts
- Logs
- Help

Artifact rows:
- Preview / Play
- Open in compatible editor (when supported)
- Pin / Unpin
- Set active
- Compare
- Provenance
- Reveal
- Invalidate
- Delete
- Help

## 14. Migration / backward compatibility

- Existing schema v6 upgrades additively to v10; v8 adds pin + node/artifact relations, v9 adds exact
  attempt IDs, and v10 adds explicit intermediate-capture requests.
- Existing artifact rows get `pinned = 0`.
- Existing runs have no `analysis_node_artifacts` rows and therefore display `LegacyUntracked`.
- Existing UI actions remain valid.
- Existing i18n catalog continues to be the fallback for viewer chrome.
- `docs/USER_GUIDE.md` remains valid for GitHub readers.

## 15. Verification contract

Required local/CI checks after import:

```sh
cargo xtask docs check
cargo fmt --check
cargo test -p uta-studio-core artifact_workbench
cargo test -p uta-studio-core library_db
cargo test -p uta-studio-desktop documentation
cargo check --workspace
nix build path:.#uta-studio
```

Manual:
- F1 opens documentation.
- Settings opens user guide.
- EN/ZH/JA document selection follows interface locale.
- Search finds English/CJK headings.
- DAG node context opens help.
- Inputs/Outputs distinguishes expected/resolved.
- Old run reports LegacyUntracked instead of invented lineage.
- Pin prevents delete.
- ArtifactRef path resolution rejects cache-root escape.
- Authored chart survives reanalysis/edit routing.

## 16. Implementation status — 2026-08-18

This section records the working tree, not the original import package.

| Item | Current state |
|---|---|
| Schema | v10 |
| Canonical docs | `docs/user-guide/{en,zh-CN,ja}.md` |
| Generated docs | `docs/USER_GUIDE.md`, `desktop/assets/docs/docs.bundle.json` |
| User-facing chapters | `guide:documentation`, `guide:artifacts` |
| Automated tests | `uta-studio-core` and `uta-studio-desktop` suites were last recorded green in the remaining-work guide |

**Complete enough for production paths:** documentation source/build, Markdown AST viewer, immutable revision store, exact commit protocol, inspector tabs, edit drafts, Lyrics/TimedTranscript editors, PitchTrack/PitchNoteCandidates selected-revision editor evidence, Candidate/Authored merge including selection-aware phrase and note-range actions, Replace/Keep Authored confirmation with Pin refusal, typed preview/health, semantic diff, intermediate capture.

**Still open:**

1. Phase 0/2/15 screenshots, EN/zh-CN/ja walkthroughs, scenarios A–G, Windows portable packaging, and the release dry run.

Do not describe this design as a finished 0.5.0 feature until the remaining-work guide §0.1 has no Partial phases.
