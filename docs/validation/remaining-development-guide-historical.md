# Uta Studio — Remaining Development Agent Guide

**Purpose:** execution contract for coding agents completing the Documentation Center and Analysis Artifact Workbench  
**Repository:** `bintis/uta-studio`  
**Baseline:** Uta Studio 0.5.x, after importing `uta-studio-0.5.0-documentation-artifact-workbench-20260818`  
**Document revision:** 2026-08-18  
**Primary design reference:** `docs/DESIGN_DOCUMENTATION_ARTIFACT_WORKBENCH.md`

---

## 0. Mission

Complete the remaining work required to turn the current prototype into a production-ready implementation of:

1. the native, offline, multilingual Documentation Center; and
2. the DAG Node I/O / Artifact Workbench, including immutable revisions, exact run lineage, compatible editing workflows, typed previews, impact analysis, lineage visualization, and intermediate-output capture.

The current imported code is **not** the completed feature. Treat it as:

> Documentation Center MVP + Artifact Workbench foundation.

Do not claim a phase is complete until its acceptance criteria and tests pass.

---

## 0.1 Implementation status — 2026-08-18 working tree

This section is the handoff truth for the current working tree. `Complete` means the production path exists and its relevant automated acceptance tests pass. `Partial` means useful production code exists but at least one acceptance item below is still missing or has not received direct manual evidence. The feature as a whole is **not yet release-complete** while any phase is Partial.

| Phase | Status | Completed and verified | Still incomplete or unverified |
|---|---|---|---|
| 0 — Baseline stabilization | **Partial** | Workspace compiles in `nix develop`; design and implementation inventory were updated; current Rust test suites pass. | GUI screenshots of Documentation, Node I/O, Artifact menu, and all three locales still need a display session. |
| 1 — Documentation sources/build | **Complete** | `docs/user-guide/{en,zh-CN,ja}.md` are canonical; `cargo xtask docs build/check` generate and validate the combined guide, embedded bundle, index, anchors, links, locale parity, and deterministic output offline. | None for this phase. |
| 2 — Markdown UI/search/navigation | **Partial** | Native Markdown AST view model, supported block/inline types, safe URI filtering, three-column/narrow layouts, exact-block search, CJK search, history, semantic links, F1, and dirty-editor routing are implemented and unit tested. | Manual narrow-window, large-font, keyboard/controller, back/forward, and EN/zh-CN/ja walkthrough evidence is still required. |
| 3 — Immutable revision store | **Complete** | Content-addressed immutable storage, path authorization, hash verification, migration, repair, Pin-aware cleanup, and later-canonical-overwrite protection are implemented and tested. | None for this phase. |
| 4 — Exact commit protocol | **Complete** | Attempt-specific input/output commit relations and Produced/Reused/Frozen/Bypassed/Source/Ephemeral/Missing/NotApplicable states are written at execution boundaries; immutable storage and DB relation writes are transactional; legacy history remains readable. | None for this phase. |
| 5 — Inspector/run-specific resolution | **Complete** | Overview, Inputs, Outputs, Attempts, Logs, and Help tabs use exact selected-run bindings, with explicitly labelled current-inventory fallback and stable tab/selection state. | None for this phase. |
| 6 — First-class nodes/edges | **Complete** | Artifact edges resolve the selected run's binding, show kind/revision/state, and select that binding on click. Binding-state colors distinguish produced, reused, frozen, bypassed, missing, and invalidated. Export nodes expose readiness, last destination, validate, re-export, reveal, and export documentation. | Manual screenshots remain Phase 15. |
| 7 — Artifact edit drafts | **Complete** | Drafts retain exact source revisions; validation, concurrent-Active protection, provenance, immutable authored saves, Save Only, and Save + Run Downstream are implemented and tested. | None for this phase. |
| 8 — Lyrics/TimedTranscript editors | **Complete** | Plain promotion and a dedicated lossless TimedTranscript surface provide segment/word timing, exact JSON preservation, real audio waveform/playback/jump, pointer-drag timing boundaries with cancel cleanup, validation, and CJK/repeated-token regression coverage. | None for this phase. |
| 9 — Pitch/Candidate/Authored workflows | **Complete** | CandidateChart is a distinct validated immutable chart. Exact Candidate/Authored revision loading, semantic compare, and immutable Authored saves are in place. PitchTrack/PitchNoteCandidates load the selected revision as editor evidence/import. Phrase and selected-note-range merge use the current editor selection from the Artifact menu and the note context menu. Replace/Keep Authored confirmation is a global dialog; pinned authored charts refuse Replace until unpinned. | Manual walkthrough screenshots remain Phase 15. |
| 10 — Typed preview/health | **Complete** | Bounded typed previews and validators are connected to the inspector for the target text, transcript, pitch, chart, audio, and metadata kinds; malformed data returns health details instead of crashing. | None for this phase. |
| 11 — Semantic typed diff | **Complete** | Revision-specific bounded semantic diffs cover ordered text, transcripts, pitch, note candidates, audio metadata, charts, and recursive JSON fallback in a dedicated panel. | None for this phase. |
| 12 — Visual Lineage | **Complete** | Lineage On/Off on the main DAG highlights selected/upstream/downstream nodes and edges, fades unrelated content, labels edges with kind and short revision id, and marks missing legacy gaps. MINI keeps compute nodes only and still highlights producer/consumer compute nodes. | Manual GUI walkthrough screenshots remain Phase 15. |
| 13 — State-aware Impact Preview | **Complete** | Impact groups come from one frozen `AnalysisPlan` that includes the song profile, staged Freeze/Bypass/Disable intents, and Pin. Confirmation queues that same request; a unit test asserts preview groups equal the plan built from the queued request. | Manual confirmation walkthrough remains Phase 15. |
| 14 — Intermediate capture | **Complete** | Explicit one-shot/persistent capture requests materialize actual preprocessed FLAC atomically, commit an exact immutable revision/relation, clear successful one-shot requests, and remain off for ordinary runs. | None for this phase. |
| 15 — Product integration/release | **Partial** | API catalogue, three locale catalogues, embedded docs bundle, changelog/design docs, Linux Nix package, real decode/export diagnostics, native playback, and Wayland packaged launch have passing evidence. | Manual scenarios A–G are not all recorded; portable Windows packaging is unverified; a final release dry run and screenshots remain outstanding. |

### Current automated evidence

- `nix develop --offline path:. -c cargo fmt --check`: pass.
- `nix develop --offline path:. -c cargo xtask docs check`: pass.
- `nix develop --offline path:. -c cargo test -p uta-studio-core --lib -- artifact_workbench::tests`: 17 passed, including frozen-plan preview/request equivalence.
- `nix develop --offline path:. -c cargo test -p uta-studio-desktop --offline`: 170 passed, including lineage highlight and MINI compute-only filtering.
- Earlier full verification in this working session also passed Python compile/tests, API diagnostics, real audio decode, real UTZ/UltraStar export, native playback, Wayland packaged smoke launch, and `nix build path:.#uta-studio`; these must be rerun after the final edits before release handoff. Manual screenshots, scenarios A–G, and Windows portable packaging remain outstanding (see `docs/WALKTHROUGH_DOCUMENTATION_ARTIFACTS.md`).

### Remaining implementation order

1. Record Phase 0/2/15 manual screenshots and EN/zh-CN/ja walkthroughs on a display session.
2. Verify portable Windows packaging and run a release dry run.
3. Record scenarios A–G from this guide.

### Documentation alignment — 2026-08-18

The following documents now match this table instead of describing the imported prototype as finished:

- `docs/DESIGN_DOCUMENTATION_ARTIFACT_WORKBENCH.md` status is **partial**, with §16 listing the same remaining gaps.
- `docs/user-guide/{en,zh-CN,ja}.md` add `guide:documentation` and `guide:artifacts` as real pages. Artifact help links resolve to `guide:artifacts`. Node help still opens the matching workflow chapter.
- `docs/USER_GUIDE.md` and `desktop/assets/docs/docs.bundle.json` are generated from those locale sources.

Section 2 below is the current inventory. Do not treat §2.3 of an older revision as live; the leftover prototype bullets were wrong after Phases 1–5, 7–8, 10–11, and 14 landed.

---

## 1. Non-negotiable engineering rules

Every agent must follow these rules.

### 1.1 Inspect before modifying

Before editing:

```sh
git status --short
git log -1 --oneline
cargo metadata --no-deps --format-version 1
```

Read:

```text
AGENTS.md
docs/engineering-constraints.md
docs/DESIGN_DOCUMENTATION_ARTIFACT_WORKBENCH.md
docs/analysis-dag-redesign.md
```

Also inspect the latest implementation of every target function. Do not assume the imported package still matches the current branch.

### 1.2 Keep the application’s safety boundaries

- Source media is read-only.
- No operation may move, rewrite, or delete source media.
- Analyzer output must never silently destroy the Authored Chart.
- Destructive operations require explicit confirmation.
- Every Artifact path must resolve inside an authorized cache or revision-store root.
- New Artifact-facing UI actions must carry `ArtifactRef`, not a free-form `PathBuf`.
- Never infer exact lineage from a filename or modification time when exact data is unavailable.
- Legacy data must be labeled `LegacyUntracked`.
- Ephemeral output must be labeled `Ephemeral`, not `Missing`.

### 1.3 Artifact revisions must become genuinely immutable

A revision is not immutable merely because Delete is disabled.

A production implementation must guarantee that two revisions never share a mutable canonical backing file. The storage layout must be content-addressed or otherwise revision-specific.

Required invariant:

```text
revision.content_hash == hash(bytes at revision.path)
```

This invariant must continue to hold after later analysis runs, cleanup, application restart, Set Active, Pin, Freeze, and editor saves.

### 1.4 Preserve compatibility

- Existing 0.5.x databases must migrate without data loss.
- Old analysis history must remain readable.
- Existing canonical cache files may remain as compatibility pointers or active materializations.
- Old runs without relation rows remain visible.
- Existing exports and editor flows must continue working.
- English remains the fallback UI locale.

### 1.5 All app-owned commands must be discoverable

Add every new read, mutation, destructive, external, or temporary operation to:

```text
app-core/src/api.rs
```

Update diagnostics tests when the capability count or catalogue changes.

### 1.6 Internationalization is part of completion

Every new user-facing string must be represented in:

```text
desktop/assets/i18n/en.json
desktop/assets/i18n/zh-CN.json
desktop/assets/i18n/ja.json
```

Do not rely on an English fallback as the final implementation.

### 1.7 No “paper completion”

The following do not count as completion:

- defining an enum without a consumer;
- adding a database table without a production writer;
- adding an API without UI or test coverage where the design requires UI;
- adding a button that always errors;
- displaying “exact” when data was reconstructed heuristically;
- declaring a validator but returning `NotChecked` for the target type;
- adding a context-help anchor that the document viewer cannot resolve.

---

## 2. Current implementation inventory

This inventory describes the working tree, not the original import package.

### 2.1 Documentation

- `StudioRoute::Documentation` with Settings, General, F1, node Help, and artifact About entrypoints
- Canonical locale sources and `cargo xtask docs build/check`
- Embedded `docs.bundle.json` with page IDs, heading anchors, and semantic links
- Native Markdown AST renderer, safe URI filtering, three-column and narrow layouts
- Exact-block search including CJK, history back/forward, dirty-editor F1 routing
- User-facing pages: `guide:documentation`, `guide:artifacts`
- Node help still maps to `guide:analysis` / `guide:lyrics` / `guide:editor`

### 2.2 Artifact Workbench

- Content-addressed immutable revision store; schema v10
- Exact attempt/input/output commit relations with Produced/Reused/Frozen/Bypassed/Source/Ephemeral/Missing/NotApplicable
- Inspector tabs: Overview, Inputs, Outputs, Attempts, Logs, Help
- Capability-gated artifact context menus and virtual artifact nodes
- Edit drafts with source provenance, Save Only, and Save + Run Downstream
- LyricsInput / lossless TimedTranscript editors
- CandidateChart as a distinct validated chart; core merge primitives exist
- Typed preview, health, and semantic revision diff panels
- Dedicated Lineage and Impact panels
- Explicit one-shot/persistent PreprocessedAudio capture as FLAC

### 2.3 Remaining gaps

These match §0.1. They are the only unfinished implementation items:

- Phase 0/2/15 manual screenshots and EN/zh-CN/ja walkthrough evidence
- Phase 15: scenarios A–G, Windows portable packaging, release dry run

---

## 3. Definition of Done

The feature is complete only when all of the following are true:

1. `cargo fmt --check` passes.
2. `cargo test --workspace` passes.
3. `cargo check --workspace` passes.
4. `nix build path:.#uta-studio` passes on the supported Linux environment.
5. Windows and Linux CI packages include the Documentation Center.
6. Every new DB migration is covered by upgrade and idempotency tests.
7. Every new app-owned command is in `API_CAPABILITIES`.
8. EN, zh-CN, and ja catalog keys are identical and non-empty.
9. Documentation search and deep links work in all three locales.
10. Artifact revision bytes remain immutable across later runs.
11. New runs record exact node input/output bindings without mtime inference.
12. Historical run selection resolves the Artifact revisions belonging to that run.
13. Pin protects a revision from deletion, cleanup, and replacement.
14. Editing a generated Artifact never overwrites that generated revision.
15. Save Only and Save + Run Downstream are distinct, explicit choices.
16. TimedTranscript editing preserves word-level timing.
17. Lineage mode visibly highlights graph provenance.
18. Impact confirmation uses the actual plan and current bindings.
19. Intermediate capture is explicit and functional.
20. Manual narrow-window, keyboard, mouse, and dirty-editor navigation checks pass.

---

# Part I — Stabilize and compile the imported prototype

## Phase 0 — Baseline stabilization

**Owner:** Integration Agent  
**Blocking:** all later phases

### Tasks

1. Import the package on a clean branch.
2. Run:

```sh
python3 scripts/build-user-guide.py --check
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

3. Fix all compile errors before adding new functionality.
4. Record the actual imported tree in:

```text
docs/DESIGN_DOCUMENTATION_ARTIFACT_WORKBENCH.md
```

Add an “Implementation status” section containing:

- commit SHA;
- schema version;
- tests run;
- known compile/runtime gaps;
- actual files added or changed.

5. Launch the application and capture screenshots of:
   - Documentation route;
   - DAG Node I/O card;
   - Artifact right-click menu;
   - English, Chinese, and Japanese UI.

### Acceptance criteria

- Workspace compiles.
- All existing tests pass.
- No existing behavior regression is knowingly accepted.
- The design document reflects reality rather than the original plan.

### Required tests

```sh
cargo test --workspace
cargo check --workspace
```

---

# Part II — Complete the Documentation Center

## Phase 1 — Canonical documentation sources and build pipeline

**Owner:** Documentation Infrastructure Agent  
**Can run in parallel with:** Phase 2  
**Files likely involved:**

```text
docs/USER_GUIDE.md
docs/user-guide/en.md
docs/user-guide/zh-CN.md
docs/user-guide/ja.md
xtask/src/*
scripts/build-user-guide.py
desktop/src/studio/documentation.rs
```

### Required architecture

The canonical source must be:

```text
docs/user-guide/en.md
docs/user-guide/zh-CN.md
docs/user-guide/ja.md
```

Generated compatibility outputs:

```text
docs/USER_GUIDE.md
desktop/assets/docs/docs.bundle.json
```

Do not keep the combined trilingual file as the authoritative source.

### Tasks

1. Move each language into a canonical single-language source.
2. Replace the standalone split script with an `xtask` command:

```sh
cargo xtask docs build
cargo xtask docs check
```

3. Generate:
   - the GitHub-facing combined guide;
   - a deterministic documentation bundle;
   - a search index;
   - stable semantic anchor metadata.
4. Store bundle metadata:
   - schema version;
   - app version range;
   - document revision;
   - locale;
   - page IDs;
   - heading IDs.
5. Validate:
   - identical required page IDs across locales;
   - no broken internal links;
   - no duplicate anchors;
   - no empty translated pages;
   - document version compatibility;
   - no stale hard-coded release package names.

### Acceptance criteria

- Editing one locale source and running `cargo xtask docs build` deterministically updates generated outputs.
- `cargo xtask docs check` fails on drift.
- All internal links resolve.
- The build does not require network access.

### Required tests

- bundle determinism;
- duplicate-anchor rejection;
- broken-link rejection;
- missing-locale-page rejection;
- version-placeholder expansion;
- `docs check` drift detection.

---

## Phase 2 — Real Markdown AST renderer and responsive UI

**Owner:** Documentation UI Agent  
**Depends on:** Phase 1 bundle contract  
**Files likely involved:**

```text
desktop/src/studio/documentation.rs
desktop/src/studio/mod.rs
desktop/src/studio/widgets.rs
desktop/src/studio/i18n.rs
desktop/src/studio/settings.rs
```

### Supported document blocks

Implement native rendering for:

- H1–H4;
- paragraphs;
- ordered lists;
- unordered lists;
- bold;
- italic;
- inline code;
- fenced code;
- tables;
- internal links;
- HTTPS external links;
- Note/Tip/Warning callouts.

Explicitly reject or ignore:

- arbitrary HTML;
- JavaScript;
- iframes;
- remote images loaded automatically;
- `file://` links;
- unsupported URI schemes.

### Layout

Wide window:

```text
Document tree | Article body | On-this-page headings
```

Narrow window:

- collapsible document tree;
- collapsible on-this-page menu;
- body uses full width;
- no fixed-width overlap.

### Navigation state

Add:

- documentation back stack;
- forward stack;
- current page ID;
- current anchor;
- per-page scroll position;
- query;
- selected search result.

Back must return to the actual previous application route, not always Home.

### Entry points

Complete all designed entrypoints:

- global sidebar Help;
- Settings;
- About;
- F1;
- DAG node context;
- Artifact context;
- chart-problem help;
- editor-shortcut help.

### Search

Search must:

- index headings and body separately;
- weight headings higher;
- support English and CJK;
- highlight the matching excerpt;
- navigate to the exact block;
- preserve the query when moving between results.

### Acceptance criteria

- all supported Markdown structures render correctly;
- no raw Markdown punctuation remains for supported syntax;
- narrow-window layout has no serious overlap;
- search jumps to an exact anchor;
- back/forward history works;
- external links require explicit user activation.

### Required tests

- AST-to-view-model tests;
- search ranking tests for English/Chinese/Japanese;
- anchor navigation tests;
- unsupported-scheme rejection;
- responsive-layout pure calculation tests;
- dirty-editor F1 navigation tests.

---

# Part III — Make Artifact revisions truly immutable

## Phase 3 — Content-addressed revision store

**Owner:** Artifact Storage Agent  
**Blocking:** exact lineage, editing revisions, reliable Pin  
**Files likely involved:**

```text
app-core/src/analysis_artifact.rs
app-core/src/artifact_workbench.rs
app-core/src/cache.rs
app-core/src/chart.rs
app-core/src/library_db/schema.rs
app-core/src/library_db/analysis_artifacts.rs
app-core/src/library_db/mod.rs
```

### Required storage layout

Use a revision-specific path, for example:

```text
<cache-root>/artifact-store/
  <file-hash>/
    <artifact-kind>/
      <content-hash>.<extension>
```

The exact layout may differ, but a revision path must never be a mutable canonical work file.

### Canonical compatibility files

Existing paths such as:

```text
pitch_track.json
timed_transcript.json
vocal_chart.json
vocals.flac
```

may remain as:

- active materializations;
- compatibility copies;
- generated working files.

They must not be the immutable revision file.

### Tasks

1. Introduce `ArtifactStore`.
2. On revision capture:
   - validate source path is within the allowed cache root;
   - hash bytes;
   - copy or hard-link into a temporary revision-store file;
   - `fsync`;
   - atomically rename to the content-addressed destination;
   - verify destination hash;
   - insert/update the DB row using the immutable destination path.
3. Add fields if needed:
   - `producer_run_id`;
   - `producer_attempt_id`;
   - `storage_version`;
   - `media_type`;
   - `pinned`.
4. Migrate existing rows:
   - copy existing bytes into the revision store;
   - update row path;
   - preserve Active, Legacy, Invalidated, and Pin;
   - never delete the original compatibility file during migration.
5. Add a repair command:
   - detect hash/path mismatch;
   - detect missing immutable files;
   - rebuild a missing revision from a matching canonical file only when content hash matches;
   - otherwise report corruption.
6. Update cleanup:
   - clean unreferenced, unpinned revision files;
   - preserve pinned revisions;
   - do not preserve all canonical files merely because one revision was pinned.

### Invariants

Test these explicitly:

```text
hash(revision.path) == revision.content_hash
revision A path != revision B path when content differs
later analysis cannot change revision A bytes
Set Active does not modify revision bytes
Pin does not alter Active
Invalidate does not delete bytes
Delete removes only the selected revision file and row
```

### Acceptance criteria

- a new run produces immutable revision paths;
- an old revision remains byte-identical after a later run overwrites canonical working files;
- Pin protects the immutable revision;
- DB migration is idempotent.

### Required tests

- fresh store write;
- duplicate content reuses the same content-addressed file;
- different content creates a different file;
- migration from schema v8;
- path escape rejection;
- corruption detection;
- cleanup with pinned/unpinned revisions;
- later run cannot mutate an earlier revision.

---

# Part IV — Record exact run/node I/O at execution boundaries

## Phase 4 — Exact Artifact commit protocol

**Owner:** Analyzer Protocol Agent  
**Depends on:** Phase 3  
**Files likely involved:**

```text
app-core/src/analyzer.rs
app-core/src/analysis_artifact.rs
app-core/src/artifact_workbench.rs
app-core/src/library_db/analysis_node_artifacts.rs
app-core/src/library_db/analysis_node_attempts.rs
app-core/src/library_db/schema.rs
app-core/analyzer/pipeline.py
app-core/analyzer/server.py
app-core/analyzer/whisper_compat.py
app-core/analyzer/*.py
```

### Remove the mtime reconstruction path

The production writer must not decide “produced vs reused” from a time window.

Remove or demote `capture_analysis_run_artifacts` post-run scanning once the exact writer is live.

### Required protocol

Each real node boundary must emit structured events:

```json
{
  "event": "artifact_committed",
  "run_id": 123,
  "attempt_id": 456,
  "node_id": "lyrics.align",
  "direction": "output",
  "slot": "timed_transcript",
  "artifact_kind": "TimedTranscript",
  "path": "...",
  "binding_kind": "produced",
  "input_revision_ids": ["..."],
  "config_hash": "...",
  "algorithm_version": "..."
}
```

Equivalent typed structures are acceptable.

### Binding kinds

At minimum:

```text
Produced
Reused
Frozen
Bypassed
Source
Ephemeral
Missing
NotApplicable
```

### Tasks

1. Add `attempt_id` to relation rows.
2. Record input bindings when the node attempt starts.
3. Record output bindings after the file’s atomic commit succeeds.
4. Record Frozen/Bypass explicitly from the AnalysisPlan/request.
5. Record Ephemeral without a fake revision.
6. Record NotApplicable for route-gated nodes only where useful; do not create noise for every unused graph node.
7. Write ArtifactRevision and relation rows in one SQLite transaction after immutable storage succeeds.
8. If DB recording fails:
   - keep the committed file;
   - emit a visible error;
   - make repair/import possible;
   - do not silently swallow the error.
9. Ensure Rust-side flows also record exact events:
   - timed LRC import;
   - direct lyrics save;
   - chart save;
   - USDX/native import paths where applicable.
10. Preserve old event deserialization with `#[serde(default)]`.

### Acceptance criteria

- a completed new run has exact attempt→input/output rows;
- Frozen and Bypassed display accurately;
- no production code uses mtime to claim exact binding;
- relation writer errors are surfaced;
- old history still loads.

### Required tests

- produced output;
- cache reuse;
- frozen reuse;
- bypass with source media;
- ephemeral input/output;
- node failure before output commit;
- DB failure after file commit;
- timed LRC Rust-side run;
- old snapshot compatibility.

---

# Part V — Complete the Node I/O Inspector and historical DAG behavior

## Phase 5 — Inspector tabs and run-specific resolution

**Owner:** DAG Inspector Agent  
**Depends on:** Phase 4  
**Files likely involved:**

```text
desktop/src/studio/analysis_render.rs
desktop/src/studio/analysis_actions.rs
desktop/src/studio/artifact_workbench_ui.rs
desktop/src/studio/analysis_model.rs
desktop/src/studio/mod.rs
app-core/src/artifact_workbench.rs
```

### Required tabs

```text
Overview | Inputs | Outputs | Attempts | Logs | Help
```

Add session state:

- selected node ID;
- selected inspector tab;
- selected ArtifactRef;
- selected run ID;
- selected revision;
- lineage mode;
- impact preview state.

### Inputs tab

Show:

- declared input kind;
- resolved binding;
- exact revision;
- producer node;
- producer run and attempt;
- content hash;
- size;
- Active/Pin/Invalidated/Legacy;
- binding kind;
- missing explanation.

### Outputs tab

Show:

- this attempt’s output revisions;
- Active revision separately;
- historical revisions;
- downstream consumers;
- health;
- Pin;
- actions.

### Attempts tab

Show attempts across runs:

- run timestamp;
- status;
- implementation/model;
- requested/actual device;
- fallback;
- duration;
- input/output revisions;
- error;
- compare action.

### Historical correctness

When a historical run is selected:

- Artifact DAG node primary action must resolve that run’s output relation;
- do not default to current Active;
- if the run has no exact binding, state that explicitly;
- current inventory may be shown only in a separately labeled section.

### UI behavior

- wide screen: right-side inspector;
- narrow screen: inspector below graph or full-screen drawer;
- selection remains stable during graph refresh;
- keyboard navigation reaches all tabs and actions.

### Acceptance criteria

- every tab is functional;
- historical run Artifact selection is run-specific;
- exact and fallback inventory are visually distinct;
- inspector does not reread the same DB/file data repeatedly during one render.

### Required tests

- run-specific revision selection;
- fallback labeling;
- tab-state transitions;
- compound child selection;
- narrow/wide layout calculation;
- keyboard focus order.

---

## Phase 6 — Artifact nodes and edges as first-class interactive data

**Owner:** DAG Interaction Agent  
**Depends on:** Phase 5  
**Files likely involved:**

```text
desktop/src/studio/analysis_render.rs
desktop/src/studio/analysis_model.rs
desktop/src/studio/analysis_layout.rs
desktop/src/studio/artifact_workbench_ui.rs
```

### Artifact context menu must include capability-gated actions

```text
Preview / Play
Open in compatible editor
View revisions
Set active
Compare with active
Inspect provenance
Reveal
Pin / Unpin
Invalidate
Delete
Open Artifact documentation
```

Only show valid actions.

### Edge interaction

Implement:

- hover label showing ArtifactKind;
- click selects the concrete binding;
- run-specific revision ID;
- producer and consumer highlighting;
- distinct styles for:
  - produced;
  - reused;
  - frozen;
  - bypassed;
  - missing;
  - invalidated.

### Export nodes

Make export nodes interactive:

- readiness;
- last export destination if tracked;
- validate;
- re-export;
- reveal output;
- export documentation.

Do not represent an export as an Analysis Artifact revision unless the domain model explicitly adds exported packages as such.

### Acceptance criteria

- Artifact nodes and edges expose the selected run’s data;
- no menu item is guaranteed to error;
- Bypass and Frozen are visually distinguishable;
- export nodes have useful actions.

---

# Part VI — Complete safe Artifact editing workflows

## Phase 7 — Artifact edit draft model

**Owner:** Editing Infrastructure Agent  
**Depends on:** Phases 3–5  
**Files likely involved:**

```text
app-core/src/artifact_workbench.rs
app-core/src/lyrics.rs
app-core/src/chart.rs
app-core/src/analysis_artifact.rs
desktop/src/studio/song_detail.rs
desktop/src/studio/editor/*
desktop/src/studio/mod.rs
```

### Required model

```rust
ArtifactEditDraft {
    source: ArtifactRef,
    draft_kind: ...,
    original_content_hash: String,
    working_copy: ...,
    dirty: bool,
    validation: ...,
}
```

The editor must retain `source_revision`.

### Save choices

Every Artifact edit must expose distinct actions:

```text
Save Only
Save and Run Downstream
Cancel
```

`Save Only`:

- validates;
- writes a new user-authored immutable revision;
- updates Active only after explicit policy;
- marks dependent analysis output stale;
- does not queue analysis.

`Save and Run Downstream`:

- first commits the new revision;
- shows state-aware Impact Preview;
- queues the actual downstream plan only after confirmation.

### Provenance

A user-authored revision must record:

- producer node or producer identity such as `user.lyrics_editor`;
- source revision ID;
- app version;
- schema/algorithm version;
- timestamp;
- content hash;
- input revisions.

### Concurrency

Reject save if the source revision or Active selection changed while the draft was open, unless the user explicitly chooses to fork from the old revision.

### Acceptance criteria

- generated revision bytes are never overwritten;
- a save creates a new revision;
- source provenance is retained;
- Save Only queues nothing;
- Save and Run Downstream uses the plan preview.

### Required tests

- draft from LyricsInput;
- draft from RecognizedText;
- draft from TimedTranscript;
- dirty cancel;
- concurrent Active change;
- validation failure;
- Save Only;
- Save and Downstream;
- source revision survives unchanged.

---

## Phase 8 — Lyrics and TimedTranscript editors

**Owner:** Lyrics/Timing Agent  
**Depends on:** Phase 7

### LyricsInput and RecognizedText

- LyricsInput opens as editable plain lyrics.
- RecognizedText opens through `Promote to lyrics draft`.
- Saving creates a user LyricsInput revision.
- Do not label RecognizedText itself as edited.

### Dedicated TimedTranscript editor

Do not reduce a TimedTranscript to line-level LRC.

Required capabilities:

- segment list;
- word/token list;
- start/end editing;
- waveform/audio playback;
- jump to time;
- drag timing boundaries;
- repeated-line handling;
- overlap detection;
- non-monotonic timing detection;
- word-outside-segment detection;
- negative/non-finite time rejection;
- preserve unknown extension fields;
- LRC import/export as a conversion, not the canonical storage model.

### Acceptance criteria

- word-level timing round-trips without loss;
- invalid timing cannot be saved;
- editing one word does not rewrite unrelated data;
- source revision stays immutable;
- saved revision can feed `chart.build_candidate`.

### Required tests

- word timing round-trip;
- overlap validator;
- segment containment;
- Unicode/CJK text;
- repeated lyric lines;
- LRC conversion;
- preservation of unknown JSON fields.

---

## Phase 9 — Pitch, Candidate Chart, and Authored Chart workflows

**Owner:** Chart Integration Agent  
**Depends on:** Phases 3, 7

### PitchTrack

- read-only evidence;
- waveform/pitch curve preview;
- playback;
- jump into Chart Editor at a selected time.

### PitchNoteCandidates

- preview candidate notes;
- import selected/all candidates into an editor working copy;
- never mutate the candidate revision.

### CandidateChart

Make CandidateChart a real versioned Artifact.

Required operations:

```text
Preview
Compare with Authored
Merge into working copy
Replace after confirmation
Keep Authored
```

### AuthoredChart

- open a specific revision, not merely the current canonical file;
- saving creates a new AuthoredChart revision;
- Active policy is explicit;
- Pin is honored;
- Replace refuses pinned Authored revisions until explicitly unpinned.

### Merge

At minimum support:

- replace phrase;
- replace selected note range;
- take candidate lyrics timing;
- take candidate pitch only;
- keep authored track metadata.

### Acceptance criteria

- Candidate and Authored are distinct revisions;
- compare is semantic, not top-level JSON only;
- Merge produces a new Authored revision;
- Replace is confirmed and respects Pin.

---

# Part VII — Typed preview, validation, and diff

## Phase 10 — Artifact preview and health system

**Owner:** Preview/Validation Agent  
**Can run partly in parallel with:** Phases 8–9  
**Files likely involved:**

```text
app-core/src/artifact_workbench.rs
app-core/src/chart.rs
app-core/src/lrc.rs
app-core/src/editor/*
desktop/src/studio/artifact_workbench_ui.rs
```

### Required previewers

- SourceMedia: media metadata.
- Audio stems: playback, duration, sample rate, channels, waveform, optional loudness.
- LyricsInput: text.
- RecognizedText: structured segments/text.
- AsrSegments: timeline.
- TimedTranscript: segment/word timeline.
- PitchTrack: pitch curve.
- PitchNoteCandidates: note overlay.
- MusicAnalysis: key/BPM/descriptors.
- CandidateChart/AuthoredChart: chart summary and preview.
- Export outputs: package validation result.

### Required validators

- file existence and size;
- content-hash verification;
- JSON schema/shape;
- LRC syntax;
- transcript ordering and containment;
- pitch MIDI range;
- confidence range;
- chart validation and problem count;
- audio decode;
- export package completeness.

### Health states

```text
Valid
Warning
Invalid
NotChecked
```

`NotChecked` is acceptable only when no validator exists yet and must not remain for the target kinds at final completion.

### Acceptance criteria

- typed preview APIs are actually used by UI;
- health details are visible;
- bounded reads prevent UI memory abuse;
- malformed artifacts do not crash the application.

---

## Phase 11 — Semantic typed diff

**Owner:** Diff Agent  
**Depends on:** Phase 10

### Required diffs

- text: ordered line diff preserving duplicates;
- LyricsInput: per-line change;
- TimedTranscript: text/start/end/word timing changes;
- PitchTrack: aligned curve summary;
- PitchNoteCandidates: added/removed/moved/transposed notes;
- Audio: duration/sample rate/channels/content metadata;
- Chart: tracks/phrases/notes/lyrics/note kinds;
- JSON fallback: recursive structural diff.

### UI

Add a dedicated diff panel. Do not place a long diff in `session.notice`.

### Acceptance criteria

- comparison is revision-specific;
- large diffs are virtualized or bounded;
- same-content revisions report byte identity;
- semantic changes are understandable to a non-developer.

---

# Part VIII — Lineage and state-aware impact

## Phase 12 — Visual Lineage mode

**Owner:** DAG Visualization Agent  
**Depends on:** Phases 4–6

### Required behavior

When an Artifact or node is selected in Lineage mode:

- selected item is emphasized;
- upstream producers and revisions are highlighted;
- downstream consumers are highlighted;
- unrelated nodes and edges are de-emphasized;
- missing legacy links appear as explicit gaps;
- edge labels show ArtifactKind and revision short ID;
- clicking a revision opens its inspector.

### Controls

```text
Lineage On/Off
Upstream only
Downstream only
Full lineage
Return to run view
```

### Acceptance criteria

- lineage is run-specific;
- no fabricated edge appears;
- graph remains navigable with Pan/Zoom/Fit;
- MINI view behavior is defined and tested.

---

## Phase 13 — State-aware Impact Preview

**Owner:** Plan/Impact Agent  
**Depends on:** exact bindings and edit drafts

### Impact must use

- current AnalysisPlan;
- selected run or prospective run;
- current Active revisions;
- pinned/frozen/bypassed state;
- current song profile and run overrides;
- chart staleness;
- export readiness.

### Trigger Impact Preview before

- Save and Run Downstream;
- Set Active;
- Invalidate;
- Delete an Active or consumed revision;
- parameter changes;
- Freeze;
- Bypass;
- Disable;
- Candidate merge/replace.

### UI groups

```text
Will run
Will reuse
Will become stale
Will be blocked
Will remain preserved
Exports needing regeneration
```

### Acceptance criteria

- the preview agrees with the execution plan;
- Authored Chart preservation is explicit;
- no mutation happens from the preview itself;
- confirmation commits exactly the previewed request.

---

# Part IX — Intermediate capture

## Phase 14 — Capture ephemeral output on next run

**Owner:** Analyzer Capture Agent  
**Depends on:** exact protocol and immutable store

### Required workflow

Artifact/Node menu:

```text
Capture intermediate output on next run
```

Confirmation shows:

- output kind;
- expected disk usage when estimable;
- whether capture applies once or persistently;
- privacy/storage implications.

### Required implementation

- persistent or one-shot capture request;
- request included in the frozen run configuration;
- pipeline materializes the requested intermediate atomically;
- exact Artifact revision and relation recorded;
- one-shot request clears after successful capture;
- cleanup respects Pin.

### Do not

- retain every temporary file by default;
- silently enable capture;
- call an ephemeral file “captured” before immutable storage succeeds.

### Acceptance criteria

- PreprocessedAudio can be captured explicitly;
- ordinary runs remain unchanged;
- captured output appears in Node I/O and Lineage;
- disk cleanup behaves correctly.

---

# Part X — Final integration, localization, packaging, and QA

## Phase 15 — Complete product integration

**Owner:** Integration/Release Agent  
**Depends on:** all prior phases

### Documentation links

Every semantic link must resolve:

```text
guide:getting-started
guide:analysis
guide:lyrics
guide:editor
guide:export
guide:troubleshooting
guide:documentation
guide:artifacts
node:<node-id>
artifact:<artifact-kind>
problem:<problem-kind>
```

`guide:documentation` and `guide:artifacts` are real user-guide pages. Artifact help aliases to `guide:artifacts`. Node help still aliases to the matching workflow chapter (`guide:analysis`, `guide:lyrics`, `guide:editor`) and is documented as such.

### Localization

Run a key parity test and manually review:

- English;
- Simplified Chinese;
- Japanese.

Translate:

- all tabs;
- statuses;
- validation errors;
- lineage labels;
- impact groups;
- editor confirmations;
- intermediate capture;
- migration warnings.

### Accessibility and input

Test:

- keyboard;
- mouse;
- controller/navigation abstraction where supported;
- focus visibility;
- Escape handling;
- F1;
- narrow window;
- large font scale;
- dark/light themes.

### Packaging

Verify that:

- docs bundle is embedded;
- no source Markdown is required at runtime;
- schema migration runs on upgrade;
- portable Windows and Linux packages work;
- Nix package includes no missing asset.

### Release notes

Update:

```text
CHANGELOG.md
README.md
docs/USER_GUIDE.md
docs/DESIGN_DOCUMENTATION_ARTIFACT_WORKBENCH.md
```

Describe migration and Pin semantics precisely.

---

# 4. Recommended multi-agent decomposition

Use separate branches or worktrees. Avoid concurrent edits to the same high-conflict files.

| Agent | Primary ownership | Avoid editing |
|---|---|---|
| Integration Agent | build, merge, `mod.rs`, final CI | feature internals until handoff |
| Documentation Infrastructure | `docs/`, `xtask` docs bundle | DAG UI |
| Documentation UI | `documentation.rs`, doc widgets | DB/schema |
| Artifact Storage | Artifact store, DB migrations, cleanup | desktop rendering |
| Analyzer Protocol | Python/Rust events, exact relation writer | Documentation UI |
| DAG Inspector | Inspector tabs and run-specific resolution | Artifact file storage |
| DAG Interaction | Artifact nodes, edges, menus, lineage visuals | schema |
| Editing Infrastructure | edit draft, save policies, provenance | documentation |
| Lyrics/Timing | lyrics and timing editor | pitch/chart merge |
| Chart Integration | pitch/candidate/authored workflows | docs |
| Preview/Validation | typed viewers and health | pipeline scheduling |
| Diff Agent | semantic diff API and panel | storage migration |
| Plan/Impact | plan-aware impact preview | Markdown |
| Capture Agent | explicit intermediate retention | unrelated cache behavior |
| QA/Release | cross-platform checks, localization, packaging | feature scope changes |

## Merge order

Recommended merge sequence:

```text
0 Baseline stabilization
1 Documentation source/build
3 Immutable Artifact store
4 Exact commit protocol
2 Documentation UI
5 Inspector
6 Artifact/edge interaction
7 Edit draft
8 Timing editor
9 Chart workflows
10 Preview/Health
11 Diff
12 Lineage
13 Impact
14 Intermediate capture
15 Integration/Release
```

Documentation UI may merge earlier if its bundle contract is stable.

---

# 5. Handoff contract for every agent

Every agent handoff must include:

```markdown
## Scope completed

## Files changed

## Schema/API changes

## Safety invariants checked

## Tests added

## Commands run and results

## Manual checks performed

## Known limitations

## Follow-up dependencies
```

Do not hand off with only “implemented” or “tests pass”.

---

# 6. Pull request checklist

Each PR must answer:

- [ ] Does this modify source media?
- [ ] Can this overwrite an existing Artifact revision?
- [ ] Is every path validated against an authorized root?
- [ ] Does this preserve the Authored Chart by default?
- [ ] Is a destructive action confirmed?
- [ ] Does the UI distinguish exact data from fallback data?
- [ ] Are old DB/history records compatible?
- [ ] Are all app-owned commands catalogued?
- [ ] Are EN/zh-CN/ja strings present?
- [ ] Are tests included?
- [ ] Were `cargo fmt`, `cargo test`, and `cargo check` run?
- [ ] Was the design status updated?

---

# 7. Minimum test matrix

## Core

```sh
cargo test -p uta-studio-core
```

Required categories:

- database migrations;
- immutable storage;
- path boundary;
- Artifact capture;
- lineage;
- impact;
- editing provenance;
- validators;
- semantic diff;
- cleanup and Pin.

## Desktop

```sh
cargo test -p uta-studio-desktop
```

Required categories:

- Documentation search and navigation;
- route/back stack;
- dirty-editor confirmation;
- inspector tabs;
- historical revision selection;
- capability-gated menus;
- lineage visual model;
- impact dialog model;
- narrow layout;
- i18n key coverage.

## Python analyzer

Run the repository’s Python analyzer tests, including new tests for:

- artifact commit events;
- reused outputs;
- frozen/bypassed bindings;
- intermediate capture;
- failed node without output commit;
- output path confinement.

## Workspace and package

```sh
cargo fmt --check
cargo check --workspace
cargo test --workspace
nix build path:.#uta-studio
```

Also execute the existing GitHub Actions release workflow on a draft tag or equivalent dry-run branch.

---

# 8. Manual acceptance scenarios

## Scenario A — Documentation

1. Switch UI to Chinese.
2. Open Settings → User guide.
3. Search `对齐`.
4. Jump to the timing section.
5. Follow an internal link.
6. Navigate back.
7. Resize to a narrow window.
8. Press F1 from the Editor with unsaved changes.
9. Confirm that leave protection appears.
10. Repeat in English and Japanese.

## Scenario B — Exact run I/O

1. Analyze a song.
2. Select the run.
3. Select `lyrics.align`.
4. Verify exact input revision IDs and output revision.
5. Run again with cache reuse.
6. Verify `Reused`.
7. Freeze a node and run.
8. Verify `Frozen`.
9. Bypass stems and run.
10. Verify SourceMedia bypass binding.

## Scenario C — Immutable revision

1. Record PitchTrack revision A.
2. Pin A.
3. Run pitch analysis again to produce B.
4. Verify A bytes and hash are unchanged.
5. Set B Active.
6. Verify A remains pinned and readable.
7. Clear generated cache.
8. Verify A survives, B cleanup follows policy.

## Scenario D — Lyrics edit

1. Open a RecognizedText revision.
2. Promote to lyrics draft.
3. Correct text.
4. Choose Save Only.
5. Verify a new LyricsInput revision exists.
6. Verify no analysis was queued.
7. Choose Save and Run Downstream.
8. Review Impact Preview.
9. Confirm.
10. Verify exact downstream plan and Authored Chart preservation.

## Scenario E — TimedTranscript

1. Open a word-timed transcript.
2. Move one word boundary.
3. Save.
4. Reload the new revision.
5. Verify all unrelated word timings and extension fields survive exactly.

## Scenario F — Historical Artifact

1. Select an old run.
2. Right-click its TimedTranscript Artifact node.
3. Verify the menu references the old run’s revision.
4. Switch to the latest run.
5. Verify the revision changes.
6. Confirm current Active is shown separately, not substituted silently.

## Scenario G — Lineage and Impact

1. Select AuthoredChart.
2. Enable Lineage.
3. Verify upstream transcript and pitch paths highlight.
4. Select a missing legacy input.
5. Verify an explicit gap.
6. Request Invalidate on an upstream revision.
7. Verify Impact Preview lists affected nodes and exports.
8. Cancel and confirm no mutation occurred.

---

# 9. Completion report template

When all phases are finished, produce:

```markdown
# Documentation Center & Artifact Workbench Completion Report

## Final commit

## Schema version

## Feature checklist

## Migration behavior

## API catalogue additions

## Test results

## Cross-platform build results

## Manual acceptance results

## Known residual limitations

## Release packaging
```

The completion report must link each design requirement to:

- implementation files;
- tests;
- acceptance evidence.

---

## Final instruction to agents

Prefer a smaller, fully correct vertical slice over broad placeholder coverage. However, do not stop at the imported prototype’s abstractions. The remaining work is specifically about closing the semantic gaps:

- real immutable revision bytes;
- exact execution-time lineage;
- run-specific UI;
- safe revision-based editing;
- meaningful typed visualization;
- state-aware impact;
- functional intermediate capture;
- production verification.

Update the design status after every merged phase so the repository never again describes a prototype as a completed production feature.
