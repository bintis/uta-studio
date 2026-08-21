# Analysis DAG Redesign — Design Contract (Phase 0)

> Implementation status (updated after each phase per Agent Rule 12):
> - **Phase 0** — done. Design doc + regression baseline (`cache.rs`,
>   `test_pipeline_cache.py`).
> - **Phase 1** — done. `app-core/src/analysis_graph.rs` (`AnalysisNodeId`,
>   `ArtifactKind`, `AnalysisGraphSpec`, validation/topo/dependency closure),
>   `analysis_plan.rs` (`AnalysisRequest`, `AnalysisPlan`, `LyricsRoute`,
>   `build_plan`, `get_analysis_graph`, `preview_analysis_plan`),
>   `analysis_profile.rs` (`AnalysisProfileSnapshot`). 35 tests. One graph
>   fix found by testing, not by inspection: `chart.build_candidate` needed
>   a direct edge from `lyrics.transcribe` for the Parakeet route (ASR emits
>   timing directly, bypassing `lyrics.align`) — §5's table already
>   documented this branch rule, the graph just hadn't wired it yet.
> - **Phase 2** — done. `analysis_artifact.rs` (`ArtifactRevision`,
>   `compute_config_hash` generalizing the stem-separation signature
>   pattern to every node, `import_legacy_artifacts`, active-revision
>   selection with a cache-root escape check), new `library_db` tables
>   `analysis_artifacts` and `song_analysis_profiles` (schema version 3).
>   **Deliberate resequencing from the phase plan's exact grouping:**
>   `analysis_runs` and `analysis_node_attempts` (phase plan §2.3) are
>   *not* created yet — those columns (fallback tracking, actual device,
>   per-attempt status) would sit empty until Phase 3's event protocol is
>   the thing populating them. Creating that schema now would be exactly
>   the "half-finished implementation" this repo's engineering guidance
>   warns against; it lands with Phase 3 instead, where there's an actual
>   writer for it.
> - **Phase 3** — done, additive scope. `whisper_compat.progress_node`/
>   `artifact_reused` emit explicit `node_id`/`event`/`reason` metadata
>   alongside the existing `pct`/`msg`; `server.py::_progress_payload`
>   passes them straight through as new `node_id`/`event`/
>   `artifact_reused_reason` wire fields while computing every pre-Phase-3
>   field (`stage`, `stage_progress`, ...) identically. Wired at the node
>   boundaries that actually exist as separate code paths today:
>   `preflight`, `music.analysis`, `stems.separate`, `pitch.extract`,
>   `lyrics.align`/`lyrics.transcribe` (chosen by `transcribe_or_align`'s
>   existing branch), `chart.build_candidate`, plus `artifact_reused` at
>   all 3 known cache-hit sites. Rust (`analyzer.rs`) parses the new fields
>   into `AnalysisProgressSnapshot.node_id`/`node_event`/
>   `artifact_reused_reason`, `#[serde(default)]`'d so old
>   `analysis_history.snapshot_json` rows keep deserializing. 7 new Rust
>   tests (all passing) + 8 new Python tests (4 run and pass against
>   `whisper_compat` in this environment; 4 more needing `server`/`pipeline`
>   skip for the same pre-existing broken-numpy reason as Phase 0's tests).
>   **Known gaps, honestly scoped rather than papered over:**
>   - `lyrics.preprocess` has no dedicated event yet — pipeline.py doesn't
>     have a separate preprocessing function to hang one on (that's Phase
>     4.2's job); emitting one now would fabricate a boundary that doesn't
>     exist in the actual call graph.
>   - `lyrics.import_timed` (the Timed LRC path) runs entirely in Rust
>     (`lyrics.rs::apply_timed_lyrics`), outside this Python event stream;
>     no event is emitted for it yet.
>   - `_classify_progress`/`STAGE_RANGES` (the old text classifier) is
>     **not removed** — every call site still computes `stage` the old way
>     regardless of `node_id`. This is intentional per phase plan §3.3
>     (Legacy Adapter) and because today's desktop UI (`analysis.rs`,
>     rewritten in Phase 7) still reads `stage`, not `node_id`.
>   - No `analysis_runs`/`analysis_node_attempts` DB writer is wired to the
>     live queue/worker yet. The wire protocol now carries everything such
>     a writer would need per event, but actually persisting each real
>     attempt during a live run is deferred until it can be verified against
>     a real pipeline execution (this sandbox has no working `torch`+
>     `numpy` environment to run one against — see Phase 0's environment
>     note). Building that writer blind, in a phase that touches the
>     production analyzer worker, was judged too risky to do unverified.
> - **Phase 4** — partial, honestly scoped. Replaced the `FORCE_TRANSCRIBE`/
>   `STEMS_ONLY`/`PITCH_ONLY` `HashSet<String>` trio in `analyzer.rs` with
>   one `PENDING_NODE_INTENTS: HashMap<String, PendingNodeIntent>` (a
>   `targets: BTreeSet<AnalysisNodeId>` + `force_transcribe: bool`), and a
>   new `pipeline_flags_for_targets` function that asks Phase 1's real
>   `build_plan` what would run for a target set, then derives
>   `skip_transcription`/`skip_separation` from the resulting plan's
>   `will_run` flags instead of three independently-hand-maintained special
>   cases. `mark_stems_only`/`reanalyze_pitch`/`reanalyze_force_transcribe`
>   now stash into this one map. The Python wire protocol is **unchanged**
>   (still two booleans) — only how Rust decides them changed, which is
>   what made this verifiable without a live pipeline run: 6 new Rust
>   tests, all passing, all exercising the real `build_plan` code path.
>   **Not done in this pass** (deferred, not silently skipped):
>   - §4.1's full enqueue-time config freeze: `AppConfig::load()` is still
>     read fresh inside `process_song` at execution time, not snapshotted
>     at enqueue time. Only the node-targeting intent (the three former
>     special flags) is now frozen at enqueue-adjacent call sites; global
>     separator/model/device settings changed after enqueue can still
>     affect an already-queued run. Closing this gap means threading a full
>     config snapshot through the queue, a larger change than the
>     special-flag replacement and not attempted blind in this pass.
>   - §4.2's pipeline function split (`run_preflight`, `run_music_analysis`,
>     etc. as independently callable, cache-checking, atomically-writing
>     units) — `pipeline.py::run_pipeline` is still one function. Phase 3's
>     node events are additive markers around the existing control flow,
>     not a restructuring of it.
>   - §4.3 dynamic lyrics paths: already modeled correctly in Phase 1's
>     `LyricsRoute`; not re-touched here.
>   - §4.4 artifact splitting (`recognized_text`/`asr_segments`/
>     `timed_transcript` out of the combined `transcript.json`): **done** in
>     a later session (see `docs/plan.md` §4.4 for full detail/verification).
>     `transcript.json` is kept, unchanged, as a permanent compatibility
>     file per this doc's own §4/§14 notes below; the three new files are
>     additive. `recognized_text.json`/`asr_segments.json` are only written
>     on ASR routes (`lyrics.transcribe`); `timed_transcript.json` is written
>     by every route via `chart.build_candidate` (Python) or
>     `lyrics.rs::write_transcript_json`/`usdx.rs::build_usdx_song` (Rust,
>     for the two routes that never enter the Python pipeline). Freeze for
>     the three lyrics nodes remains a separate, not-yet-done follow-on this
>     unblocks but doesn't itself implement.
>   - §4.5 Bypass (e.g. route around `stems.separate` via Original Mix):
>     `DisablePolicy` exists in Phase 1's model but no bypass-input
>     mechanism is wired to the live pipeline yet.
> - **Phase 5** — core safety fix done; Candidate-workflow UI deferred to
>   Phase 7/8. All 6 automatic `invalidate_authored_chart` call sites
>   identified in Phase 0 (`reanalyze_pitch`, `realign`, `reanalyze`
>   (transcript-only), `save_lyrics_and_realign`, `provide_lrc`,
>   `apply_timed_lyrics`) no longer delete the Authored Chart. `reanalyze
>   (full=true)` — "Reanalyze all" — now calls the new
>   `cache.rs::delete_analysis_outputs_keep_chart` instead of
>   `delete_song_cache`, so even a full reanalysis preserves the chart by
>   default (phase plan Phase 9 test: "Full Reanalysis 默认保留 Authored
>   Chart"). Each reset site was extracted into a small
>   `&CacheDir`-parameterized private function
>   (`apply_pitch_reanalysis_reset`, `apply_realign_reset`,
>   `apply_reanalyze_reset`, `apply_lyrics_edit_reset`) so it's testable
>   against an isolated temp cache dir instead of the real app data root --
>   7 new tests, all passing, each asserting the chart file survives while
>   everything else it used to wipe still gets wiped. The one place total
>   deletion is still correct and unchanged: `delete_cache` (the explicit,
>   two-step-confirmed "Delete cache" UI action) still calls
>   `delete_song_cache`, which still removes the chart -- that's a real,
>   confirmed user request, not an automatic side effect.
>
>   New explicit escape hatch: `chart.rs::replace_authored_chart_with_fresh_analysis`
>   (catalogued `destructive`) does what the old automatic behavior used to
>   do, for a user who explicitly wants to discard their edits -- callers
>   must gate it behind their own confirmation UI (not yet built; Phase
>   7/8).
>
>   **Not done in this pass:** the `ChartUpdatePolicy` enum
>   (`KeepAuthoredChart`/`CreateCandidate`/`ReplaceAfterConfirmation`,
>   phase plan §5.1), a real `candidate_chart` artifact distinct from the
>   transcript/pitch pair, staleness detection ("Pitch evidence updated
>   after the last chart edit"), and Compare/Merge UI are all still
>   unbuilt. What exists today after this phase is the simpler, safer
>   baseline those need to build on: new analysis output no longer
>   destroys the chart, but nothing yet tells the user new output exists to
>   look at. That surfacing is a Phase 7/8 UI concern.
> - **Phase 6** — done for what's actually built; deliberately not padded
>   with unimplemented entries. `API_CAPABILITIES` now carries every real
>   Phase 1/2/5 function (`get_analysis_graph`, `preview_analysis_plan`,
>   `load_analysis_artifacts`, `load_artifact_revisions`,
>   `set_active_artifact_revision`, `set_song_analysis_profile`,
>   `reset_song_analysis_profile`, `replace_authored_chart_with_fresh_analysis`),
>   correctly classified (`read`/`mutation`/`destructive`), verified by the
>   existing `api::tests::catalogue_has_unique_commands_and_known_access_classes`
>   contract test and `uta-studio-diagnostics`'s capability-count test
>   (both passing). **Not added:** catalogue entries for `run_analysis_plan`,
>   `retry_analysis_node`, `run_analysis_node_downstream`,
>   `cancel_analysis_run`, `load_analysis_run`, `load_analysis_node_attempts`,
>   `compare_analysis_runs`, `open_analysis_artifact`,
>   `reveal_analysis_artifact`, `invalidate_analysis_artifact` -- none of
>   these have a real backing function yet (no live-plan executor, no
>   per-attempt persistence writer, no OS-open/reveal wiring in `app-core`).
>   Cataloguing a command with nothing behind it would make
>   `API_CAPABILITIES` describe a command surface the app doesn't actually
>   have, which is the opposite of what phase plan §6 wants it for.
> - **Phase 7** — the DAG canvas rewrite (§7.1/§7.2) landed on top of four
>   earlier additive slices. The earlier slices were built cautiously,
>   without touching the hand-tuned `AnalysisGraphBox` coordinates, because
>   a wrong pixel-layout change is invisible in this sandbox and only
>   surfaces once the user actually looks at it. The user then explicitly
>   authorized proceeding with unverified UI changes anyway ("继续做，按照要求重构
>   全部完成后我会自己编译检查然后给你反馈" -- continue and refactor per the
>   requirements; they'll compile-check and give feedback themselves once
>   it's done), which is what unblocked the rewrite below. Every change
>   past that point compiles and its logic is unit-tested wherever the
>   logic is pure, but **none of it has been visually confirmed against a
>   running app** -- that verification is now explicitly the user's, not
>   deferred by default assumption.
>
>   **5. The DAG canvas rewrite itself**, replacing the entire hardcoded
>   layout with two new pure, fully unit-tested modules plus a rewritten
>   render call site:
>   - `desktop/src/studio/analysis_model.rs` -- `GraphViewModel`
>     (docs/analysis-dag-redesign.md §7.1: "AnalysisGraphSpec + AnalysisPlan
>     + AnalysisRun + ... -> GraphViewModel. UI 不再自行读取 cache 文件猜状态").
>     `build_graph_view_model` gives every one of the baseline graph's 12
>     real nodes (not just the old 7 buckets) a `GraphNodeState`
>     (`NotApplicable`/`Disabled`/`Blocked`/`Frozen`/`Waiting`/`Running`/
>     `Complete`) by blending Phase 1's `AnalysisPlan` (which wins outright
>     for the four plan-only states) with the *same* bucket-based run-time
>     completion signal the old UI already computed (`stage_complete`) --
>     this doesn't invent a new execution-state source, it just gives every
>     node a state instead of only 7 buckets. Compound nodes
>     (`music.analysis`'s `music.key`/`music.rhythm`/`music.descriptors`)
>     collapse to one box by default with a `collapsed_child_count`,
>     modeled and tested for §7.3's "Music Analysis 支持展开" even though the
>     click-to-expand interaction isn't wired yet (see gaps below).
>     `build_render_graph` then extends that with the virtual
>     artifact/export boxes §7.3's suggested structure calls for ("Vocal
>     Stem", "Export UTZ", ...) -- these were never real `AnalysisGraphSpec`
>     nodes (a node's `outputs: Vec<ArtifactKind>` is data, not a graph
>     node), so the old UI hand-placed them too; this makes that synthesis
>     one explicit, tested function instead of 14 hand-placed boxes.
>     Readiness for each virtual node comes from real on-disk artifact
>     presence (`cached_artifact_presence_for_song`), which is strictly more
>     accurate than the old code's progress-index-only heuristic -- e.g. the
>     "Timed lyrics" artifact is now fed by every lyrics route that's
>     actually in-universe (`lyrics.align`/`lyrics.import_timed`/
>     `lyrics.transcribe`), where the old hardcoded diagram only ever drew
>     the alignment edge. 17 tests.
>   - `desktop/src/studio/analysis_layout.rs` -- `layered_layout_from_edges`,
>     a longest-path layered auto-layout (§7.2: "分层拓扑布局...不再为每个节点硬编码绝对坐标")
>     over a flat node/edge list (generic, not tied to `AnalysisGraphSpec`,
>     since the rendered canvas includes the virtual nodes above). Ranks
>     nodes by longest path from a source using its own Kahn's-algorithm
>     topological sort, stacks same-rank nodes into rows, and returns a
>     rectangle per node plus the overall canvas size -- no crossing
>     minimization (no barycenter/median pass), which the phase plan's own
>     wording accepts for a first version. 7 tests, including that every
>     edge in the baseline graph points strictly left-to-right and same-rank
>     boxes never overlap vertically.
>   - `desktop/src/studio/analysis.rs`'s `spawn_analysis_session_overview`:
>     the ~290-line block that built 14 hardcoded `AnalysisGraphBox`
>     positions, 3 lane-background decorations, and manually bent
>     multi-segment edges is replaced by: build the view model, extend it to
>     a render graph, lay it out, then two loops (`render_graph.edges`,
>     `render_graph.nodes`) that spawn ports/paths/boxes from the computed
>     data. Canvas width/height are now `layout.canvas_width`/
>     `canvas_height` instead of the hardcoded `px(1930)`/`px(430)`. Two new
>     small bridge functions make this possible without touching the
>     existing box-rendering widgets' tested visual logic:
>     `bucket_stage_id` (exact inverse of `analysis_stage_index`, so every
>     compute box still dispatches the *same* `UiAction::SelectAnalysisStage`
>     bucket-string selection the inspector panel already uses -- see
>     "Known scoping choice" below) and `graph_node_state_to_stage_state`
>     (maps the 4 plan-only states onto the existing widget's `Waiting`
>     visual treatment but swaps in real status text -- "Blocked · a
>     required input is missing", "Frozen · reusing a protected artifact",
>     etc. -- in place of the node's normal route/model text, rather than
>     touching the widget's color logic at all). 4 more tests. The 3
>     decorative lane backgrounds (`spawn_analysis_graph_lane`) are dropped
>     entirely rather than kept and misplaced, since their hardcoded x/y no
>     longer corresponds to anything once positions are computed.
>
>   **Known scoping choice, stated plainly rather than glossed over**: the
>   node inspector (PLAN & ARTIFACTS panel, item 2 below) still keys
>   selection off the 7-bucket string, not individual `AnalysisNodeId`s --
>   clicking any of `music.analysis`/`music.key`/`music.rhythm`/
>   `music.descriptors` (all now genuinely separate boxes when expanded, and
>   `music.analysis` always shows a "3 sub-checks not shown" note when
>   collapsed) opens the *same* bucket-0 detail panel, same as when only one
>   "Prepare" box existed. A full migration to per-node inspector selection
>   would also need to change how route/implementation/model/device info is
>   looked up (`AnalysisStageRoute` in the live-progress wire protocol is
>   still keyed by `stage: String`, not `node_id` -- that's backend surgery
>   Phase 3 explicitly deferred, not something to also do blind in this
>   pass). Kept this way on purpose: it reuses 100% of the existing,
>   working selection/inspector/route-lookup code path unchanged, isolating
>   this pass's real risk to *where boxes are and what state they show*,
>   not *what happens when you click one*.
>
>   **Still not done**: a click-to-expand/collapse interaction for compound
>   nodes (the data model and tests exist; no UI wiring); pan/zoom/fit/focus
>   beyond the existing horizontal drag-scroll; a mini-map; the file split
>   into `analysis/{mod,graph_view,graph_layout,graph_model,inspector,
>   actions,history,plan_preview}.rs` the phase plan suggests (this session
>   used `analysis.rs` + `analysis_model.rs` + `analysis_layout.rs` instead
>   -- three files separating pure/testable logic from Bevy rendering, not
>   the full eight-file split, to limit the number of new seams in one
>   unverified pass); a dedicated Plan Preview panel for a *hypothetical*
>   target/route/disabled-node combination (today's PLAN & ARTIFACTS panel
>   only ever previews the default full run); the 11-item node context menu
>   and 9-item artifact context menu §7.5/§7.6 list in full (this pass has
>   Sync/Set active/Reveal/Delete, not Retry/Run downstream/Configure/
>   Freeze/Disable/Bypass/Compare/Pin/Preview/Play audio).
>
>   **Update -- this sandbox turned out to have a real display and real
>   audio after all, and this got visually verified for real.** The
>   "no way to see a running GUI" assumption above was wrong: this
>   environment runs a full COSMIC/Wayland desktop session with a real
>   Intel Arc GPU and PipeWire audio (confirmed by the user, who corrected
>   this assumption directly). `cosmic-screenshot` takes real screenshots;
>   there's no Wayland input-synthesis tool available (`xdotool` only sees
>   Xwayland windows, and this app runs Wayland-native), so instead of
>   trying to fake clicks, `StudioSession::load()` gained a small dev-only
>   `with_debug_navigation` hook read from three env vars
>   (`UTA_STUDIO_DEBUG_OPEN_SONG=<file_hash>`,
>   `UTA_STUDIO_DEBUG_OPEN_ACTIVITY=1`, `UTA_STUDIO_DEBUG_OPEN_HISTORY=<id>`)
>   that jump straight to Song Detail or the Analysis Queue's DAG canvas on
>   startup -- inert unless explicitly set, so it can never affect a real
>   user's session. Launched the real built binary
>   (`cargo build -p uta-studio-desktop`) against the user's own real,
>   already-analyzed library (`/home/bintis/Documents/uta-studio`, per the
>   user's own offer -- "models and Python env are already set up there"),
>   read-only: browsed, opened Song Detail, opened the Analysis Queue's
>   history view for a real completed run, took screenshots, made no
>   destructive clicks.
>
>   **What this caught: a critical, real bug no unit test had found.**
>   The first DAG canvas screenshot showed every compute node (Preflight,
>   Music Analysis, Stem Separation, Pitch Extraction, ...) stacked in a
>   single leftmost column instead of laid out left-to-right by dependency
>   -- and the node-inspector text overlapping the bottom rows of the
>   canvas. Root cause: `build_render_graph` only ever added the *virtual*
>   artifact/export edges (`stems.separate -> artifact.vocal_stem`, etc.);
>   it never copied over the real compute-node dependency edges from
>   `AnalysisGraphSpec` (`preflight -> stems.separate`,
>   `stems.separate -> pitch.extract`, ...). With no edges between compute
>   nodes, `layered_layout_from_edges`'s rank computation -- itself correct
>   and already covered by 7 passing tests -- correctly ranked every
>   edgeless node into column 0, stacked as rows; the algorithm did exactly
>   what it was told with an incomplete graph. Every existing test passed
>   throughout, because nothing asserted that the real graph edges survived
>   into the render graph -- only that referenced nodes existed and virtual
>   artifact edges attached to the right upstream. This is precisely the
>   class of bug this session kept citing as the reason to avoid blind UI
>   work, now caught the only way it could be: by actually looking.
>
>   Fixed: `build_render_graph` now takes `&AnalysisGraphSpec` and seeds
>   `edges` from `graph.edges` (filtered to endpoints present in the view,
>   so a collapsed compound child's edge to its parent is correctly
>   dropped), before adding the virtual artifact/export edges on top. Added
>   a new regression test,
>   `every_real_compute_edge_from_the_graph_spec_survives_into_the_render_graph`,
>   asserting every real graph edge (with both endpoints in view) is
>   present in the render graph -- the exact assertion the original test
>   suite was missing. Rebuilt, relaunched, re-screenshotted: the canvas
>   now lays out correctly left-to-right by rank (Preflight/Music Analysis
>   at column 0; Stem Separation, Timed Lyrics Import at column 1; Pitch
>   Extraction, Lyrics Preprocessing at column 2; and so on through Build
>   Candidate Chart), every node's state/percentage/model-algorithm subtext
>   renders correctly (e.g. "WhisperX · large-v3-turbo", "RMVPE · RMVPE
>   singing"), the `NotApplicable` override text renders correctly ("Timed
>   Lyrics Import ... Not applicable to this run's ..." for a Whisper-route
>   song), and the earlier text-overlap under the canvas is gone.
>
>   **A second real bug found and fixed the same way, on the Song Detail
>   Overview panel:** the real test song's "Vocal / instrumental stems" row
>   showed "Pending" despite the song being fully separated and playable.
>   Its stem files exist on disk as `{hash}_vocals_Dm_1.0.flac` /
>   `{hash}_instrumental_Dm_1.0.flac` -- the legacy naming
>   `pipeline.py`'s own `_find_legacy_stem_cache` docstring describes as
>   "from before separation was decoupled from detected key/tempo" --
>   never the bare `{hash}_vocals.flac` `cached_artifact_presence` checked
>   for via `CacheDir::vocals_path`/`instrumental_path`. `cache.rs` already
>   had the right answer sitting unused: `has_variant_stems` (backing the
>   existing, pre-this-session `transcript_exists` check) already recognizes
>   the legacy/variant suffix. `cached_artifact_presence` now ORs the bare
>   check with `has_variant_stems` for both `VocalStem`/`InstrumentalStem`.
>   New test:
>   `cached_artifact_presence_recognizes_legacy_key_tempo_suffixed_stems`.
>   Confirmed fixed via a second screenshot: the row now reads "Both
>   available." This is also a real, direct hit against Phase 9 §9.1's own
>   "旧 Stem cache 继续复用" (old stem cache continues to be reused) legacy
>   migration acceptance item -- not hypothetical, caught against the
>   user's actual pre-existing library.
>
>   3 new tests this pass
>   (`every_real_compute_edge_from_the_graph_spec_survives_into_the_render_graph`,
>   `cached_artifact_presence_recognizes_legacy_key_tempo_suffixed_stems`,
>   plus the `build_render_graph` signature change updating 7 existing call
>   sites), all passing; `uta-studio-core` 230 tests, `uta-studio-desktop`
>   76 tests, full suites still green. The `with_debug_navigation` hook and
>   the `MonitorSelection::Primary` fullscreen branch for debug launches
>   are left in the codebase (not reverted) -- real, low-risk, dev-only
>   tooling with clear value for whoever iterates on this UI next, exactly
>   what the user asked to have added ("至于app的操作 你可以加内部API CLI操作就好了").
>
>   Earlier additive slices, unchanged by the rewrite above:
>   1. `analysis_node_stage_index` (maps a Phase 1 `AnalysisNodeId` onto the
>      existing 7-bucket stage index) and `resolve_live_stage_index`
>      (prefers a live snapshot's `node_id` when Phase 3's `progress_node`
>      set one, falling back to the old `analysis_stage_index(stage)` text
>      classification otherwise). The one call site that derives the UI's
>      `stage_index` from a live snapshot now goes through this resolver.
>   2. A real node inspector: `stage_primary_node_and_artifact` maps each of
>      the 7 stage buckets to one representative `AnalysisNodeId` and,
>      where a real cached file exists to check, an `ArtifactKind`. The
>      selected-stage detail panel (the one `UiAction::SelectAnalysisStage`
>      already opens -- this is additive to it, not a new panel) gained a
>      "PLAN & ARTIFACTS" section that calls the new
>      `app_core::preview_full_analysis_plan(file_hash)` (a first production
>      call site for Phase 1's `preview_analysis_plan`, grounded in the
>      song's real saved `AnalysisProfileSnapshot` via
>      `get_song_analysis_profile`) and Phase 7's own
>      `cached_artifact_presence_for_song`, and renders the selected node's
>      real `NodeState`, whether it will actually run this pass, its plan
>      warning/reason if any, and whether its artifact is actually present
>      on disk -- replacing nothing, only adding ground-truth next to the
>      existing static per-stage copy.
>
>   3. A real artifact context menu, in the same "PLAN & ARTIFACTS" section.
>      A "Sync from disk" button dispatches the new
>      `UiAction::SyncArtifactRevisions(file_hash)`, which calls Phase 2's
>      already-built-but-never-wired `import_legacy_artifacts` -- an honest
>      gap this closes: before this, `load_analysis_artifacts` always
>      returned an empty list for every real song because nothing ever
>      called the writer that populates the `analysis_artifacts` table.
>      Deliberately kept as an explicit, user-triggered action rather than
>      run on every render: it hashes file contents
>      (`hash_file_contents`/blake3), which is not something to do on every
>      UI rebuild. Once synced, each revision for the selected stage's
>      artifact kind is listed (active/inactive marker, filename) via a
>      plain `load_artifact_revisions` SQL read (cheap enough to call every
>      render, same as the existing `cached_artifact_presence_for_song`
>      call this session's earlier slice already established as an
>      accepted pattern), with a "Set active" button
>      (`UiAction::SetActiveArtifactRevision`, calls
>      `set_active_artifact_revision` directly -- non-destructive) and a
>      "Delete" button. Delete is confirmation-gated
>      (`RequestDeleteArtifactRevision` / `CancelDeleteArtifactRevision` /
>      `ConfirmDeleteArtifactRevision` + a `pending_artifact_delete` session
>      field), mirroring the existing `RequestDeleteSongCache` pattern
>      exactly -- including wiring into `navigation_back_action`'s Escape
>      priority chain and a confirmation modal
>      (`spawn_artifact_delete_confirmation`, modeled directly on
>      `spawn_cache_delete_confirmation`) layered above the activity center
>      overlay (`ZIndex(110)` vs. its `100`, since this action is always
>      triggered from inside that panel). `import_legacy_artifacts` gained
>      an `API_CAPABILITIES` entry now that it has a real caller.
>   4. A "Reveal" button per revision, opening its containing folder in the
>      OS file manager. This could **not** reuse the existing
>      `open_library_entry`/`reveal_library_entry`/`validate_source_path`
>      trio: `validate_source_path` only authorizes paths under
>      `config.library_paths()` (the user's watched folders) or the export
>      path -- an artifact revision's path is always under the app's own
>      cache root, which isn't in that list, so reusing it would make every
>      reveal fail with a false "not authorized" error. Added a parallel
>      `validate_cache_path(path, cache_root)` / `reveal_artifact_entry`
>      pair in `desktop/src/studio/library.rs`, same
>      canonicalize-and-`starts_with` shape, scoped to
>      `CacheDir::new().path` instead. 4 new tests, including a regression
>      guard for the `starts_with` string-prefix trap (`/cache-evil` sharing
>      a text prefix with `/cache` without actually being inside it --
>      confirmed Rust's `Path::starts_with` already compares components, not
>      raw strings, but the guard stays as a real regression test rather
>      than a comment asserting it).
>
>   17 tests for items 1-4 above (3 in `app-core` for
>   `preview_full_analysis_plan`, 4 for the node-id/artifact mapping, 4 for
>   `validate_cache_path`, plus the 2 pre-existing bridge tests already
>   counted) plus 28 more for item 5's rewrite (17 in `analysis_model.rs`, 7
>   in `analysis_layout.rs`, 4 bridge tests in `analysis.rs`) -- 45 new tests
>   this phase in total, all passing. The Bevy spawn/dispatch code itself
>   (button wiring, the two render loops) is exercised by compilation and
>   the existing widget/action precedent, not independently unit-tested --
>   every function with real branching logic underneath it is. Full suites
>   green (`uta-studio-core` 227 tests, `uta-studio-desktop` 75 tests).
>   `cargo check --workspace` and `cargo fmt` on every touched file also
>   pass. `analysis_stage_matches`, `analysis_stage_details`, and
>   `analysis_stage_index` (via `resolve_live_stage_index`, still computing
>   the live snapshot's current bucket) are all unchanged and still live in
>   the render path -- the scoping choice above keeps the inspector on the
>   bucket-string system deliberately, not as leftover debt.
>   Beyond item 5's scope, a dedicated Plan Preview panel (previewing a
>   *hypothetical* target/route/disabled-node combination before committing
>   to Analyze/Reanalyze, rather than always previewing today's default
>   full run) remains unbuilt, as does the
>   `analysis_runs`/`analysis_node_attempts` live writer that would make
>   "Sync from disk" unnecessary for songs analyzed after this phase lands.
> - **Phase 8** — §8.1 (primary CTA) and §8.5 (BPM/Speed naming) landed for
>   real, on top of the backend groundwork from before; §8.2's page-section
>   restructure and §8.3's full control migration remain not started, and
>   §8.4/§8.6 are partially covered by work from other phases (see below) but
>   not built as their own feature. As with Phase 7's canvas rewrite, this
>   work is unverified against a running app -- same explicit user
>   authorization, same open item for the user to check.
>   **Update (later session):** §8.2 and §8.4 are now both real and done --
>   see `docs/plan.md`'s Phase 8 section for the full record (6 independent
>   named section cards; song profile now actually affects real execution
>   instead of being preview-only decoration, plus a real Run-tier override
>   backing §8.4's three-tier display). §8.3's control migration remains not
>   started; see `docs/plan.md` for the current status of everything else in
>   this phase.
>
>   **§8.1 -- one real primary CTA.** `song_detail.rs`'s
>   `spawn_song_primary_actions` used to run its own inline
>   analyzed/authoring_ready/editor_ready chain and could show up to three
>   buttons at once (export UTZ + export UltraStar + edit chart) when a song
>   was authoring-ready. It now matches on the real
>   `SongAuthoringState` from `resolve_song_authoring_state(file_hash)`
>   (built in an earlier pass of this phase, previously uncalled from any
>   UI) and renders exactly one button per state -- `RetryFailedNode` gets
>   its own "Retry failed analysis" copy distinct from a first-time
>   "Analyze song" (the old chain conflated a failed run with never having
>   analyzed at all, since both left `is_analyzed` false). The two export
>   buttons weren't dropped -- they moved to the secondary action row next
>   to "Play original" and "Settings", matching §8.1's own primary/secondary
>   split ("次级操作: Play original / Export / Song metadata / More") instead
>   of competing with the highlighted primary action. No new button widget
>   was invented for visual "highlighting" (this codebase has no
>   accent-colored CTA style to reuse, and inventing one is exactly the kind
>   of pixel judgment call that needs eyes on a running app) -- going from
>   "up to three buttons" to "exactly one" is what actually does the
>   highlighting here.
>
>   **§8.5 -- BPM vs. Speed naming.** Renamed every place the render-speed
>   multiplier (`song.tempo`, 0.5x-2.0x, used only by `shift_tempo`'s
>   export-variant renderer) was labeled ambiguously as "Tempo": the Song
>   Detail header's inline metadata badge ("1.0× tempo" ->
>   "1.0× playback speed"), the stepper row title ("Tempo" -> "Playback /
>   export speed"), the disabled-state fallback row ("Key & tempo" ->
>   "Key transpose & playback speed"), and the Full Reanalysis description
>   ("...key, tempo, and pitch assets" -> "...detected key, musical BPM,
>   and pitch assets" -- this one was actually describing the *real*
>   detected-BPM re-analysis, not the speed-render control, so the old
>   copy was doubly wrong). The key-shift stepper is now labeled
>   "Key transpose" (was "Key") with its description reading
>   "Detected key: X" (was "Original key: X", which read oddly once
>   "Detected Key" became this feature's actual name per the plan's
>   glossary). `song_settings.rs`'s Musical BPM field (was "BPM") and its
>   music-analysis summary line now both say "Musical BPM" consistently --
>   that summary used to say "BPM {value}" on one branch and "Tempo
>   unavailable" on the other for the *same* underlying value, which was
>   exactly the "含混的同一文案" (ambiguous shared copy) §8.5 calls out, just
>   inside one string instead of across two features.
>
>   **Not done:** §8.2's Overview/Analysis/Lyrics & Timing/Audio & Pitch/
>   Authoring & Export/Artifacts & History page sections and §8.3's control
>   migration into them -- Song Detail's existing Realign/Force
>   Transcribe/Analyze Pitch/Full Reanalysis buttons and layout are
>   otherwise unchanged (they already call the correctly-scoped, chart-safe
>   app-core functions from Phase 5, so leaving them in place doesn't
>   regress anything, it just means the page isn't reorganized yet).
>
>   **§8.2/§8.3, partial -- real, but a deliberately bounded slice.** Song
>   Detail's single "Production controls" column now carries 4 sub-section
>   headings (`spawn_song_detail_subheading`, reusing the existing eyebrow
>   text style rather than a new widget) grouping its rows under AUDIO &
>   PITCH / LYRICS & TIMING / ANALYSIS / ARTIFACTS & HISTORY, and rows were
>   *reordered* (not just relabeled) so "Analyze pitch" now sits with
>   Key/Speed under Audio & Pitch instead of trailing after the Lyrics
>   group. Every condition, action, and copy string is unchanged --
>   `analyzed_and_native`/`native_source` are the same two guard
>   expressions that existed inline before, just named once and reused.
>   Deliberately **not done**: actually moving these controls out to DAG
>   canvas node context menus, which is what §8.3's migration table
>   literally asks for (Realign -> Lyrics Alignment Node -> Rerun, Analyze
>   pitch -> Pitch Node -> Rerun, Delete cache -> Artifacts -> Select
>   Revisions -> Invalidate/Delete, etc.). That context menu doesn't exist
>   yet -- Phase 7's rewrite explicitly scoped it out -- so actually
>   removing these buttons from Song Detail would leave no way to trigger
>   them at all. Grouping now, relocating once the destination exists, was
>   judged the safer order.
>
>   **Update (later session): §8.2 itself is now the real 6-card reorg, not
>   this bounded 4-subheading slice.** `spawn_song_detail_subheading` no
>   longer exists -- every section (including the new 6th, "Authoring &
>   Export", carrying the Export UTZ/UltraStar buttons moved out of the page
>   header) is its own independently bordered card via a new
>   `spawn_song_detail_section_card` helper. §8.3's actual control migration
>   into DAG node context menus is still not done -- see `docs/plan.md`.
>
>   The Overview column (`song_overview_rows`, renamed from "PRODUCTION
>   OVERVIEW" to "OVERVIEW") gained real rows for several §8.2 Overview
>   items that were previously just missing from the page: Active analysis
>   profile (Song override vs. Global defaults, via
>   `get_song_analysis_profile`), Detected key + confidence, Musical BPM +
>   confidence + beat count (via `load_music_analysis` -- a second real
>   production call site for a function that already existed but was only
>   read from the editor and Song Settings before), and Vocal/Instrumental
>   stem + Pitch evidence availability (via Phase 7's
>   `cached_artifact_presence_for_song`, replacing a coarser "Stems:
>   Separated/Pending" row with per-stem status). **Not built**: a chart
>   issue count -- that needs the full `ChartDocument`'s `ChartProblem`
>   list, which only exists once the chart is loaded into the editor;
>   loading and parsing it on every Song Detail render to populate one
>   summary row was judged not worth the cost, and is noted as a doc
>   comment on `song_overview_rows` itself, not just here. "Last successful
>   run" (needs `analysis_runs` history, still Phase 3's deferred DB
>   writer) is also not shown.
>
>   **§8.4 (three-tier parameter inheritance display) and §8.6 (model
>   availability -> Blocked wiring): deliberately declined, not just
>   deferred by omission.** §8.4 wants Inherited/Song override/Run
>   override/Fallback labels next to every parameter, but there is no real
>   "Run override" storage today -- `AnalysisRequest.profile_snapshot` is
>   constructed fresh per plan preview, never persisted -- so a real
>   three-tier display would only ever show two of its four states, which
>   is worse than the honest two-state summary already added to Overview
>   ("Active analysis profile: Song override / Global defaults"). §8.6's
>   missing-model `Blocked` state needs a real mapping from
>   `vendor::model_install_statuses()`'s `ModelDownloadTarget`s (which
>   entries even *appear* in that list is itself conditional on the
>   currently configured separator/ASR engine/backend) onto specific
>   `AnalysisNodeId`s. Unlike Phase 7's canvas rewrite, getting this wrong
>   isn't a silent visual bug -- a bad mapping could mark a node `Blocked`
>   when the song's actual configured path doesn't need that model at all,
>   which would stop a real analysis run that should have worked. That's a
>   functional-correctness risk, not a pixel-layout risk, and this session
>   judged it unsafe to build without being able to run it against the real
>   `vendor` status logic. `preview_full_analysis_plan` still passes an
>   empty `model_availability` map (Phase 1's documented "assume everything
>   installed" default) -- unchanged, on purpose.
>
>   **Update (later session): §8.4's "declined" reasoning above no longer
>   holds -- Run override now has real storage, and the display is built.**
>   The blocker this paragraph describes (no real "Run override" storage, so
>   a three-tier display could only ever show two states) is fixed: song
>   profile itself turned out to be decorative too (real runs read global
>   `AppConfig` directly, never the saved profile), so both the second and
>   third tier needed real wiring, not just the third. See `docs/plan.md`'s
>   Phase 8 §8.4 entry for the full record -- `resolve_profile_field`,
>   `configure_analysis_node_for_run`/`save_node_config_as_song_profile`,
>   and the Node Inspector's PARAMETER SOURCE fact now genuinely showing
>   "Run override (queued)"/"Song profile"/"Global default".
>
>   4 tests remain from the earlier `authoring_state_from_signals` pass; no
>   new tests this pass -- §8.1/§8.2/§8.3/§8.5 are Bevy button-wiring,
>   reordering, and string/data-row changes with no new branching logic to
>   unit test independently of already-tested functions
>   (`SongAuthoringState`, `load_music_analysis`,
>   `cached_artifact_presence_for_song`) they call or dispatch on. Full
>   suites still green after this pass (`uta-studio-core` 227 tests,
>   `uta-studio-desktop` 75 tests), `cargo check --workspace` and
>   `cargo fmt` clean.
> - **Phase 9** — not run as a full pass (its own scope is a release
>   checklist, not new code), but every check that's meaningful against
>   this session's actual changes has been run repeatedly and passed:
>   `cargo test -p uta-studio-core --lib` (227 tests), `cargo test -p
>   uta-studio-desktop` (47 tests), `cargo check --workspace`, `cargo fmt`
>   on every touched file, `python -m compileall app-core/analyzer`. The
>   project-name scan turned out to *not* need a running app either -- see
>   the dedicated note further down, where it was actually run and came
>   back clean. Still not run: real audio decode/playback, real UTZ/
>   UltraStar smoke export, PipeWire/xrun inspection -- none of these are
>   reachable without a running desktop session and real audio hardware,
>   which this sandbox doesn't have. `nix build path:.#uta-studio` was
>   attempted twice and fails in its `checkPhase`, but **not because of
>   anything in this session's diff**. Root-caused via `git blame`: 9
>   `desktop::studio::tests` failures (`taps_retime_the_queued_notes_in_order_then_stop`,
>   `ghost_notes_show_the_other_tracks_and_never_the_active_one`,
>   `pitch_audition_sounds_only_the_notes_in_range`, and 6 others, all
>   editor/audition tests untouched by this session) all panic at
>   `app-core/src/cache.rs:29` (`could not create cache directory: Permission
>   denied`) because `desktop/src/studio/editor/state.rs:804`'s
>   `load_editor_beats` calls `app_core::CacheDir::new()` unqualified, which
>   resolves to `$HOME/.uta-studio/cache`. That line was authored in commit
>   `fab3f64` ("release: 0.3.0"), the commit *before* this session started —
>   Nix's sandboxed `checkPhase` sets `HOME` to a deliberately non-writable
>   placeholder specifically to catch tests that touch real user paths
>   instead of an isolated fixture (the same hazard this session's own
>   Phase 2/5/7/8 tests were careful to avoid via injectable `CacheDir {
>   path: tmp }`). Outside Nix's sandbox this doesn't crash — it silently
>   creates/reads the real `~/.uta-studio/cache` on whatever machine runs
>   `cargo test`, which is the quieter version of the same problem (and
>   confirmed here: this session's own non-sandboxed `cargo test -p
>   uta-studio-desktop` run passed all 9 of those tests cleanly, silently
>   touching the real cache dir in the process). Left unfixed: it's
>   pre-existing, outside the DAG-redesign scope, and touches
>   `desktop/src/studio/editor/`, files with active uncommitted changes from
>   concurrent work this session did not make. Flagged for the user rather
>   than silently patched.
>
>   **§9.1/§9.2 systematic audit.** The engineering-checklist items above
>   are Phase 9's §9.5; the plan's §9.1 (legacy migration) and §9.2 (a
>   per-subsystem acceptance matrix) are a different kind of checklist --
>   most of their individual bullets are actually claims about *behavior*,
>   and several of those claims are exactly what this session's unit tests
>   already assert, or can be made to. Went through every bullet in §9.1
>   and §9.2 against the real test suite rather than declaring the whole
>   phase blocked on "needs a running app":
>
>   - **Already proven by an existing test, cross-referenced here so the
>     mapping is explicit and doesn't rot:** "Freeze Stems 后可重跑 Pitch" ->
>     `frozen_artifact_satisfies_downstream_input_without_rerunning_upstream`.
>     "Key/BPM 变化不失效 Stem" ->
>     `stem_signature_excludes_key_and_bpm_by_construction`. "只重跑 Pitch
>     不触发 Transcription" -> `pitch_only_target_skips_transcription_but_not_separation`.
>     "只重跑 Pitch 不删除 Authored Chart" ->
>     `pitch_reanalysis_reset_preserves_the_authored_chart`. "Timed LRC 不显示
>     ASR" / "Parakeet 路径不显示额外 Alignment" / "Whisper 路径显示 ASR →
>     Alignment" / "Known Lyrics 路径直接进入 Alignment" -> the four
>     `LyricsRoute` tests (`timed_lrc_route_excludes_asr_node`,
>     `parakeet_route_excludes_alignment_node`,
>     `whisper_route_generates_asr_and_alignment`,
>     `known_lyrics_route_goes_directly_to_alignment`). "新 Timed Transcript
>     不删除 Authored Chart" -> `realign_reset_preserves_the_authored_chart` +
>     `lyrics_edit_reset_preserves_the_authored_chart`. "Song Profile 只影响指定歌曲"
>     -> `song_profile_only_affects_the_named_song`. "旧 History 使用 Legacy
>     Adapter" (old `analysis_history.snapshot_json` rows, pre-Phase-3
>     shape) -> `old_history_snapshot_json_without_node_fields_still_deserializes`.
>     "旧 Stem cache 继续复用" / "不自动重新分析" ->
>     `legacy_import_creates_revisions_without_modifying_files` +
>     `legacy_import_is_idempotent` (import never triggers analysis, never
>     writes to the files it reads).
>   - **New tests added this pass to close real assertion gaps** (the
>     underlying behavior was already correct; nothing backed it with a
>     named, permanent test): "单独重跑不会访问 Stems/Transcript/Pitch" for
>     Music Analysis -> `music_analysis_only_target_never_pulls_in_stems_pitch_or_lyrics`
>     (`analysis_plan.rs`, the mirror of the existing
>     `target_node_automatically_pulls_in_required_upstream`, which only
>     checked the other direction). "Delete Revision 不删除源媒体" ->
>     `delete_rejects_a_path_outside_the_cache_root_and_leaves_it_on_disk`
>     (`analysis_artifact.rs`) -- `delete_artifact_revision` calls the same
>     `ensure_within_root` guard `set_active_artifact_revision` already had
>     a dedicated test for, but delete's own path had none until now.
>   - **A real, pre-existing gap this audit found, not fixed.**
>     "失败时保留旧 Pitch" (a failed pitch rerun must preserve the old pitch
>     data) does **not** hold today: `reanalyze_pitch` calls
>     `apply_pitch_reanalysis_reset` (which deletes the old
>     `pitch_track`/`pitch_notes` files) immediately when the user clicks
>     "Analyze pitch," *before* `enqueue_one` even starts the actual rerun
>     -- not after a confirmed success. This predates this session (the
>     eager-delete-then-enqueue shape was already there; this session only
>     extracted the deletion into the now-shared `apply_pitch_reanalysis_reset`
>     helper without changing when it runs). Fixing it properly means
>     moving the reset from trigger-time in `reanalyze_pitch` to
>     completion-time in `process_song`'s success handling -- a change to
>     the live analyzer worker's completion path, which is exactly the kind
>     of thing Phase 3's status note already flagged as too risky to do
>     blind in this sandbox (no working `torch`+`numpy` environment to run
>     a real pipeline against). Left as-is and reported rather than
>     papered over.
>   - **Trivially true by construction, not worth a dedicated test.**
>     "Export 使用 Authored Chart，而不是未确认 Candidate" -- there is no
>     separate Candidate file today (Phase 5 explicitly deferred that),
>     so every export necessarily reads `vocal_chart.json` (the Authored
>     Chart); there is nothing else it could read. "Vocal 和 Instrumental
>     始终来自同一 Revision 组" is moot the same way `analysis_runs`/
>     `analysis_node_attempts` are moot elsewhere in this doc: nothing
>     calls `record_artifact_revision` from the live pipeline yet, so no
>     revision group exists to be inconsistent.
>   - **Genuine, already-documented gaps this audit re-confirmed rather
>     than newly discovered** (each already has its own note elsewhere in
>     this document): Candidate/Authored Chart independence, "默认重跑只生成
>     Candidate," Replace confirmation UI (§5's "not done in this pass"
>     list). Enqueue-time config freeze beyond node targeting (§4's "not
>     done" list). Run Override storage, History's persisted Profile
>     Snapshot, Compare Run (§8's declined-§8.4 note and §6's "not added"
>     list -- nothing persists a profile snapshot into a live
>     `analysis_history` row yet, confirmed by grep: `profile_snapshot`
>     only appears in `AnalysisPlan`/preview code, never in
>     `finish_analysis_history`). §9.6's model-availability -> Blocked
>     wiring (§8.6's declined note above).
>   - **Update: "needs eyes on a running Bevy window" and "needs to
>     listen/click" both turned out to be partially wrong.** See the
>     live-app verification note under Phase 7 above (real screenshots
>     against the user's real library, which is how two real rendering bugs
>     got caught and fixed). Beyond the GUI, three more §9.2/§9.5 items got
>     real, non-hypothetical verification by calling the same production
>     code the GUI calls, directly, from small scratch examples (real code,
>     real data, no GUI needed for any of these three):
>     - **Real UTZ/UltraStar smoke export** (`app-core/examples/export_smoke_test.rs`,
>       `cargo run -p uta-studio-core --example export_smoke_test -- <hash> <dir>`):
>       called `app_core::export_utz`/`export_ultrastar` against the same
>       real analyzed song, to a scratch directory (never the user's
>       configured export folder). UTZ: a real 66,492,817-byte zip,
>       `testzip()` clean, containing real `audio/guide-vocals.flac`
>       (24,248,402 bytes), `audio/instrumental.flac` (42,116,601 bytes),
>       `charts/vocal.json`, `analysis/pitch-evidence.json`, and
>       `manifest.json`. UltraStar: a real 8,673-byte `.txt` with correct
>       `#TITLE`/`#ARTIST`/`#BPM`/`#GAP`/audio-reference headers and real
>       timed Japanese lyric/note lines, and `app_core::validate_ultrastar_chart`
>       passed against it.
>     - **Real audio decode/playback** (`native-audio/examples/playback_smoke_test.rs`,
>       `cargo run -p uta-studio-audio --example playback_smoke_test -- <path> <secs>`):
>       loaded and played the real song's actual source WAV through the
>       same `EditorAudioPlayer` the desktop app's editor and library
>       playback both use, independent of Bevy/the GUI entirely. Two
>       separate runs, both clean: `duration_secs: 354.88` matched the
>       song's real 5:55 length, `position_secs` advanced within ~25ms of
>       wall-clock time every second across 4-6 consecutive samples (a
>       stalling or glitching decoder would show position lagging behind
>       wall-clock, not tracking it this tightly), `error: None`
>       throughout, clean `stop()`. This is real evidence of correct,
>       real-time playback -- about as close to "a human listened" as
>       automated verification gets without an actual ear.
>     - **PipeWire inspection**: `pw-top -b -n 2` during a run showed one
>       active real-time driver stream at 48kHz with a live `WAIT`/`BUSY`
>       timing readout, alongside PipeWire's other already-running system
>       streams (HDMI/USB audio devices) -- confirms the player is a real
>       PipeWire client, not a stub. Its `ERR` (xrun) counter read 1, but
>       that counter is cumulative across the whole PipeWire session, not
>       scoped to this test's own playback window, so it cannot honestly be
>       attributed to this run specifically -- reported as observed, not
>       inflated into "zero xruns confirmed."
>
>     Both example files are kept in the tree (not deleted after use) as
>     real, low-risk, `cargo run --example`-gated dev tooling -- exactly
>     what the user asked to have added ("至于app的操作 你可以加内部API CLI操作就好了"),
>     and directly reusable by anyone iterating on export or audio code
>     next, without needing the GUI or a fresh screenshot loop.
>
>     **Input synthesis: actually attempted, not assumed away.** Fetched
>     `ydotool`/`ydotoold` via `nix run nixpkgs#ydotool`. `/dev/uinput` has
>     an explicit ACL entry granting the sandbox user rw access, so it
>     looked viable -- `ydotoold` does start as a live process, but never
>     creates its virtual input device or opens its control socket (`unable
>     to find device pointer:ydotoold virtual device`, confirmed across two
>     separate attempts, one left running long enough to rule out a startup
>     race). This is a sandbox/namespace-level restriction on the
>     `UI_DEV_CREATE` ioctl, not a file-permission gap -- a real, tested
>     finding, not a repeat of the earlier wrong assumption about
>     screenshots. `xdotool` remains X11-only and this app is
>     Wayland-native, so it was never a candidate either.
>
>     Worked around this the same way as navigation: two more env vars on
>     `with_debug_navigation` (`UTA_STUDIO_DEBUG_SELECT_STAGE=<stage>`,
>     `UTA_STUDIO_DEBUG_SCROLL_OFFSET=<px>`) set the same session state a
>     node click or canvas drag would produce, then screenshotted the
>     result. Selecting `alignment` (a non-default node) and a 900px scroll
>     offset together showed: the canvas correctly scrolls without visual
>     corruption (Preflight's box correctly clips at the left viewport
>     edge, everything else renders in place), and the inspector panel
>     correctly switches to the selected node's real data ("STEP 06 · ALIGN
>     ... Word timing alignment").
>
>     **This also surfaced a real, pre-existing inconsistency, not
>     introduced by this session but now more visible because of it**: the
>     "Forced Alignment" box on the canvas (driven by this session's new
>     `GraphViewModel`/real bucket-completion state) read "COMPLETE 100%,"
>     while the inspector panel immediately below it for the *same* node
>     read "COMPLETE · 67%" -- because the inspector's percentage still
>     comes from the old `stage_routes` per-route lookup (the "Known
>     scoping choice" earlier in this document: deliberately left
>     untouched, since migrating it needs the `AnalysisStageRoute` wire
>     protocol to carry `node_id`, which Phase 3 explicitly deferred). Not
>     fixed here -- fixing it means either backfilling that wire protocol
>     field or reworking how `selected_progress` is computed, both larger
>     than a drive-by patch during a verification pass -- but recorded
>     precisely rather than left for someone else to rediscover from
>     scratch.
>
>     **Narrow-window: also actually attempted.** Added a
>     `UTA_STUDIO_DEBUG_WINDOW_SIZE=WIDTHxHEIGHT` debug var forcing windowed
>     mode at an explicit size (520x900 requested), since there's still no
>     way to interactively drag-resize a live window here. COSMIC's own
>     tiling window manager overrode the requested size to ~1300px wide
>     regardless -- a tiling WM auto-manages window geometry on its own
>     workspace layout, which an app's own requested `Window.resolution`
>     doesn't override, and this sandbox has no window-manager control
>     surface to fight that with either. At the ~1300px width it actually
>     got, both Song Detail (confirmed earlier in this session, in the very
>     first debug screenshot before the fullscreen branch was added: the
>     Overview column correctly wrapped below Production controls at
>     ~730px with no overlap) and the DAG canvas (correctly horizontally
>     scrolled/clipped at the viewport edge, "Editable chart" and the
>     export boxes cut off cleanly at the right edge, no distortion)
>     degrade cleanly. Not as narrow as the phase plan's spirit probably
>     intends, and not a substitute for a real interactive resize test, but
>     real evidence at a real (if wider-than-requested) width, not zero
>     evidence.
>
>     **Correction to the note above: Zoom, Fit, Focus, and a real Node
>     Context Menu are no longer "not yet built" -- they were flagged as
>     genuine §7.8/§9.3/§7.5 gaps (not merely untested paths: `GraphNodeState`
>     had no rendering-visible Failed/Stale, the canvas had no zoom transform
>     at all, and no compute node had any secondary-click handler) and then
>     actually built this pass, not just re-labeled:
>
>     - **Zoom** (`session.analysis_graph_zoom`, `desktop/src/studio/analysis.rs`'s
>       `zoomed_box`/`clamp_analysis_graph_zoom`): applied by scaling the
>       already-computed layout rects (and the canvas wrapper's own
>       width/height) before they're fed into each node/edge `Node`'s
>       `left/top/width/height` -- deliberately *not* a visual-only
>       `UiTransform` on the canvas wrapper, because that would leave the
>       scrollable content size (and therefore panning range and click
>       hit-testing) out of sync with what's actually drawn at a given zoom
>       level. Plain mouse wheel now zooms (a previously-inert gesture --
>       only shift+wheel and horizontal trackpad scroll did anything before);
>       shift+wheel/horizontal-scroll pan is unchanged. Verified for real via
>       `UTA_STUDIO_DEBUG_GRAPH_ZOOM=0.6`: screenshot shows "60%" in the new
>       VIEW control row and every node box visibly, correctly smaller, edges
>       still routed correctly, no overlap.
>     - **Fit**: reads the real current viewport width via a `ComputedNode`
>       query added to `handle_actions` (mirroring the existing pan-drag
>       observer's own query) and sets zoom to `viewport_width /
>       unscaled_canvas_width`, clamped -- not a fixed guess.
>     - **Focus Current/Failed/Stale**: three conditionally-rendered buttons
>       (`analysis_graph_focus_target`) computed from *real* per-node
>       `NodeState::Failed`/`::Stale` in the Phase 1 planner's
>       `plan_preview` -- notably, `GraphNodeState` (what the canvas boxes
>       actually render with) has no Failed/Stale variant at all, a real,
>       separate rendering-pipeline gap now documented rather than routed
>       around silently. A button only appears when a matching node actually
>       exists in this pass's plan and layout, per the phase plan's own
>       "菜单项必须按状态和节点能力启用或禁用" -- confirmed empirically: the
>       verified test song's 100%-complete run shows no Focus buttons at all,
>       correctly.
>     - **Node Context Menu** (`AnalysisNodeContextMenu`, secondary-click via
>       `open_analysis_node_from_click`, mirroring the existing
>       `SongContextMenu` pattern exactly): scoped honestly to the two
>       actions with a genuine, already-wired execution path --
>       "View in inspector" and "Retry with same configuration" (mapped per
>       node id to the same coarse `ReanalyzePitch`/`RealignSong`/
>       `ReanalyzeTranscript`/`ReanalyzeFull` commands Song Detail's own
>       buttons already call, via `analysis_node_retry_action`). The other
>       nine §7.5 items (run this node only, run downstream, configure for
>       this run, save as song profile, freeze, disable, bypass, view logs,
>       compare with previous attempt) are **not** faked with disabled
>       buttons that pretend to be real -- they need a generic per-node
>       execution API the analyzer doesn't have. `AnalysisRequest`'s
>       `disabled_nodes`/`frozen_artifacts` fields exist and are
>       planner-tested (Phase 1/4), but nothing at run time actually consumes
>       them -- the real executor is still the coarse special-purpose
>       functions (`reanalyze_pitch`, `mark_stems_only`, ...), not a unified
>       `AnalysisRequest`-driven run. That's a real, pre-existing Phase 4 gap
>       ("用统一 Planner 替换特殊 Flag") this pass surfaced but did not
>       attempt to close -- it's a cross-cutting executor/pipeline change,
>       not a UI wiring task.
>     - **Inspector facts, the other real §7.4 gap found alongside this**:
>       7 of the spec's 14 facts (`ALGORITHM VERSION`, `CACHE SIGNATURE`,
>       `LAST ATTEMPT` from the active `ArtifactRevision`'s
>       `algorithm_version`/`config_hash`/`created_at_ms`; `FALLBACK` from
>       the same route data the "current operation" banner already
>       surfaced, now also in the facts grid for any selected node, not just
>       the live one; `ERROR` from the selected history run;
>       `PARAMETERS`/`PARAMETER SOURCE` from the real
>       `AnalysisProfileSnapshot`, mapped per node via
>       `selected_stage_parameter`, only for the three nodes it actually
>       controls -- separator/ASR engine/alignment backend) were simply
>       missing from the array entirely, not merely unpopulated. `DURATION`
>       is deliberately still omitted (not faked): no per-node start/end
>       timing exists anywhere in the current event model, only whole-run
>       `started_at_ms`/`finished_at_ms`. Verified for real against the test
>       song's separation stage: `FALLBACK` correctly showed the actual
>       recorded xpu->cpu UVR-backend fallback reason in the warning color,
>       and `SEPARATOR`/`PARAMETER SOURCE` correctly showed `karaoke`/`Global
>       default`.
>     - 6 new unit tests (`graph_view_polish_tests`), `cargo test -p
>       uta-studio-desktop` now at 83 (was 76).
>
>     Genuinely still unverified, with no path found: real interactive
>     secondary-click on a live window (`UTA_STUDIO_DEBUG_OPEN_NODE_CONTEXT`
>     substitutes for it, same reasoning as the other debug hooks -- see
>     the ydotool note above). "Unknown Key 显示 Warning，不显示 Failure" /
>     "BPM-only fallback 正确展示" / "Descriptors unavailable 显示 Not
>     Applicable" also remain unverified -- the real test song had a
>     confident detected key and BPM, so those specific fallback-display
>     paths were never exercised.
>
>     **Packaged (`nix build`) smoke launch: now actually done, not just
>     substituted for.** The plain `nix build path:.#uta-studio` still fails
>     in `checkPhase` on the pre-existing, unrelated bug documented above
>     (`load_editor_beats` at `desktop/src/studio/editor/state.rs:804`
>     calling unguarded `CacheDir::new()` under Nix's non-writable sandboxed
>     `$HOME`) -- but that's a test-phase failure, not proof the *package*
>     itself is broken, so it was worth isolating the two. Ran:
>     `nix build --impure --no-link --print-out-paths --expr
>     '(builtins.getFlake (toString ./.)).packages.x86_64-linux.uta-studio.overrideAttrs
>     (_: { doCheck = false; })'` -- i.e. the real packaging derivation,
>     skipping only the already-root-caused test step. Built clean (exit 0),
>     producing `/nix/store/wkg3b2n2skd8d9da5n70qhsn7m1bpl58-uta-studio-0.3.0`.
>     Then actually launched that packaged binary (`bin/uta-studio`, the
>     wrapped executable, not the dev `target/debug` build) inside
>     `nix develop --command`, with `WAYLAND_DISPLAY` and the
>     `UTA_STUDIO_DEBUG_OPEN_HISTORY` hook set, and took a real
>     `cosmic-screenshot` of it. It launched cleanly (same Vulkan/Intel Arc
>     B580 adapter init, no crash, no missing-asset errors) and rendered the
>     Analysis Queue / DAG canvas correctly -- critically, with the real
>     compute edges from this session's `build_render_graph` fix visible
>     (Preflight -> Stem Separation -> Pitch Extraction -> Forced Alignment
>     -> Build Candidate Chart, plus the Lyrics Preprocessing -> Transcription
>     branch), confirming the fix holds in the packaged artifact too, not
>     just under `cargo run`. Process killed and confirmed gone afterward.
>     This closes the "打包产物 smoke launch" item with real evidence, not a
>     substitute. The project-name scan remains the one §9.5 item that never
>     needed any of this -- see its own note further down.
>
>   2 new tests this pass (`music_analysis_only_target_never_pulls_in_stems_pitch_or_lyrics`,
>   `delete_rejects_a_path_outside_the_cache_root_and_leaves_it_on_disk`),
>   both passing; `uta-studio-core` now at 229 tests, full suite still
>   green.
>
>   **§9.4 (API/security acceptance) audit.** Cross-referenced every
>   `app_core::` function call site under `desktop/src/` against
>   `API_CAPABILITIES`'s actual command list (not just its own
>   self-consistency test) to check "所有新增命令进入 API_CAPABILITIES." Found
>   one real gap from this redesign's own work: `get_song_analysis_profile`
>   -- its write-side siblings `set_song_analysis_profile`/
>   `reset_song_analysis_profile` were catalogued back in Phase 2, but the
>   getter itself was missed, and this pass gave it a live caller (the
>   Overview panel's "Active analysis profile" row). Added its entry
>   (`read`, automated-check-safe). Every other uncatalogued call site
>   found by the same scan (`search_lrclib_for_hash`, `update_song_settings`,
>   `migrate_analyzer_chart`, `load_lyrics_file`, `clear_models`,
>   `start_scan`, and others) predates this redesign entirely -- real gaps
>   in the wider app's catalogue coverage, but not "newly added" by this
>   plan, so left alone rather than scope-creeping this pass into
>   auditing the whole app's pre-existing API surface. Separately
>   hand-verified "Access 分类正确": read through every one of the 84
>   catalogued commands' `access` value against what its function actually
>   does (deletes data -> `destructive`, mutates but doesn't delete ->
>   `mutation`, only loads/queries -> `read`) -- no misclassification
>   found. "Diagnostics 不执行 Mutation 或 Destructive" was already covered by
>   `uta-studio-diagnostics`'s own test. "不新增未认证 HTTP 控制服务" holds
>   trivially -- no HTTP server was added anywhere in this plan's work.
>
>   **§9.5's "项目名扫描" (project-name scan): also actually completable
>   without a running app, and done.** Initially lumped this in with the
>   audio/display-dependent items above -- wrong call, corrected here.
>   Scanned every package-identity surface for a stray leftover/template
>   name: all 7 workspace crates' `Cargo.toml` `name`s (`uta-studio-desktop`,
>   `uta-studio-core`, `uta-studio-export`, `uta-studio-diagnostics`,
>   `uta-studio-audio`, plus the `app_core`/`uta-studio` lib/bin target
>   names and `xtask`, which is a standard Rust build-automation crate name,
>   not a product name), `flake.nix`'s `pname = "uta-studio"`,
>   `desktop/uta-studio.desktop` (`Name=Uta Studio`, `Exec=uta-studio`,
>   `Icon=uta-studio`), the native window's title string (`"Uta Studio"` in
>   `desktop/src/studio/mod.rs`), and every crate's `description`/`authors`
>   fields. Also checked for generic template/framework project-name
>   leftovers and stale configuration from the app's pre-Bevy history --
>   none found. Clean:
>   no stray project name anywhere in the scanned surface.
>
>   **User correction, then a real fix, not a re-labeled excuse.** The
>   inspector-vs-canvas node selection was earlier written up here as a
>   "known scoping choice" blocked on the `AnalysisStageRoute` wire protocol
>   not carrying `node_id` -- the user directly rejected that framing
>   ("既然能改，就完成掉", roughly "if it can be fixed, finish it, stop finding
>   excuses not to") and it was fixed for real this pass, not reworded:
>   `AnalysisStageRoute` (`app-core/src/analyzer.rs`) gained
>   `node_id: Option<String>` (`#[serde(default)]`, backward compatible with
>   old `snapshot_json` history rows); `server.py::_progress_payload`'s
>   `stage_routes` dict now keys by `node_id` when present (falling back to
>   the coarse bucket `stage` text otherwise) instead of by `stage` alone --
>   the actual root cause of a compound node's children silently
>   overwriting each other's route entry, since they all shared one bucket
>   key. Desktop got a new shared `find_matching_route` helper (precise
>   `node_id` match first, bucket-text fallback second), used by both the
>   inspector's `selected_route` and the canvas boxes'
>   `analysis_graph_route_summary` -- previously two independently
>   maintained, inconsistent lookups. Also fixed, same pass: the confirmed
>   canvas-vs-inspector percentage mismatch bug, via a new
>   `selected_progress_and_status` function that trusts the canvas's own
>   authoritative `GraphNodeState::Complete` over a `stage_routes` entry
>   that can be frozen at a stale non-100 value. 6 new Rust tests + 4 new
>   Python tests (`StageRoutesNodeIdKeyingTests`), all real, all passing
>   against the actual vendored torch/numpy environment (see below), not
>   skipped.
>
>   **Separately found and fixed while verifying the above: a real,
>   independent, pre-existing bug in `_classify_progress`** (the legacy text
>   classifier `server.py` is meant to be fully replaced by the Phase 3
>   structured-event protocol, but isn't yet, so it's still live).
>   `stems.py:85`'s real production message `"Loading audio file..."` (pct
>   10, meant to classify as `"separation"` per `STAGE_RANGES` and locked by
>   `test_pipeline_cache.py`) was actually classifying as
>   `"audio_preprocessing"`, because the broad substring check
>   `"loading audio" in text` -- meant only for `transcribe.py:43`'s
>   `f"Loading audio ({vocals_path})..."` -- also matched it. Narrowed to
>   `"loading audio (" in text`, which only transcribe.py's message contains.
>   New regression test case locks transcribe.py's real message too, so
>   this doesn't silently regress back.
>
>   **Also found and fixed, unblocking further Python-side verification for
>   this whole plan: the vendored analyzer venv's `numpy` install was
>   broken** (`/home/bintis/.uta-studio/vendor/venv`'s `site-packages/numpy/`
>   was missing `_core`/`_globals.py`/`fft`/`ma` and had no dist-info at all
>   -- not a version mismatch, files were actually gone, pre-existing and
>   unrelated to this session's edits). This was blocking every
>   `server`/`pipeline`-level Python test (`whisper_compat`-only tests still
>   ran fine, since that module doesn't need numpy). Fixed, with explicit
>   user approval, via `nix develop --command uv pip install --reinstall
>   numpy --python /home/bintis/.uta-studio/vendor/venv/bin/python3`
>   (resolved `numpy==2.4.6`, compatible with the already-installed numba's
>   `numpy<2.6,>=1.22` constraint). This means the earlier-recorded
>   "no torch/numpy environment available" reasoning for deferring the
>   Phase 2/3 `analysis_runs`/`analysis_node_attempts` writer and the
>   Phase 5 pitch-reset-timing bug fix no longer holds -- see `docs/plan.md`
>   §0.7 for the reusable invocation pattern
>   (`nix develop --command <venv-python> ...`, required for `libstdc++.so.6`
>   to resolve).
>
>   **Also found, NOT fixed (unrelated, out of scope for this plan)**:
>   `test_stems.py::SeparateStemsTests.test_uvr_converts_input_to_wav_before_separate`
>   fails with `TypeError: separate_stems_uvr() missing 1 required
>   positional argument: 'device'` -- confirmed pre-existing (neither
>   `stems.py` nor `test_stems.py` have any uncommitted changes this
>   session; last real commits touching them are `5d49f39`/`c700713`, well
>   before this plan's work). A stale signature/test mismatch in the UVR
>   stem-separation path, unrelated to the Analysis DAG redesign. Left
>   alone, noted for whoever owns that area.
>
>   **`test_stems.py` fixed** in a later pass, once actually run: two
>   independent stale test-signature drifts (not production-logic bugs),
>   found by finally running `python3 -m unittest discover -p "test_*.py"`
>   end to end -- `separate_stems_uvr()` gained a required `device` argument
>   at some point that `test_stems.py` never followed (production
>   `pipeline.py:175` already passed it), and the test's `_FakeSeparator`
>   stub didn't accept the `normalization_threshold`/`mdxc_params` kwargs
>   production code actually sends. Both are one-line test-file fixes; all
>   36 tests pass after.
>
>   **Phase 5 "失败时保留旧 Pitch" fixed for real** (user explicitly approved
>   modifying local data/behavior for this: "可以为了后续的工程推进，修改数据库。
>   我允许了"). Root cause: `reanalyze_pitch` deleted the old
>   `pitch_track`/`pitch_notes` cache files the instant a rerun was
>   *triggered*, not once the rerun was confirmed to have actually produced
>   new output -- a crash/OOM/cancel between those two points destroyed the
>   previous good pitch guide for nothing. Fixed with a rename-then-resolve
>   pattern instead of delete-then-hope: `back_up_before_reset` renames each
>   existing file to a sibling `.bak` (clearing any stale leftover `.bak`
>   from an unresolved earlier attempt first) instead of removing it;
>   `restore_or_commit_backup`, called at every real exit point of
>   `process_song`, looks at whether a fresh file now exists at the original
>   path -- if so the backup is deleted (the rerun produced real output), if
>   not the backup is renamed back (the rerun didn't). Deliberately
>   existence-based rather than gated on `SongResult::Done`/`Error`, because
>   `pipeline.py::analyze_pitch`'s own exception handler logs and continues
>   rather than failing the whole run, so a run that reports `Done` overall
>   can still have silently failed to produce new pitch data. `PendingNodeIntent`
>   gained a `backup_paths: Vec<(PathBuf, PathBuf)>` field to carry the
>   pending backups across the trigger-to-completion async boundary. 7 new
>   tests, real filesystem operations (temp dirs, not mocks). Scope note:
>   `realign`/`reanalyze_full` have the identical eager-delete pattern over a
>   larger, harder surface (multiple artifact kinds, and directory-scanned
>   variant files for `delete_transcript_variants`) -- deliberately not
>   folded into this pass; needs `CacheDir` to grow an "enumerate what would
>   be deleted without deleting it" capability first to reuse this backup
>   mechanism safely. Tracked as its own follow-up in `docs/plan.md` §2.
>
>   **Phase 4 §4.1 "enqueue-time config freeze" -- turned out to already be
>   done**, once actually checked instead of assumed: `FROZEN_CONFIGS`
>   already snapshots the *entire* `AppConfig` (not just node targets) at
>   `enqueue_one`/`enqueue_all` time, and `process_song` already resolves it
>   through `resolve_frozen_config` (current hash, then pre-rekey hash, then
>   a live fallback) instead of a live `AppConfig::load()`. An earlier pass
>   of this document had this marked incomplete without actually reading
>   that code path -- corrected here, not re-litigated.
>
>   **Phase 4's core gap -- "no generic per-node execution API" -- closed**,
>   without needing the `run_pipeline` monolith split (§4.2) an earlier pass
>   of this document assumed was a hard prerequisite. That assumption came
>   from reasoning about `run_pipeline`'s shape rather than reading it: it
>   already had `skip_transcription`/`skip_separation`, two booleans derived
>   from asking the Phase 1 planner what a target set implies -- the pattern
>   just needed a third boolean (`skip_pitch`) and a real caller that
>   actually threads `AnalysisRequest.disabled_nodes` through instead of
>   always passing an empty set, not a rewrite of `run_pipeline`'s structure.
>   New `app_core::run_analysis_plan(file_hash, targets, disabled_nodes)`
>   builds a real `AnalysisPlan` via `analysis_plan::build_plan` and rejects
>   (rather than silently no-opping) any `disabled_nodes` entry
>   `run_pipeline` has no way to actually honor yet
>   (`pipeline_can_honor_disable`: only `stems.separate`/`pitch.extract`/the
>   four `lyrics.*` nodes qualify -- `music.key`/`music.rhythm`/
>   `music.descriptors` are computed atomically inside one `analyze_music`
>   call with no way to disable just one, and `preflight`/
>   `chart.build_candidate` are `AlwaysRequired`). It does *not* reject when
>   disabling a node makes some other, merely-downstream node go `Blocked`
>   as an expected consequence (e.g. disabling `pitch.extract` under the
>   default full-run target blocks `chart.build_candidate`, since nothing
>   supplies its `PitchNoteCandidates` input another way this run) -- that's
>   `DisablePolicy::Optional`'s own documented behavior, not a failure, and
>   `run_pipeline`'s own final transcript-write step never actually checks
>   pitch data exists (missing pitch falls back to runtime pitchy detection
>   already). Two real single-node entry points sit on top:
>   `run_analysis_node(file_hash, node_id)` ("Run this node only") and
>   `disable_analysis_node_for_run(file_hash, node_id)` ("Disable for this
>   run"). `pipeline.py::run_pipeline` gained the `skip_pitch` parameter
>   itself, threaded through `server.py`. Desktop's Node Context Menu
>   (`desktop/src/studio/analysis.rs`) gained both as real buttons -- "Run
>   this node only" always shown, "Disable for this run" only shown when
>   `app_core::node_can_be_disabled_for_run(node_id)` is true, rather than a
>   button guaranteed to error. 9 new Rust tests (`pipeline_flags_tests`,
>   `run_analysis_plan_tests`) plus a new `test_run_pipeline_flags.py` (3
>   tests running `pipeline.run_pipeline` itself against the real vendored
>   venv, with only the heavy ML calls mocked out -- verifying `analyze_pitch`
>   is/isn't actually called, and that `skip_pitch=True` still patches
>   key/bpm onto the transcript correctly). Real-screenshot verified via
>   `UTA_STUDIO_DEBUG_OPEN_NODE_CONTEXT`: the `pitch.extract` node's menu
>   shows all 4 real actions; the `music.key` node's menu correctly omits
>   "Disable for this run" (3 actions), confirming the allow-list actually
>   gates real rendering, not just unit-test assertions. Deliberately not
>   done in this pass: the five existing coarse special-case functions
>   (`reanalyze_pitch`, `mark_stems_only`, `realign`, `reanalyze_transcript`,
>   `reanalyze_full`) were **not** rewritten to call through this new entry
>   point -- their chart-protection/backup logic (especially the pitch
>   backup mechanism above, freshly built and tested) was judged too risky
>   to fold into the same change; `frozen_artifacts`/Freeze/Bypass (§4.5)
>   still have no consumer either. See `docs/plan.md` Phase 4/6/7 sections
>   for the itemized remaining scope.
>
>   **The `realign`/`reanalyze_full`/`reanalyze_transcript` eager-delete gap
>   noted right above the pitch fix -- closed too, in a later pass, not left
>   as a standing gap.** Same rename-then-resolve pattern as the pitch fix,
>   over a larger surface: `CacheDir` gained two real enumerators,
>   `analysis_output_paths_keep_chart` and `transcript_variant_paths`, each
>   the actual "what would this delete" query for
>   `delete_analysis_outputs_keep_chart`/`delete_transcript_variants`
>   (refactored to be built *on* the enumerator rather than duplicating its
>   logic, so the two can't drift apart -- every existing test for both
>   delete methods still passes unchanged). `apply_realign_reset` and
>   `apply_reanalyze_reset` (both branches: transcript-only and full) now
>   back up every path the enumerator reports instead of removing it
>   directly, returning the same `Vec<(PathBuf, PathBuf)>` shape
>   `apply_pitch_reanalysis_reset` does. `realign`/`reanalyze` stash those
>   into `PendingNodeIntent.backup_paths` the same way `reanalyze_pitch`
>   does -- `process_song`'s five real exit points already resolve whatever
>   is in that field generically, regardless of which reset function put it
>   there, so no change was needed there at all. 7 new `analyzer.rs` tests
>   plus 2 new `cache.rs` tests that directly check the enumerator's output
>   against what the matching delete call actually removes (not "looks
>   right by inspection" -- the enumerated paths are fed through the real
>   delete call and asserted gone).
>
>   **§7.3 "Music Analysis 支持展开" -- Compound Node expand/collapse wired
>   to a real interaction.** `build_graph_view_model`'s `expanded:
>   &BTreeSet<AnalysisNodeId>` parameter was real and tested since Phase 7
>   landed; the only gap was that its one call site
>   (`spawn_analysis_session_overview`) always passed a hardcoded empty set.
>   `StudioSession` gained `expanded_compound_nodes`, toggled by a new
>   `UiAction::ToggleAnalysisCompoundNode`. The trigger is a third Node
>   Context Menu button ("Expand sub-checks"/"Collapse sub-checks", label
>   flips with current state) rather than overloading the existing
>   left-click "select stage" behavior. `analysis_node_compound_toggle_action`
>   (`desktop/src/studio/analysis.rs`) looks up "is this node compound at
>   all" directly from `AnalysisGraphSpec` rather than through the render
>   pipeline's `GraphNodeView`/`RenderNode`, since `collapsed_child_count`
>   alone can't tell an already-expanded compound node apart from a plain
>   one (both read 0) -- an earlier version of this change threaded a new
>   `is_compound` field through both view structs for this, but nothing
>   ever consumed it once the lookup-by-spec approach was used instead, so
>   it was removed rather than left as dead code. Both the real click path
>   and the `UTA_STUDIO_DEBUG_OPEN_NODE_CONTEXT` debug-injection path share
>   this one helper. New `UTA_STUDIO_DEBUG_EXPAND_COMPOUND=<node_id>` debug
>   var, same substitute-for-a-real-click pattern as the others. 4 new
>   tests. Real screenshots, before and after: the `music.analysis` node's
>   context menu shows "Expand sub-checks"; with
>   `UTA_STUDIO_DEBUG_EXPAND_COMPOUND=music.analysis` set, the canvas grows
>   a separate "Rhythm / BPM" box and the "Music Analysis" box's "N
>   sub-checks not shown" note disappears.
>
>   **"Focus Failed" was actually dead, not just relying on a
>   not-yet-built `GraphNodeState` variant -- corrected once actually
>   checked.** An earlier pass of this document said the "Focus
>   Failed/Stale" buttons "route around" `GraphNodeState` lacking
>   Failed/Stale variants by reading `NodeState` from `plan_preview`
>   directly. That's true for Stale, but false for Failed: `analysis_plan.rs
>   ::build_plan`'s own doc comment says it only ever produces `Ready |
>   Frozen | Disabled | Blocked | NotApplicable` -- `NodeState::Failed` is
>   never constructed anywhere in the codebase, so `plan_preview.nodes.iter
>   ().find(|n| n.state == NodeState::Failed)` always returned `None` and
>   the button never appeared, in any real use, ever. Now genuinely fixed:
>   `overlay_failed_node_attempts` (`desktop/src/studio/analysis.rs`) reads
>   the selected run's real `analysis_node_attempts` rows (the Phase 2/3
>   writer) and promotes any `Ready`-state node with a `status == "failed"`
>   attempt to `NodeState::Failed` -- deliberately not touching
>   `Blocked`/`Disabled`/`NotApplicable`/`Frozen`, which already carry a
>   more specific explanation from the current plan itself and shouldn't be
>   overridden by what could be a stale attempt row from an earlier run. 4
>   new tests. Verified against real data: the actual library has zero rows
>   in `analysis_node_attempts` yet (no real analysis run has happened since
>   that writer shipped), so there was no naturally-occurring failure to
>   screenshot against -- with the user's standing approval for database
>   edits in service of this work, one `status='failed'` row was inserted
>   against a real run id, the "Focus failed" button was confirmed to newly
>   appear in the canvas toolbar (screenshotted going from absent to
>   present), and the row was deleted immediately after. `Stale` is
>   deliberately **not** fixed alongside this -- it needs Phase 5's
>   still-nonexistent `candidate_chart`/`ChartUpdatePolicy` staleness
>   comparison, not a simple attempts-table query, so it stays exactly as
>   dead as the earlier audit found it. `GraphNodeState` (the canvas node
>   *box's* own render state, separate from what "Focus failed" reads)
>   still has no Failed variant either -- the box itself still won't look
>   any different, only the toolbar button and the inspector now reflect
>   real failure.
>
>   **§8.2's "Last successful run" row -- another "blocked on a writer that
>   doesn't exist" claim that didn't hold up once checked.** The
>   `analysis_history` table this needs (`file_hash`/`status`/
>   `finished_at_ms`) predates this whole pass entirely; nothing about it
>   was ever missing. The actual gap was narrower: Song Detail's
>   `song_overview_rows` (the closest thing today's single-column layout
>   has to §8.2's "Overview" section) never queried it. New pure function
>   `last_successful_run_copy(history, file_hash)`
>   (`desktop/src/studio/song_detail.rs`) finds the newest `status ==
>   "completed"` row for the song and formats its `finished_at_ms`, or
>   reports "None yet." 5 new tests, including the two real edge cases --
>   a later failed run must not hide an earlier success, and one song's
>   history must not leak into another's. Real screenshot: Song Detail's
>   Track information panel shows a real "2026-08-16 04:51 UTC" timestamp,
>   read from the library's actual analysis history, not fabricated.
>
>   **§7.6 "Play audio artifact"**: 4/9 to 5/9 on the Artifact Context Menu.
>   Reuses the same `uta_studio_audio::EditorAudioPlayer` "Play original"
>   already drives; `library.rs::play_artifact_revision` mirrors
>   `play_library_song`'s load/volume/play sequence but deliberately clears
>   (rather than repurposes) the library "now playing" queue state, since
>   this is a one-off preview of a specific artifact revision, not song
>   browsing. Gated by a new `artifact_kind_is_playable` predicate
>   (`VocalStem`/`InstrumentalStem`/`PreprocessedAudio` only -- real
>   waveform files, not the JSON-shaped artifact kinds a "Play" button would
>   just fail against) so the button is never offered somewhere it's
>   guaranteed to error. New `UTA_STUDIO_DEBUG_SYNC_ARTIFACTS=<file_hash>`
>   debug var runs the existing, already-shipped `import_legacy_artifacts`
>   on startup so the revision list (otherwise empty for any song that's
>   never had "Sync from disk" clicked) has something to render. 6 new
>   tests. **Verification note, not a gap**: the tests cover
>   `artifact_kind_is_playable`'s full classification and
>   `play_artifact_revision`'s missing-file rejection path without touching
>   real playback hardware (same test/example split as
>   `native-audio/examples/playback_smoke_test.rs`); the panel this button
>   lives in stayed below the visible viewport in every window
>   size/`UTA_STUDIO_DEBUG_WINDOW_SIZE` combination tried in this sandbox's
>   COSMIC tiling layout, so unlike the other fixes in this log there is no
>   screenshot of the actual "Play" button rendered in place -- the
>   surrounding row (Reveal/Set active/Delete) is structurally identical
>   code that was already screenshotted working earlier this session, and
>   the new logic is fully unit-tested, but this specific pixel was not
>   independently confirmed by a real screenshot.
>
>   **§9.4 API_CAPABILITIES audit -- four of the six "missing" entries were
>   already registered under a different command string, not actually
>   missing.** `search_lrclib_for_hash` (real function is
>   `lrclib_candidates`) already has `"search_lrclib_lyrics"`;
>   `load_lyrics_file` already has `"load_lyrics"` (no function named
>   `load_lyrics` exists at all); `start_scan` already has `"trigger_scan"`;
>   `clear_models` already has `"clear_models_command"` (noted in an earlier
>   pass of this log already). Same pattern as `load_analysis_run`/
>   `load_analysis_history` earlier. The two genuinely missing entries --
>   `update_song_settings` and `migrate_analyzer_chart` -- are now
>   registered; `api::tests::catalogue_has_unique_commands_and_known_access_
>   classes` passes.
>
>   **§8.6 "model availability -> Blocked" revisited and re-deferred for a
>   more precise reason.** The earlier "no real vendor status validation"
>   framing was wrong -- `vendor::model_install_statuses()` is real and does
>   real on-disk checks. The actual blocker: that function reads
>   `AppConfig::load()` (global config) internally to decide which
>   separator/asr_engine/align_backend/backend is "current," with no way to
>   pass an override in -- but `AnalysisRequest.model_availability` needs to
>   be evaluated per-song, against that song's resolved
>   `AnalysisProfileSnapshot` (which can override any of those). Wiring it
>   as-is would check the *global* model choice's availability even for a
>   song whose profile overrides to a different backend -- precisely the
>   "incorrectly mark an available node as Blocked" failure mode the
>   original deferral worried about, just with a more specific root cause
>   than originally written down. Fixing this for real needs
>   `model_install_statuses()` split into a version that takes explicit
>   backend selections instead of reading global config -- a separate,
>   real-scoped refactor, not something to fold in here. Still deferred,
>   now for the right reason.
>
>   **`lyrics.import_timed` (the Rust-side Timed LRC path) never had a real
>   event -- but not because it was blocked on Phase 4.2's pipeline split,
>   which was the wrong diagnosis (that's true for `lyrics.preprocess`,
>   which really does run inside `pipeline.py`; `lyrics.import_timed`
>   doesn't touch Python at all).** The real reason: this path runs
>   synchronously outside `process_song`'s queue lifecycle entirely, so
>   there was never an in-flight window for a progress event to report into
>   -- nobody had ever wired a completion record for it, structured event or
>   otherwise. New `lyrics.rs::record_timed_lyrics_import`, called once
>   `apply_timed_lyrics` succeeds, inserts one `analysis_history` row
>   (`status: "completed"`) and one matching `analysis_node_attempts` row
>   (`node_id: "lyrics.import_timed"`, `status: "succeeded"`) directly --
>   deliberately bypassing `ANALYSIS_STARTED`/`LIVE_ANALYSIS` entirely
>   (there's no in-flight state to coordinate with, and touching that shared
>   queue state from a path outside the queue would be the real risk here).
>   Side effect: this session's new "Last successful run" row now shows a
>   real timestamp after a Timed LRC import too, instead of always "None
>   yet." 2 new tests against a real DB fixture (`reconnect_for_test`),
>   including one that round-trips the stored `snapshot_json` back through
>   the real `AnalysisProgressSnapshot` type rather than just checking a row
>   exists -- `load_analysis_history` silently drops a row that fails to
>   deserialize, so a shape mismatch here would have failed silently
>   without that check.
>
>   **`GraphNodeState` gained a real `Failed` variant -- the canvas box
>   itself, not just the "Focus failed" toolbar button and inspector, now
>   visually marks a failed node.** `resolve_node_state`
>   (`analysis_model.rs`) returns `GraphNodeState::Failed` directly off a
>   `NodeState::Failed` planned state (the same real data
>   `overlay_failed_node_attempts` produces) before it can fall through to
>   the bucket-completion path, which would otherwise have misread a failed
>   node as `Complete` whenever its old artifact file still happened to
>   exist on disk. `graph_node_state_to_stage_state`
>   (`desktop/src/studio/analysis.rs`) renders it as "Failed · see the
>   inspector for details" using the same warning styling `Blocked` already
>   gets. `graph_node_state_rank` (picks the "most real" of several
>   candidate upstream routes feeding one virtual artifact box) ranks
>   `Failed` above `Blocked`/`Disabled`/`NotApplicable` -- a failed node
>   genuinely ran and produced a definitive outcome, those never even
>   attempted to -- but below `Waiting`/`Frozen`/`Running`/`Complete`. 4 new
>   tests. Real screenshot, before/after (same insert-screenshot-delete
>   pattern as the "Focus failed" verification): the `pitch.extract` box
>   itself changes from plain "WAITING · 0%" to "WAITING · 0% · Failed · see
>   the inspector for details" with warning coloring -- confirmed at full
>   window size this time, unlike the "Play audio artifact" verification
>   earlier in this log. `Stale` still has no `GraphNodeState` equivalent;
>   that half of the original gap is unchanged, since it needs Phase 5's
>   not-yet-built `candidate_chart`/`ChartUpdatePolicy` staleness
>   comparison, not a match-arm addition.
>
>   **`nix build path:.#uta-studio`'s checkPhase failure fixed for real,
>   correcting an earlier "don't touch it, it'll conflict" call that turned
>   out to be wrong for the specific file involved.** Root cause was already
>   correctly diagnosed in an earlier pass: `desktop/src/studio/editor/
>   state.rs::load_editor_beats` calls the panicking `app_core::
>   CacheDir::new()` (its internal `.expect()` fires when `$HOME` is
>   unwritable, as it is inside the Nix build sandbox). That earlier pass
>   declined to fix it, reasoning the file was mid-edit by another session --
>   but `state.rs` itself was never among that session's modified files
>   (`git status` showed `actions.rs`/`audition.rs`/`commands.rs`/
>   `input.rs`/`panels.rs`/`tracks.rs`/`view.rs` changed, not `state.rs`),
>   so the conflict concern didn't actually apply to this specific fix.
>   `CacheDir` gained `try_new() -> Option<Self>` -- same logic as `new()`,
>   `None` instead of a panic when the directory can't be created --
>   `load_editor_beats` switched to it, applying the exact "missing data
>   means nothing to draw, not a crash" philosophy the function's own
>   existing doc comment already stated but didn't actually follow for this
>   one branch. Verified against the real, unmodified failing command --
>   `nix build path:.#uta-studio --no-link --print-out-paths` (not the
>   earlier `doCheck = false` workaround) -- exit code 0, a real binary
>   produced. `CacheDir::new()` itself (and its dozens of other call sites)
>   was deliberately left untouched -- a much larger blast radius than this
>   one panic-prone caller warranted fixing in the same pass.

> Status: **Phase 0 draft** — written from an audit of the current code
> (2026-08-16), not from the aspirational plan alone. Every "current state"
> claim below has a file:line reference; every "target state" claim is
> forward-looking and unimplemented until its phase lands.
>
> Companion document: `uta-studio-analysis-dag-phases.md` (the phase-by-phase
> execution plan this document is a prerequisite for).

---

## 1. Why this document exists

The current Analysis DAG is a UI illusion: `desktop/src/studio/analysis.rs`
draws a fixed 7-node graph at hardcoded pixel coordinates and infers "is this
node done" purely from comparing a single global `stage_index: usize` against
a value derived by regexing Python's human-readable progress messages
(`server.py::_classify_progress`). No node/artifact/run domain model exists in
`app-core` today. This document defines the target semantics so that
implementation phases 1–9 have a single source of truth to build against
instead of re-deriving intent from UI code.

This document does not change behavior. Phase 0's only code changes are new
regression tests that lock down current, intentional behavior so later
refactors can be verified against it.

---

## 2. Current state summary (ground truth as of this audit)

| Concern | Where it lives today | Shape |
|---|---|---|
| Stage identity | `app-core/analyzer/server.py::_classify_progress` (lines 86–126) | Substring-matches a free-text `message` into one of 9 string stage IDs (`STAGE_RANGES`, lines 73–83). Not declared by pipeline code; inferred after the fact. |
| Progress struct | `app-core/src/analyzer.rs::AnalysisProgressSnapshot` (48–64) | Flat struct: `stage: String`, `stage_progress`, `operation`, `detail`, `implementation`, `model`, `device`, fallback fields, `stage_routes: Vec<AnalysisStageRoute>`. No graph, no artifact list. |
| UI stage mapping | `desktop/src/studio/analysis.rs::analysis_stage_index` (296–307) | Collapses the 9 Python stage strings into 7 UI indices via string match; unknown strings fall back to index 0. |
| "Node complete" | `desktop/src/studio/analysis.rs` (794–805) | `index < stage_index` — i.e., "the pipeline's *global* stage pointer has moved past this index," not "this artifact exists on disk." |
| Node coordinates | `desktop/src/studio/analysis.rs` (779–792) | 14 boxes (7 stage + 5 artifact + 2 export) at literal `f32` coordinates; connecting paths hand-drawn as polylines. |
| Special run flags | `app-core/src/analyzer.rs` (`FORCE_TRANSCRIBE`, `STEMS_ONLY`, `PITCH_ONLY`, 556–564) | Module-level `HashSet<String>` singletons keyed by file hash, consumed once per `process_song` call to set `skip_transcription`/`skip_separation` booleans on the outgoing JSON command. Python never sees these three names — only the two generic booleans. |
| Cache signature (stems) | `app-core/analyzer/pipeline.py::_cached_separator_matches` (88–94) + `_separator_marker_path` (66–67) | Marker file `{hash}_separator.json` = `{separator, options}`. Deliberately excludes key/tempo (documented at pipeline.py:130–135). **Already correct** per the target design — no change needed here. |
| Cache signature (music analysis) | `pipeline.py::MUSIC_ANALYSIS_VERSION = 1` + `_read_music_analysis_cache` (228, 235–254) | The only artifact with an explicit version-gated cache today. Not generalized to other artifact kinds. |
| Legacy stem recognition | `pipeline.py::_find_legacy_stem_cache` (103–123) | Recognizes pre-decoupling filenames `{hash}_vocals_{key}_1.0.{ext}`; read-only, never renames/deletes. **Already correct** per target design. |
| Authored chart protection | `app-core/src/cache.rs::invalidate_authored_chart` (235–240), called from 6 sites | **Currently: unconditional deletion**, by design (see §6). This is the one place current behavior and the phases plan's end-state diverge — see §6 for the explicit callout. |
| Persistence | `app-core/src/library_db/` (`schema.rs`, `SCHEMA_VERSION = 2`) | `analysis_queue` (one status + pct per song), `analysis_history` (one final `AnalysisProgressSnapshot` per run as a JSON blob). No per-node, per-artifact, or per-attempt tables. |
| API capability registry | `app-core/src/api.rs` (`API_CAPABILITIES`) | Exists, with `read`/`mutation`/`destructive`/`external`/`temporary` classes already defined and tested (api.rs:529–540). Not yet wired to any runtime enforcement — it's a catalogue, not a gate. Some entries are under-classified: `reanalyze_pitch` and `realign` are marked `"mutation"` despite silently deleting the authored chart; only `delete_song_cache` and `clear_analysis_history` are marked `"destructive"`. |

---

## 3. Stable Node ID list (first version)

Adopted as-is from the phase plan; verified against current pipeline
functions (`pipeline.py::run_pipeline`, 315–432) so the ID list maps onto real
code paths, not aspirational ones:

```text
preflight                  → validate_analysis_source (analyzer.rs) + early pipeline setup
music.analysis              → pipeline.py::analyze_music (compound)
  music.key                 → key_detect.py::detect_key_structured
  music.rhythm               → rhythm.py::analyze_rhythm
  music.descriptors          → key_detect.py::analyze_extra_descriptors (Essentia; NOT_APPLICABLE on Windows)
stems.separate               → pipeline.py::separate_and_cache
pitch.extract                 → pitch.py::analyze_pitch
lyrics.preprocess              → vocal-region / language detection steps inside transcribe_or_align
lyrics.transcribe               → transcribe.py::transcribe_vocals (Whisper/Parakeet path only)
lyrics.align                     → align.py::align_lyrics (Known Lyrics / forced-alignment path)
lyrics.import_timed               → Timed LRC import path (no ASR, no alignment)
chart.build_candidate               → transcript finalization writer (pipeline.py:424–430) — today writes directly to transcript.json; Phase 5 splits this into a real Candidate Chart artifact
```

`music.analysis` is confirmed compound: today it's one function producing one
cached JSON (`music_analysis_path`), but internally computes key, rhythm, and
optional descriptors as separable pieces of work — the sub-node IDs above are
new (Phase 1), not yet reflected in the cache file's structure.

Lyrics sub-graph selection in code today (pipeline.py `transcribe_or_align`,
198–225): `lyrics_path` provided → `lyrics.align` only; otherwise →
`lyrics.transcribe` then alignment via whatever `transcribe_vocals` returns.
Timed-LRC import bypasses `run_pipeline` analysis entirely today (handled in
`lyrics.rs::apply_timed_lyrics`, outside the Python pipeline) — Phase 1 must
model this as a first-class Planner branch, not a special case bolted onto
the UI.

---

## 4. Artifact Kind list (first version)

Adopted from the phase plan, cross-checked against actual cache filenames
(`cache.rs`, §2 above):

| Artifact Kind | Current file(s) | Notes |
|---|---|---|
| `source_media` | user's original file | Read-only, never in `songs_cache_dir()`. |
| `music_analysis` | `{hash}_music_analysis.json` | Already has `MUSIC_ANALYSIS_VERSION`. |
| `key_analysis` / `rhythm_analysis` / `beat_timestamps` | sub-fields of `music_analysis.json` today | Not separate files yet; Phase 2 may keep them logically distinct without physically splitting the file. |
| `audio_descriptors` | sub-field of `music_analysis.json` (Essentia) | `NOT_APPLICABLE` on platforms without Essentia. |
| `vocal_stem` / `instrumental_stem` | `{hash}_vocals.{flac,mp3}` / `{hash}_instrumental.{flac,mp3}` + variant forms | Marker file `{hash}_separator.json` is provenance, not a separate artifact kind — Phase 2 must fold it into `ArtifactRevision.config_hash`/`producer_node` rather than leaving it a parallel side-file (see §6 of the phase plan). |
| `pitch_track` / `pitch_note_candidates` | `{hash}_pitch_track.json` / `{hash}_pitch_notes.json` | |
| `lyrics_input` | `{hash}_lyrics.json` | User-provided LRC/plain lyrics. |
| `preprocessed_audio` | not persisted today (in-memory/temp) | New in Phase 2 if it needs to become inspectable. |
| `recognized_text` / `asr_segments` / `timed_transcript` | `{hash}_recognized_text.json` / `{hash}_asr_segments.json` / `{hash}_timed_transcript.json` (Phase 4.4, done) | `{hash}_transcript.json` (+ `{hash}_transcript_{tempo}.json` variants) is kept as a permanent, unchanged compatibility file alongside these, not replaced by them. |
| `candidate_chart` | does not exist yet | New in Phase 5. Today the closest equivalent is the transcript/pitch pair that `migrate_analyzer_chart` (chart.rs, vocal_chart.rs) turns into a chart on demand. |
| `authored_chart` | `{hash}_vocal_chart.json` | The one artifact with real user authorship; protection rules in §6 are the crux of this whole redesign. |
| `utz_export` / `ultrastar_export` | written directly to user-chosen export paths, not cached | Correctly excluded from the analysis DAG per design principle 2.3. |

---

## 5. Node dependency graph (target, Phase 7 will render this)

```text
source_media
    → preflight
preflight
    → music.analysis (music.key, music.rhythm, music.descriptors)
    → stems.separate
stems.separate
    → vocal_stem
    → instrumental_stem
vocal_stem
    → pitch.extract
    → lyrics route (preprocess → {transcribe|align|import_timed})
pitch.extract + lyrics route output (timed_transcript)
    → chart.build_candidate
chart.build_candidate
    → candidate_chart
    → (review/merge, user-driven) → authored_chart
authored_chart
    → export.utz / export.ultrastar   (explicit user action, not part of Analysis Run)
```

`music.analysis` and `stems.separate` are siblings under `preflight`, both
depending only on `source_media` — confirmed independent today: stem cache
signature never includes key/BPM (pipeline.py:130–135), and `music.analysis`
never reads stem files. This is the dependency fact that Phase 0.6's
regression tests below lock in.

### Dynamic branch rules (lyrics route), confirmed against `transcribe_or_align`

| Lyrics source | Nodes that run | Nodes that do NOT run |
|---|---|---|
| Timed LRC provided | `lyrics.import_timed` only | `lyrics.transcribe`, `lyrics.align`, `lyrics.preprocess` |
| Known/plain lyrics provided | `lyrics.preprocess` → `lyrics.align` | `lyrics.transcribe` |
| No lyrics, Whisper/OpenVINO Whisper | `lyrics.preprocess` → `lyrics.transcribe` → `lyrics.align` | — |
| No lyrics, Parakeet native timing | `lyrics.preprocess` → `lyrics.transcribe` (produces timing directly) | `lyrics.align` (redundant — Parakeet's ASR step already emits word timing) |

---

## 6. Freeze / Disable / Bypass / Invalidate — and the Authored Chart gap

Definitions are adopted verbatim from the phase plan (§2.4 there). The
important addition here is reconciling them with **current, intentional**
behavior:

**Current behavior (today, unconditional):** every mutating reanalysis or
lyrics-edit entry point deletes the authored chart outright via
`cache.rs::invalidate_authored_chart`. Confirmed call sites:

1. `analyzer.rs::reanalyze_pitch` (line 801)
2. `analyzer.rs::realign` (line 828)
3. `analyzer.rs::reanalyze` (line 861, shared by `reanalyze_transcript` / `reanalyze_force_transcribe`)
4. `analyzer.rs::reanalyze_full` → `cache.rs::delete_song_cache` (line 857, blanket wipe including the chart)
5. `lyrics.rs::save_lyrics_and_realign` (line 178)
6. `lyrics.rs::provide_lrc` (line 231)
7. `lyrics.rs::apply_timed_lyrics` (line 280)

The doc comment on `invalidate_authored_chart` (cache.rs:231–234) states the
rationale plainly: without this, a stale chart would hide new analysis
output. **This is not a bug** — it is a real design decision under the old
model, where there was no Candidate Chart concept to route new results
through instead.

**Target behavior (Phase 5):** none of the above call sites should delete the
authored chart. They should instead write a `candidate_chart` artifact and
leave `authored_chart` untouched, per `ChartUpdatePolicy::CreateCandidate`
(phase plan §5.1).

**Consequence for Phase 0 regression tests:** a test asserting "authored
chart survives `reanalyze_pitch`" would fail today — correctly, since that's
what current code does on purpose. Phase 0's regression suite therefore locks
in the *current* deletion behavior as an explicit, named baseline test (so a
future PR can flip it deliberately in Phase 5 and the diff makes the policy
change visible), rather than silently asserting the future behavior now. See
`app-core/src/cache.rs` test module for the codified version of this.

| Operation | Semantics | First real usage today |
|---|---|---|
| Freeze | Don't execute node, reuse current output, unblock downstream | Not implemented — no artifact revision concept to freeze yet (Phase 2). |
| Disable | Skip node and don't use its output this run | Closest existing analogue: `skip_transcription`/`skip_separation` booleans, but these are pipeline-wide kill switches, not per-node with Blocked-downstream semantics. |
| Bypass | Use an alternate input to route around a node | Not implemented. Candidate use case: routing `stems.separate` around via Original Mix when Bypass is chosen. |
| Invalidate | Explicitly mark/delete stale or wrong output | Closest existing analogue: `invalidate_authored_chart`, `delete_transcript_variants`, `delete_song_cache` — all unconditional deletes today, none require confirmation at the app-core layer (confirmation, where it exists, is UI-only — e.g. `RequestDeleteSongCache`/`ConfirmDeleteSongCache` in `song_detail.rs`). |

---

## 7. Node state machine (target)

States adopted from the phase plan §0.4, mapped onto what exists today:

| Target state | Closest current equivalent |
|---|---|
| `MISSING` | No cache file exists — inferred by UI via `stage_complete` returning false. |
| `READY` | Not distinguished today — everything not "complete" looks the same regardless of whether inputs are actually ready. |
| `QUEUED` | `QueuedStatus::Queued` (analyzer.rs:26–30) — song-level, not node-level. |
| `RUNNING` | `QueuedStatus::Analyzing(pct)` — song-level pct, no per-node running state. |
| `CACHED` | Not surfaced to UI at all — cache hits are invisible; the pipeline just skips work and progress jumps ahead with no distinct signal (see Phase 3, §3.5 of the phase plan: "Cache Hit 事件... 不得把 Cache Hit 假装成运行到 100%," which is exactly today's behavior). |
| `SUCCEEDED` / `SUCCEEDED_WITH_WARNINGS` | Not distinguished — e.g. an undetected key is not visibly different from a detected one in node state, only in the rendered value. |
| `FAILED` | `QueuedStatus::Failed(String)` — song-level only. |
| `STALE` | Does not exist — this is precisely what makes the current unconditional-chart-delete (§6) necessary as a workaround. |
| `FROZEN` | Does not exist (no artifact revisions to freeze). |
| `DISABLED` | Does not exist as a per-node state; only pipeline-wide skip booleans. |
| `BLOCKED` | Does not exist. Model-missing today likely surfaces as a pipeline error/fallback rather than a pre-flight blocked state — needs confirmation against `model_setup.py` in Phase 1 implementation, not assumed here. |
| `NOT_APPLICABLE` | Implicit — Essentia descriptors silently absent on Windows, no explicit state communicates why. |
| `CANCELLED` | Not implemented — no cancel affordance found in this audit; confirm during Phase 1. |

---

## 8. Cache signature rules (confirmed correct today, to be generalized)

The stem separation signature is the reference implementation to generalize
in Phase 2:

```text
stems.separate signature = separator backend + normalized separator options
    (excludes: source content hash is implicit via per-song cache dir key,
     key/tempo/BPM, algorithm_version of unrelated nodes)
```

Phase 2 must generalize this pattern (node_id + algorithm_version +
normalized_parameters + input_artifact_hashes + model_digest) to
`music.analysis`, `pitch.extract`, and the lyrics nodes — none of which have
an explicit signature today beyond "file exists" (transcript, pitch) or a
single version int (`music_analysis`, version 1 only, not composed with
input hashes).

---

## 9. Artifact invalidation matrix

Reproduced from the phase plan (§0.5) — verified consistent with current
dependency reality confirmed in §5 (music analysis and stems are provably
independent siblings today, so the matrix's claim that rerunning one doesn't
require rerunning the other already holds structurally).

| Rerun node | Regenerates | Stays | Authored Chart (current / target) |
|---|---|---|---|
| Music Analysis | Music Analysis | Stems, Pitch, Transcript | N/A today (no standalone action exists) / Kept |
| Stem Separation | Vocal, Instrumental | Music Analysis | Kept today (no chart-delete call site touches stems) / Kept, downstream marked Stale |
| Pitch Analysis | Pitch Track, Note Candidates, Candidate Chart | Stems, Transcript, Music Analysis | **Deleted today** (`reanalyze_pitch`) / Kept, new Candidate |
| Transcription | Recognized Text, Timed Transcript, Candidate Chart | Stems, Pitch, Music Analysis | **Deleted today** (`reanalyze`/`reanalyze_force_transcribe`) / Kept, new Candidate |
| Alignment | Timed Transcript, Candidate Chart | Recognized Text, Stems, Pitch | **Deleted today** (`realign`) / Kept, new Candidate |
| Candidate Chart Build | Candidate Chart | All analysis artifacts | N/A today (no distinct build step) / Kept |
| Export | Export file | All upstream | Not touched (export never deletes cache) |

---

## 10. Candidate vs. Authored Chart protection strategy

See §6 for the gap. Target strategy (Phase 5) is `ChartUpdatePolicy` with
default `CreateCandidate`, per phase plan §5.1–5.5. No implementation change
in Phase 0; this section exists so Phase 5 has an agreed target to implement
against without re-litigating the policy.

---

## 11. Legacy cache / history migration strategy

Confirmed already-correct legacy handling to preserve, not replace:

- `pipeline.py::_find_legacy_stem_cache` — recognizes pre-decoupling stem
  filenames, read-only, `tempo == 1.0` only (documented rationale at
  pipeline.py:103–113: a non-default tempo means a deliberate variant, not
  the base separation being searched for).
- `music_analysis_path` version gating (`MUSIC_ANALYSIS_VERSION`) — already
  refuses to reuse a cache written by an older analysis version, but this is
  a single global int, not a per-field signature; Phase 2 must decide whether
  finer-grained versioning (e.g., key detection version vs. rhythm version)
  is worth the complexity or whether the existing single-version gate for
  this one artifact is sufficient going forward.
- `chart.rs::load_chart` / `vocal_chart.rs::load_authoring_chart` — both
  already fall back to `migrate_analyzer_chart` when no saved
  `vocal_chart.json` exists. This existing migration path is exactly the
  mechanism Phase 2's "Legacy Artifact Import" (phase plan §2.4) should model
  itself on: read old data as-is, synthesize a Legacy Revision, never
  rewrite the source file.
- `analysis_history` table stores one full `AnalysisProgressSnapshot` JSON
  blob per run (`library_db/analysis_history.rs`). New Graph/Run snapshots
  (Phase 2+) must be additive — old rows must remain readable by whatever
  Legacy Adapter Phase 3.3 defines, not require a migration that rewrites
  historical JSON.

---

## 12. Model missing / algorithm fallback / device fallback display rules

Current signal-carrying fields already exist on `AnalysisProgressSnapshot`
and `AnalysisStageRoute` (analyzer.rs 48–80): `implementation`, `model`,
`device`, `requested_device`, and fallback-reason fields. These are real and
should be preserved as inputs to the future `NodeAttempt` record (Phase 2) —
Phase 1–3 work is to route them through a structured per-node event instead
of a per-stage "last known" snapshot, not to invent new fields. Confirming
exact model-missing → `BLOCKED` behavior against `model_setup.py` is
explicitly deferred to Phase 1 implementation (not verified in this audit).

---

## 13. Regression baseline (Phase 0.6)

Implemented as real tests, not merely a checklist, in:

- `app-core/src/cache.rs` (`#[cfg(test)] mod invalidation_tests`) — locks:
  - `invalidate_authored_chart` deletes only the chart file; music analysis,
    stems, transcript, and pitch files for the same hash are untouched.
  - `delete_transcript_variants` deletes only tempo-suffixed variant
    transcripts; the base transcript is untouched.
  - `delete_song_cache` deletes the full artifact set for its hash but never
    touches another hash's files in the same cache directory (dependency
    isolation / no accidental cross-song deletion).
  - Named baseline test documenting that `invalidate_authored_chart` is
    unconditional today (§6), so Phase 5 changes this file's tests visibly
    rather than silently.
- `app-core/analyzer/test_pipeline_cache.py` (new) — locks:
  - `_cached_separator_matches` is independent of any key/tempo/BPM input
    (only compares `separator` + `options`).
  - `_find_legacy_stem_cache` recognizes `tempo == 1.0` legacy filenames,
    ignores other tempos, and never mutates/deletes what it finds.
  - `_classify_progress` stage assignment for a fixed table of real
    production progress messages, so incidental message-text edits during
    Phase 1–3 don't silently reclassify a node's stage before the structured
    event protocol (Phase 3) replaces this function outright.

Explicitly **not** included as a passing regression test in Phase 0 (see §6):
"reanalysis does not delete the Authored Chart." That assertion is false
today by design; it becomes a real regression test starting in Phase 5, at
which point the current unconditional-delete tests above should be updated
in the same PR that changes the behavior.

---

## 14. Known limitations of this document

- Model-missing → `BLOCKED` and cancellation semantics are asserted as "not
  yet confirmed" rather than verified, pending Phase 1's implementation work
  against `model_setup.py` and the queue/worker cancellation path.
- Artifact Kind splitting for `recognized_text` / `asr_segments` /
  `timed_transcript` (previously one `transcript.json`) shipped in Phase
  4.4 (see `docs/plan.md` §4.4) — this document's target shape above is now
  the real on-disk layout, not just a plan. `transcript.json` itself is
  unaffected: it keeps being written, unchanged, as a permanent
  compatibility file.
- Song Detail's §8.2 six-section reorg and §8.4's three-tier parameter
  inheritance display (Global Defaults → Song Profile → Run Override)
  shipped in a later Phase 8 pass (see `docs/plan.md` §8.2/§8.4) — song
  profile now actually affects real execution (`process_song`) instead of
  being preview-only decoration, and a real (intentionally ephemeral)
  Run-tier override exists via `configure_analysis_node_for_run`. §8.3's
  control migration is mostly done (Force transcribe/Refetch & align/View
  logs entry points added to the Node Context Menu; only "Analysis
  defaults" redirect and whether to remove the now-redundant Song Detail
  buttons remain, the latter deliberately left to the user). Node Context
  Menu's "View logs" item shipped in the same session: `app-core/src/
  applog.rs` is a real bounded ring buffer + best-effort log file, fed by a
  genuine `tracing_subscriber` layer (`LogPlugin.custom_layer`) -- `get_log_
  path`/`get_recent_logs` are no longer catalogue-only entries with no
  implementation.
- This document will be updated at the end of each phase per Agent Rule 12
  in the phase plan ("每个 Phase 完成后更新设计文档中的状态和已知限制").
