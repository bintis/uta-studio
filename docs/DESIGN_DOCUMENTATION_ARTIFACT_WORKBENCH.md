# Uta! Studio — Documentation Center & Artifact Workbench

**Status:** current architecture summary
**Task state:** `tasks/remaining-models/STATE.md` remains the durable closure index.

## Purpose

The Documentation Center provides offline, native user documentation. The Artifact Workbench is the app-owned representation for inspecting immutable analysis artifacts, provenance, revisions, lineage, typed previews/diffs, and safe authoring imports.

This document describes Studio presentation and authoring behavior only. Analysis planning and execution belong exclusively to `uta-analyze`; Runtime lifecycle and policy belong exclusively to `uta-runtime`.

## Process boundary

```text
Desktop -> app-core -> uta-analyze / uta-runtime
```

Studio does not import either backend implementation crate. It does not contain an Analysis Engine planner, scheduler, fusion implementation, native-worker router, or Runtime Manager policy implementation.

The Workbench may project authoring impact from the current Studio Workflow definition so users can understand which authored stages are downstream. That projection is not an executable Analysis Plan. Exact requirements, routes, blockers, and execution order shown before a run come from the versioned Engine Plan returned by `uta-analyze`.

## Safety invariants

- Source media is read-only and never moved, rewritten, or deleted.
- Analyzer-produced revisions are immutable; edits create user/authored revisions.
- Candidate artifacts never silently replace Authored charts.
- Artifact actions use `ArtifactRef { file_hash, kind, revision_id }`; app-core resolves paths inside authorized cache roots.
- Pinned revisions cannot be deleted.
- Historical runs without exact lineage are shown as `LegacyUntracked`; missing relations are never fabricated.
- Result paths are confined, checked for existence and byte count, and rejected on symlink/path escape. Hash metadata is not an execution or publication gate.
- No Python, `uv`, virtual environment, script-runtime, or network inference fallback exists.

## Documentation Center

Canonical localized sources are:

- `docs/user-guide/en.md`
- `docs/user-guide/zh-CN.md`
- `docs/user-guide/ja.md`

`cargo xtask docs build` generates `docs/USER_GUIDE.md` and `desktop/assets/docs/docs.bundle.json`. The packaged desktop embeds the bundle and does not execute Markdown, HTML, JavaScript, remote images, or `file://` links.

The native viewer supports semantic deep links, context help, offline substring search (including CJK), and back/forward history. Document body text bypasses runtime string-catalog localization; viewer chrome remains localized through the normal catalogue.

## Artifact representation

`app-core/src/artifact_workbench/` owns:

- stable artifact references and media types;
- expected and resolved node I/O inspection;
- revision health and typed preview;
- exact revision lineage;
- typed text/JSON/pitch/chart comparisons;
- pinning and safe revision activation;
- lyrics/chart edit drafts and immutable authored commits;
- candidate-to-authored merge operations;
- read-only downstream authoring-impact projection;
- capture of artifacts already produced by a completed Engine run.

It does not persist intermediate-capture requests or ask a compatibility worker to materialize temporary arrays. Engine result commits are captured only after the Engine process returns valid, confined output references.

## Editing semantics

- Lyrics and timed transcripts open as drafts; unknown structured extensions round-trip where supported.
- Recognized text, pitch tracks, and note candidates remain evidence and are never edited in place.
- Candidate and Authored charts open by exact immutable revision.
- Full, phrase/range, lyric-timing, and pitch merges create a new Authored revision and preserve source provenance.
- Authored replacement remains explicit and refuses unsafe pinned-state transitions.

## Historical data

Database migrations needed to read existing libraries, artifact revisions, runs, and missing-lineage states remain supported. They are data compatibility, not executable compatibility: no retired analyzer, local planner, capture-request executor, or loose protocol is retained.

## Verification

Relevant checks are:

```sh
bash dev.sh -c cargo fmt --all -- --check
bash dev.sh -c cargo check --workspace --all-targets --locked
bash dev.sh -c cargo test -p uta-studio-core -p uta-studio-desktop --locked
cargo xtask docs check
```

Nix packaging and whole-repository release acceptance remain reserved for the explicit release pass.
