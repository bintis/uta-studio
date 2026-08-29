# 22 — UI / Workflow Execution UX Convergence

**State:** `READY`

**Precondition:** 21I / 21J may proceed independently; do not block this card on their algorithm work unless a touched surface overlaps.

**Task class:** Desktop UX + truthful execution presentation + narrowly scoped protocol/domain exposure where required.

## Mission

Converge several current UI surfaces that are technically functional but no longer match the product mental model or the real execution model.

The target is not a cosmetic redesign. The target is:

```text
simple default presentation
+
exact execution truth on demand
+
no fake progress
+
no fake model/provider topology
```

This card covers five user-facing problems:

```text
A. Advanced DAG is visually unreadable because every real dependency is permanently drawn.
B. Node logs/progress do not consistently reflect the worker's real chunk execution.
C. The Editor entry opens a local-file picker instead of first showing the chart library.
D. The chart-library context menu has no explicit Delete chart action.
E. Processing Studio model/provider selection does not fully express multi-output / multi-provider execution topology.
```

Do not use this card to rewrite the Analysis Engine or Workflow compiler.

---

# A. Advanced DAG visual simplification

## A1. Current problem

The current Advanced DAG renders the exact compiled bindings as persistent edges.

That is truthful as a data model, but visually it creates a fan-in/fan-out wall of solid lines, especially around:

```text
preprocessing output
-> every lyric/pitch/note expert
-> evidence fusion
-> candidate graph
-> finalization
```

The result is difficult to read even when the user only wants to answer:

```text
What is running now?
What comes next?
Which model is this card?
```

Current source builds render edges directly from every compiled workflow binding:

```text
desktop/src/studio/analysis_model/workflow.rs
```

with roles such as:

```text
ComputeDependency
AnalyzerAttachment
InactiveBinding
```

and active bindings are normally rendered as solid lines.

## A2. Required presentation model

Do **not** delete or alter exact Workflow bindings.

Separate:

```text
execution topology truth
```

from:

```text
default visual connector set
```

The graph model must retain every exact binding for inspection, history, validation and edge selection.

The normal canvas should be simplified.

### Solid-edge rule

For the default non-selected graph view:

```text
one visible node may have at most one outgoing solid continuation edge
```

and that solid edge may only target the next meaningful visible execution node/continuation for that presentation lane.

Do not draw permanent solid fan-out from one source into every downstream analyzer.

Do not draw permanent solid fan-in from every expert into Step 4.

### Secondary exact bindings

All additional real bindings remain available but should be represented as one of:

```text
hidden by default
thin/dashed contextual edge
revealed when a node/edge is selected or hovered
shown in the node inspector / dependency list
```

Preferred behavior:

```text
Default graph:
    clean backbone / local continuation only

Select node:
    reveal all exact incoming/outgoing bindings for that node

Select edge / Inspect:
    show exact from-port, to-port, semantic type and audio role
```

Analyzer attachment lines from the same audio source to many independent experts should not remain as full-height permanent solid wires.

Inactive bindings should be hidden or extremely faint by default, while still inspectable.

### Disabled-node visibility rule

A Workflow node whose authored execution policy is `ExecutionPolicy::Disabled` must not appear anywhere on the Advanced DAG canvas.

This means:

```text
Disabled node
-> no visible DAG card
-> no dashed/ghost placeholder
-> no incident visible edge
```

The node is **not deleted from the Workflow definition**. Processing Studio remains the configuration surface where the user can see that disabled step and re-enable it. Once re-enabled, it may return to the Advanced DAG according to the exact compiled plan.

Do not conflate Disabled with other execution states:

```text
Disabled       -> hidden from Advanced DAG
NotRequested   -> may remain visible when useful to explain the exact request
ProfileSkipped -> may remain visible as profile-skipped
Deferred       -> remains visible as conditional/deferred
```

Only explicit user-authored disablement removes the node from the DAG presentation. This filtering is presentation-only and must not mutate the persisted Workflow or compiled execution truth.

## A3. Truthfulness constraints

Visual simplification must never create a false execution dependency.

If there is no real direct binding between two nodes, do not invent a solid arrow merely to make the drawing chain-like.

If a branch has no unique solid continuation, it is valid to show no solid outgoing edge until the user selects the node.

The exact compiled topology remains authoritative.

The layout cache/digest must remain based on execution topology, not on transient selected-edge visibility.

## A4. Acceptance

Add tests proving:

```text
1. Every exact Workflow binding is still retained in the underlying render/inspection model.
2. A node authored with ExecutionPolicy::Disabled has no visible DAG card or incident visible edge.
3. Re-enabling that node makes it eligible to appear again without reconstructing or losing its authored Workflow identity.
4. NotRequested / ProfileSkipped / Deferred are not accidentally hidden by the Disabled-node filter.
5. Default-visible solid outgoing edge count <= 1 per visible node.
6. Selecting a visible node exposes every exact binding attached to that visible node.
7. No presentation-only solid edge connects nodes that have no real execution relationship.
8. Inactive/conditional bindings remain inspectable without permanently cluttering the canvas.
9. Parallel provider/model cards remain independently selectable.
```

Main files likely include:

```text
desktop/src/studio/analysis_model.rs
desktop/src/studio/analysis_model/workflow.rs
desktop/src/studio/analysis_render/graph.rs
desktop/src/studio/analysis_render/nodes.rs
desktop/src/studio/analysis_edge_selection.rs
desktop/src/studio/analysis_layout/**
```

---

# B. Node progress and logs must follow real worker chunks

## B1. Current problem

The UI currently has lifecycle fields for real work-unit accounting:

```text
work_units_completed
work_units_total
```

but the native worker progress protocol mainly exposes:

```text
fraction
message
```

and `LifecycleNodeGuard::progress()` currently emits only the fraction/message.

This causes several bad outcomes:

```text
- node percentage may not correspond to real model chunk/window completion;
- two workers can report superficially similar percentages with different underlying work;
- presentation-node logs may show only run-level records while the actual worker executed under another engine/task identity;
- the node's visible status and the node-filtered JSONL log can disagree.
```

A visible percentage is only useful if it is based on actual completed work.

## B2. Canonical progress contract

For any worker that processes the input in chunks/windows/batches, progress must expose real units.

Extend the worker/lifecycle protocol with the semantic equivalent of:

```text
work_units_completed: u64
work_units_total: u64
work_unit_kind: optional string
```

Examples of `work_unit_kind`:

```text
chunk
window
batch
frame_block
segment
```

Do not hard-code UI behavior around one model's wording.

### Percentage rule

When real units are present:

```text
percent = completed / total
```

with strict validation:

```text
total > 0
completed <= total
monotonic completed for one task
```

When real units are **not** available:

```text
show Running / Indeterminate
```

Do **not** invent a percentage from:

```text
node order
elapsed wall time
number of DAG nodes completed
estimated model duration
presentation animation
```

If a legacy worker only provides an ungrounded fraction, it may remain in debug/log data temporarily, but the normal node percentage should not present it as exact chunk progress.

One-shot operations may use a truthful `0/1 -> 1/1` model only if the operation really is one indivisible work unit.

## B3. Workers must report their actual execution units

Audit at least:

```text
GGML / RoFormer workers
OpenVINO workers
Qwen ASR / forced alignment windows
RMVPE / FCPE
GAME / Basic Pitch / ROSVOT / STARS
```

Where a model is already internally chunked/windowed, emit those exact boundaries/counts.

Do not create synthetic chunks solely to make a progress bar move.

For workers whose underlying inference library provides no meaningful chunk boundary, use indeterminate progress until a real unit contract exists.

## B4. Lifecycle forwarding

Add a measured-unit API on the Engine lifecycle layer, for example the semantic equivalent of:

```text
LifecycleNodeGuard::progress_units(completed, total, kind, message)
```

and forward it through:

```text
native worker
-> Analysis Engine worker client
-> Engine lifecycle frame
-> app-core AnalysisProgressSnapshot / AnalysisStageRoute
-> Desktop DAG node
```

The existing `work_units_completed/work_units_total` fields should become actually populated by real execution.

## B5. Log console must use the same execution identity

The node log viewer must show records belonging to the selected presentation node and its actual Engine/native task identities.

Every relevant run-owned JSONL lifecycle/worker record should contain enough correlation information to map:

```text
request_id
engine node_id
presentation_node_id
task_id / worker task identity
capability_id
model_id
implementation
work units
message/output event
```

Opening the log for a node such as:

```text
vocal_bgm_split.vocal
```

must not produce an apparently empty console containing only a generic `run_requested` record if the actual extraction worker has emitted lifecycle/progress/output events.

Do not duplicate fake model logs in Desktop.

The visible node status, Activity state, and node-filtered log must be projections of the same Engine-owned lifecycle facts.

## B6. Log safety

Retain current bounded machine-protocol rules.

Never dump raw model tensors, unbounded stderr, credentials, source audio bytes or arbitrary provider content into the log.

## B7. Acceptance

Add tests proving:

```text
1. Worker chunk/window events propagate completed/total to Desktop.
2. A 3/10 worker event displays 30%, not a guessed node-stage percentage.
3. Progress units are monotonic and invalid completed/total frames fail closed.
4. A worker without real units displays indeterminate Running rather than a fake percent.
5. Node-filtered logs include actual lifecycle/progress/output records for that presentation node.
6. Split presentation nodes (e.g. vocal vs instrumental) filter to the correct engine/model work.
7. Duplicate capability instances remain independently correlated.
8. Completion is 100% only after the real task reports all units / terminal completion.
```

Main files likely include:

```text
analysis-engine/src/events.rs
analysis-engine/src/execution/client.rs
analysis-engine/src/worker.rs
native-inference/*/src/protocol.rs
native-inference/* worker execution paths
app-core/src/backend_cli/analysis_wire.rs
app-core/src/backend_cli/analysis_client.rs
app-core/src/analyzer/engine_run.rs
app-core/src/analyzer/queue.rs
desktop/src/studio/analysis_model/workflow.rs
desktop/src/studio/analysis_render/**
desktop/src/studio/analysis_actions.rs
```

---

# C. Editor entry should open the chart library first

## C1. Current problem

The current chart-library toolbar has an `Open editor` action that triggers `ChooseEditorFile`, which immediately opens the native local-file picker.

That is the wrong primary Editor mental model.

The product already has a chart/authoring library surface (`LibraryView::Completed`, shown to the user as the chart/sheet collection).

The normal path should be:

```text
Editor
-> chart library
-> user selects a song
-> Editor opens that song
```

not:

```text
Editor
-> native file picker
```

## C2. Required navigation

The primary sidebar/top-level **Editor** entry should navigate to the chart/sheet library surface.

The chart library should show authored/editor-ready songs as cards/rows.

Clicking a song/card should enter the Editor for that song directly.

Preserve the current unsaved-dirty guard before switching from one active editor document to another.

## C3. Local file is a secondary action

Move the current local-file picker into a clearly named top-right toolbar action on the chart library:

```text
Open local file…
```

This is an escape hatch, not the main Editor entry.

Initially preserve current library-identity safety unless a separate import design explicitly changes it:

```text
selected local file must resolve to an indexed library song
```

If the file is not indexed, show an actionable message such as:

```text
This file is not in the indexed library. Add its folder or rescan first.
```

Do not silently create a second identity for the same audio source.

## C4. Avoid duplicate/confusing navigation

If both an `Editor` sidebar item and a `Charts/谱面` library facet remain visible, they must resolve to the same chart-library destination or be consolidated so the user does not see two apparently different entry points for the same task.

Do not make one open a file picker and the other open the library.

## C5. Acceptance

```text
1. Clicking Editor never immediately opens a native file picker.
2. Editor navigation lands on the chart/sheet library.
3. Clicking an editor-ready song opens that song in NativeEditor.
4. Open local file… is visible in the chart-library top-right toolbar.
5. Local-file selection preserves indexed-library identity checks.
6. Dirty editor documents block accidental song replacement.
7. Browser/library/editor back navigation remains deterministic.
```

Main files likely include:

```text
desktop/src/studio/chrome.rs
desktop/src/studio/navigation.rs
desktop/src/studio/commands.rs
desktop/src/studio/actions_content.rs
desktop/src/studio/library/browse.rs
desktop/src/studio/song_detail/**
```

---

# D. Chart-library right-click menu needs Delete

## D1. Required action

On the chart/sheet library surface, the song/chart context menu should include:

```text
Delete chart…
```

This means deleting the user's chart/authoring product for that song.

It must **not** mean deleting the original audio/video source file from disk.

## D2. Confirmation and pin safety

Deletion is destructive and must require confirmation.

Reuse the existing authored-chart ownership/pinning rules.

Current app-core already has explicit authored-chart discard semantics around:

```text
replace_authored_chart_with_fresh_analysis(file_hash)
```

and pinned artifact protection.

Do not bypass pinning.

If the active authored chart/revision is pinned, deletion should fail with an actionable explanation until the user unpins it.

## D3. Post-delete behavior

After confirmed deletion:

```text
- source song remains in the music library;
- original audio/video remains untouched;
- analysis evidence/candidate data should remain unless the user explicitly chose a broader cache deletion action;
- the song leaves the authored/editor-ready chart collection if no chart remains;
- opening it later may return to Analyze/Create Candidate flow as appropriate.
```

Do not overload the existing `Delete song cache` action with this menu item.

Do not delete immutable historical evidence/artifact revisions unless the existing artifact contract explicitly defines that as part of chart deletion.

## D4. Acceptance

```text
1. Right-click chart item exposes Delete chart….
2. First click opens confirmation; no data is deleted yet.
3. Confirm deletes only the intended authored chart state/materialization.
4. Source media remains on disk and remains indexed.
5. Pinned authored revision prevents deletion.
6. Analysis evidence remains available unless separately deleted.
7. Library/chart counts refresh immediately after success.
```

Main files likely include:

```text
desktop/src/studio/library/browse.rs
desktop/src/studio/commands.rs
desktop/src/studio/actions_content.rs
app-core/src/chart.rs
app-core/src/analysis_artifact.rs
app-core/src/cache.rs
```

---

# E. Workflow cards must expose truthful model/provider topology

## E1. Product intent

A Workflow card should not merely show a hard-coded provider label.

Where the capability supports multiple eligible model strategies, the user should be able to choose the model/provider from the Workflow card.

The **number of visible execution cards must follow the real selected execution topology**.

This is especially important for Vocal/BGM separation.

## E2. Current state

The code already has partial model selection for interchangeable providers such as:

```text
Continuous F0: RMVPE / FCPE
Note boundary: GAME / Basic Pitch / ROSVOT / STARS
```

through `workflow_model_options()` and `SetWorkflowNodeModel`.

But `audio.separate_vocal_bgm` is different.

It owns at least two semantic output/provider slots:

```text
vocal output provider
instrumental/BGM output provider
```

The current Processing Studio card displays both configured providers, but does not expose the same truthful multi-slot selection model to the user.

The Advanced DAG already projects that logical separation node into concrete vocal/instrumental presentation nodes.

This card should finish that model instead of adding more hard-coded presentation exceptions.

## E3. Execution-card rule

Card count must be based on **actual worker/model execution identity**, not merely the number of semantic output ports.

Required rule:

```text
If one selected model invocation genuinely produces both required semantic outputs:
    render one execution card with two output ports/roles.

If Vocal and Instrumental require different model invocations/providers:
    render two execution cards.
```

Example product behavior intended by this card:

```text
Strategy A
one separation model executes once
-> Vocal
-> Instrumental
=> one card

Strategy B
karaoke/vocal-specialized model executes
+
independent BGM/instrumental model executes
=> two cards
```

Before collapsing anything to one card, verify the Engine/native worker contract really produces both outputs in one invocation.

Do not collapse two independent workers just because they live under one product capability.

Do not split one real invocation into two fake progress cards merely because it has two output ports.

## E4. Provider-slot model

Replace ad-hoc separation parameters with a typed product representation where practical.

The semantic equivalent should be able to represent:

```text
capability: audio.separate_vocal_bgm

strategy / provider executions:
    execution 1:
        model = <model A>
        outputs = [Vocal, Instrumental]

or

    execution 1:
        model = <vocal/karaoke model>
        outputs = [Vocal]

    execution 2:
        model = <instrumental model>
        outputs = [Instrumental]
```

The exact schema may differ, but it must not require Desktop to infer model semantics from model names.

Model output roles/capabilities belong in the Workflow/Runtime/Engine contract.

## E5. Model picker UX

When a Workflow card/node is selected, expose only model strategies that are actually valid for that semantic slot/capability.

The picker should show:

```text
model display name
semantic output role(s)
configured execution condition
```

Runtime readiness remains Plan Preview / Runtime Manager truth.

Processing Studio may configure a provider even if it is currently missing, but must not claim it is runnable.

Do not duplicate Runtime Manager installation policy in Desktop.

For multi-output separation, allow independent selection where the strategy genuinely has independent provider slots.

## E6. DAG projection

Advanced DAG must render the selected strategy truthfully:

```text
one real invocation -> one compute node/card
multiple real invocations -> multiple compute nodes/cards
```

Each card must have its own:

```text
model identity
runtime state
progress/work units
log correlation
output ports
```

This directly connects section E to section B: two separate model executions require two independently correct progress/log streams.

## E7. Migration

Existing saved workflows currently use fields such as:

```text
node.model_id
instrumental_model_id / provider_preferences.instrumental
```

Add deterministic migration into the new typed provider/execution representation if the schema changes.

Do not reinterpret an old workflow into a materially different execution strategy without making the mapping explicit.

## E8. Acceptance

Add tests for at least:

```text
1. A single-invocation dual-output separation strategy renders one execution card.
2. A two-provider Vocal + Instrumental strategy renders two execution cards.
3. Each two-provider card receives only its own runtime progress/log events.
4. Changing the Vocal provider does not silently change the Instrumental provider.
5. Changing the Instrumental provider does not silently change the Vocal provider.
6. Invalid model/semantic-role combinations fail before queueing.
7. Exact Plan Preview shows the exact selected provider execution topology.
8. Saved legacy separation workflows migrate deterministically.
9. Desktop never infers output roles from a model filename/id substring.
```

Main files likely include:

```text
app-core/src/workflow/capability.rs
app-core/src/workflow/definition.rs
app-core/src/workflow/compiler.rs
app-core/src/workflow/wire.rs
app-core/src/workflow/default_definition.rs
analysis-engine/src/workflow.rs
analysis-engine/src/workflow_executor.rs
analysis-engine/src/planner/plan.rs
analysis-engine/src/engine.rs
desktop/src/studio/processing_studio/**
desktop/src/studio/analysis_model/workflow.rs
desktop/src/studio/analysis_render/**
```

---

# F. Interaction and visual consistency

The screenshots that motivated this task show a broader consistency issue: some surfaces use Chinese localization while adjacent model/status/detail copy remains raw English.

Any new controls introduced by this card must be localized in:

```text
EN
zh-CN
ja
```

Do not add new raw English-only production strings for:

```text
Delete chart
Open local file
progress unit labels
provider strategy labels
DAG contextual dependency labels
```

Keep current visual language; this task is not a theme redesign.

---

# G. Non-goals

Do not do the following as part of Task 22:

```text
- rewrite the Analysis Engine into a generic DAG executor;
- change actual Workflow dependencies merely to reduce visual edge count;
- invent model progress percentages;
- make Desktop inspect native process state directly;
- delete source media from the chart Delete action;
- silently import arbitrary local files under a second song identity;
- infer model output semantics from filenames/model names;
- merge distinct model executions into one fake card;
- split one real worker invocation into fake independent progress cards;
- redesign Step 4 fusion policy (21I owns that);
- redesign melody-path inference (21J owns that).
```

---

# H. Recommended implementation order

Implement serially so UI presentation is always backed by the required truth source:

```text
1. B — real work-unit/chunk lifecycle contract + node-log correlation
2. E — typed provider/execution topology for Workflow cards
3. A — simplified DAG presentation using the now-correct execution identities
4. C — Editor navigation / chart-library landing page
5. D — Delete chart action and confirmation
6. F — localization and final visual polish
7. full focused regression matrix
```

Do not start by hiding graph edges before exact-node identity/progress topology is stable.

---

# I. Verification

Run at least:

```text
bash dev.sh -c cargo test -p uta-analysis-engine
bash dev.sh -c cargo test -p uta-runtime-manager
bash dev.sh -c cargo test -p uta-studio-core
bash dev.sh -c cargo test -p uta-studio-desktop
bash dev.sh -c cargo fmt --all -- --check
bash dev.sh -c cargo xtask docs check
```

Also run:

```text
git diff --check
```

excluding retained binary/test evidence only where already justified by repository policy.

Add focused tests that cover all acceptance rows in A/B/C/D/E.

If worker protocol fields change, update every native worker implementation and its protocol tests in the same change; do not leave mixed incompatible worker versions.

---

# J. Definition of done

Task 22 is complete only when all of the following are true:

```text
[x] Advanced DAG default view no longer shows permanent full fan-in/fan-out spaghetti.
[x] Every visible node has at most one default solid outgoing continuation edge.
[x] Nodes authored as ExecutionPolicy::Disabled do not appear on the Advanced DAG and leave no ghost/dashed incident edges.
[x] Re-enabling a disabled Workflow node restores it to the DAG without losing its persisted configuration; NotRequested/ProfileSkipped/Deferred remain distinct visible states where applicable.
[x] Exact compiled bindings remain inspectable on selection and are never deleted for presentation simplicity.
[x] Node percentage is derived from real completed/total worker units when shown.
[x] Nodes without real work-unit accounting show indeterminate Running, not a guessed percentage.
[x] Node log viewer shows the actual correlated Engine/native events for that presentation node.
[x] Split/multi-provider executions have independent progress and logs.
[x] Clicking the primary Editor entry opens the chart/sheet library rather than a native file picker.
[x] Clicking an editor-ready song opens its editor.
[x] Open local file… exists as a secondary top-right chart-library action.
[x] Chart right-click menu includes confirmed Delete chart… behavior that never deletes source media.
[x] Pinned authored charts cannot be deleted accidentally.
[x] Workflow cards expose valid model/provider selection where the capability supports alternatives.
[x] Vocal/BGM separation card count follows actual model invocation topology: one invocation = one card, two invocations = two cards.
[x] Model output roles come from typed capability/provider contracts, not Desktop inference.
[x] Exact Plan Preview matches the selected provider execution topology.
[x] EN / zh-CN / ja copy is synchronized.
[x] Focused tests, fmt, docs and diff hygiene pass.
```

---

# K. Implementation evidence

- Advanced DAG simplification is presentation-only: persisted exact bindings remain
  unchanged, Disabled cards and their incident presentation edges are omitted,
  only the default outgoing route is solid, secondary context is dashed/thinner,
  and selecting a card reveals every exact incoming/outgoing binding.
- Native measured work units retain exact worker task identity through Engine,
  app-core and Desktop. Basic Pitch, FCPE, GAME, ROSVOT, STARS, Qwen ASR and Qwen
  alignment report their real completed/total inference-window or conditioned-
  segment units; missing, malformed, uncorrelated or regressing units are rejected
  or rendered indeterminate rather than converted into guessed progress. Worker
  readiness also binds the exact runtime-recipe digest, so a same-component but
  incompatible package cannot pass the handshake. Unitless lifecycle fractions
  never leak as exact percentages and provider-split logs correlate through typed
  presentation identity. Qwen child stdout/stderr are read concurrently under one
  exact 16 MiB cap and the whole process group is terminated on overflow.
- The primary Editor route and Charts facet share one destination; editor-ready
  Charts cards open the Editor directly and `Open local file…` is a secondary
  indexed-library action. Dirty editor state blocks both accidental chart
  replacement and every Activity/Processing Studio/library route change until
  the exact deferred target is preserved and the user confirms leaving.
- Delete Chart clears only the active AuthoredChart selection and compatibility
  materialization. Every usable authored pin blocks the transaction; exact current
  compatibility bytes are captured first, then the recovery row and deactivation
  commit together even for legacy-only charts. Post-commit cleanup is best-effort,
  so a hidden staged-file cleanup failure cannot falsely report that the semantic
  deletion rolled back. Source media, CandidateChart and evidence remain intact,
  normal loading does not silently resurrect history, and Artifact Workbench can
  explicitly reactivate the retained revision. Confirmation/success copy is
  synchronized in EN, zh-CN and ja and states that recovery remains available.
- Typed execution-invocation descriptors, not model-name inference, determine
  one-card dual-output versus independent provider cards, their progress/log
  correlation, and exact Plan Preview topology. Engine validation binds vocal and
  instrumental outputs to their exact requested capabilities rather than accepting
  set-equal but semantically swapped topology. Compilation keeps only descriptors
  intersecting the exact requested capabilities, so partial requests cannot claim
  an unrequested provider invocation. Separation nodes cannot omit their typed
  invocation topology, and required inputs must come from a producer whose
  execution policy covers the consumer's policy. Provider-topology copy is
  localized in EN, zh-CN and ja.
- Verification (2026-08-29 re-check, after 21J's Qwen alignment/ASR and
  real-song fixes — `native-inference/qwen-worker`,
  `analysis-engine/src/fusion/{candidate_states,baseline,hsmm}.rs` (a
  candidate-evidence relation bound calibration fix), and
  `analysis-engine/src/engine/runtime_route.rs` (a canonical-lyrics
  line-boundary fix); see `21J_MELODY_PATH_SCORE_COHERENCE.md` §21.2–21.4 for
  detail. No Task 22 UI/workflow/DAG/provider-topology surface was touched
  this pass) found the Windows-target build genuinely broken: `cargo check
  --target x86_64-pc-windows-gnu -p uta-qwen-worker` (part of the
  `package-windows` release job's build set) failed — `engine.rs` used
  `std::os::unix::process::CommandExt`, `Command::process_group`/`pre_exec`
  and `libc::kill`/`prctl` unconditionally, with no `#[cfg(unix)]` gate and no
  Windows equivalent. This predated this session and this task's other
  changes (present unmodified at `HEAD`) and was recorded honestly rather
  than silently left unverified.
- **Fixed** (2026-08-29, same day): `native-inference/qwen-worker/src/engine.rs`
  gained a `ProcessTreeGuard` mirroring the Job-Object pattern already used in
  `analysis-engine/src/execution/agent_client.rs` — Unix keeps the fresh
  process-group identity the pinned engine is spawned as leader of; Windows
  creates the engine suspended (`CREATE_SUSPENDED`), assigns it to a
  kill-on-close Job Object before resuming its threads, so no descendant it
  spawns can escape containment. `terminate_engine_group(pid)` (which only
  ever terminated the direct child on a real process-group leak risk) is
  replaced by `process_tree.terminate()`, called at the exact same two sites
  (oversized captured output; normal exit) plus on `Drop` for any early-return
  path. `libc`/`windows-sys` are now correctly `target.'cfg(unix/windows)'`
  scoped in `Cargo.toml` instead of an unconditional `libc` dependency. The
  6 existing end-to-end tests that spawn a `#!/usr/bin/env bash` fake-engine
  fixture are now explicitly `#[cfg(unix)]` (they were implicitly Unix-only
  before via the same unconditional import that broke the Windows build; the
  portable pure-function tests already covered the same orchestration logic
  cross-platform). Added one new descendant-process-tree test per platform
  (`run_engine_kills_descendants_that_outlive_the_direct_child`): the Unix
  variant spawns and passes locally; the Windows variant mirrors
  `agent_client.rs`'s own `job_object_close_terminates_adapter_descendants`
  (PowerShell spawns a detached `ping.exe`) and is compile-verified via the
  cross-target check on this Linux host, same as that existing precedent.
  `cargo check --target x86_64-pc-windows-gnu` now passes cleanly for the
  entire `package-windows` release job's package set
  (`uta-studio-desktop`, `uta-analysis-engine`, `uta-runtime-manager`,
  `uta-ggml-worker`, `uta-openvino-worker`, `uta-qwen-worker` together, not
  just `uta-qwen-worker` alone). Analysis Engine `252` with `2` ignored + CLI
  `4`; Runtime Manager `67` + CLI `10`; app-core `409` with `1` ignored;
  Desktop `187`; OpenVINO worker `58`; Qwen worker `35` (16 pre-existing + 19
  new: 18 from the 21J alignment/ASR fixes, 1 new descendant-cleanup test);
  GGML worker `5`. `cargo fmt --all -- --check`, `cargo xtask docs check`
  (covers generated JSON doc bundles) and `git diff --check` all pass. Source-
  size checks: no standalone checker distinct from the crate test suites was
  found in-repo; the bounded-read invariants they would enforce
  (`MAX_REQUEST_BYTES`, `MAX_EVIDENCE_BYTES`, `MAX_ENGINE_OUTPUT_BYTES`, etc.)
  are exercised by the passing test suites above. This was the sole remaining
  gap blocking Task 22; **State moves to `READY`.**
