# 21D — Analysis + Expert System Design Completion

**State:** `READY`
**Parent:** card 21 final design-parity audit
**Task class:** serial source/test closure; no model download or accelerator execution by default
**Current scope:** current-source blockers found by rereading the authoritative `docs/design` set after all packaged model routes were admitted to Production

## Current truth

All packaged models now expose their effective non-CPU route as `ProductionPinned` under the explicit repository-owner release policy. Model validation labels are therefore not a standing blocker for this card. Normal Studio execution must still use `RuntimePolicy::Production`; CPU reference remains an explicit diagnostic lane, and missing installation/runtime/structural requirements still fail closed.

The deleted `docs/design/audio-analysis/UTA_EXPERT_FUSION_STAGE4_GAP_REVIEW_2026-08-27.md` was a stale historical review. This card is the single current implementation checklist. Do not create another gap log.

## Mandatory ownership and safety

Read and obey:

```text
AGENTS.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
docs/design/README.md
```

Hard constraints:

- Studio communicates only through `AnalysisCliClient` / `RuntimeCliClient`; no backend implementation dependency under `app-core/**` or `desktop/**`.
- Engine owns analysis algorithms, expert scheduling, fusion, Candidate construction and typed analysis artifacts.
- Runtime Manager owns resource/policy truth.
- Studio owns queue/history, Candidate/Authored revisions, editor and user-facing UTZ/UltraStar export.
- No model download, inference, Vulkan/Level Zero context, Nix build or whole-workspace release pass for ordinary checklist items. Any non-Qwen accelerator execution requires separate explicit user authorization.
- Source media and configured model directories remain read-only user data.
- Keep every application source file at or below 2,000 lines.

## Completion order

Work serially. A later phase must not be marked complete while an earlier contract it depends on remains open.

### A. Production policy and trust boundary

- [x] Compile normal Studio requests with `RuntimePolicy::Production`.
- [x] Keep Experimental only for an explicit CPU diagnostic request.
- [x] Query Engine capabilities under the exact request policy instead of a hard-coded policy.
- [x] Make `Unsupported` unresolvable under every Runtime policy.
- [x] Update current design/status copy to reflect the explicit all-model Production admission without removing fail-closed installation/runtime/structural behavior.
- [x] Add focused wire/default tests proving omitted policy defaults to Production and explicit CPU diagnostics alone select Experimental.
- [x] Verify the shared compiler used by every normal product analysis entry produces Production request snapshots for every product target.

Acceptance:

```text
normal Preview JSON contains runtime_policy=production
explicit diagnostic_cpu Preview JSON contains runtime_policy=experimental
Unsupported never resolves
no automatic CPU or Production -> Benchmark/Experimental fallback
```

### B. Immediate execution-path correctness

- [x] Build FCPE-primary baseline review from FCPE evidence instead of hard-coded RMVPE evidence.
- [x] Skip unresolved optional cleanup nodes while preserving the explicit degraded reason from resource resolution.
- [x] Reject `f0_derived` at the Engine trust boundary whenever any GAME node remains enabled.
- [x] Use profile-aware default conditional policies for standalone requests instead of treating every optional expert as `Always`.
- [x] Declare optional `technique_evidence` in the exact Engine Plan whenever `technique.analyze` can emit it.
- [x] Remove LRCLIB lookup/write side effects from read-only Plan Preview; online lyric acquisition remains an explicit Song Detail action.
- [x] Add an end-to-end fake-worker FCPE-primary Candidate test covering validate -> requirements -> plan -> analyze.
- [x] Add Preview + execution regression coverage for a missing optional denoise/dereverb resource, asserting `ok_degraded` rather than failure.
- [x] Add packaged-CLI validation tests for forged `f0_derived + GAME enabled` requests.
- [x] Add Engine-result/app-core-commit coverage for present and absent optional technique artifacts.

### C. Candidate and evidence artifact contracts

- [x] Replace newly emitted custom Candidate bytes with a strict `utz::VocalChart` 0.3 document; the final Active `CandidateChart` is strict VocalChart 0.3.
- [x] Validate legacy-projected or strict Candidate JSON semantically at the Studio process boundary before any Artifact DB mutation.
- [x] Preserve backward read compatibility for existing `uta.analysis-engine.candidate-vocal-chart/v1` cache entries without emitting new entries in that legacy shape.
- [x] Redesign newly emitted `SingingAnalysisV1` so selected evidence references stable track/phrase/note/lyric IDs rather than embedding a second authoritative canonical track; retain deserialize-only legacy compatibility.
- [x] Ensure quantization cannot make Candidate geometry diverge from duplicated SingingAnalysis geometry; raw/unquantized candidate ranges remain explicitly non-authoritative proposal evidence.
- [x] Keep continuous F0 in PitchEvidence/SingingAnalysis evidence only; strict VocalChart contains symbolic target-note geometry only.
- [x] Cover malformed chart, duplicate IDs, unresolved lyric continuation, overlapping notes, wrong MIME and post-quantization ID stability.

Acceptance:

```text
Engine Candidate bytes parse and validate with vendor/utz
Studio validates before publish_batch
Candidate/Authored authority remains separate
SingingAnalysis references chart IDs and owns no duplicate authoritative timing
```

### D. Candidate graph and global pitch selection

- [x] Represent segment-level semitone/octave pitch proposals as decoder states, not review-only `alternatives[]` attached after `target_midi` is fixed.
- [x] Keep frame-wise continuous F0 separate; only robust segment proposals become target-note states.
- [x] Let GAME/ROSVOT/STARS boundary candidates and RMVPE/FCPE segment pitch proposals participate in one deterministic context-aware graph.
- [x] Preserve hard boundaries, duration/non-overlap constraints, correlation metadata and source-local-score rules.
- [x] Add cases where GAME remains selected, where a context-supported pitch alternative wins, and where unresolved disagreement stays uncertain/reviewable.
- [x] Extend decision traces with considered/selected pitch sources and a typed deterministic selection reason.

Acceptance:

```text
final target MIDI is assigned by global decode
no raw cross-model score comparison
no frame-wise nearest-MIDI conversion
same request/evidence produces byte-stable graph, trace and result
```

### E. Transcript, alignment and expert escalation

- [x] Build typed transcript disagreement regions from Qwen confidence, reference mismatch and language/coverage facts before FireRed scheduling.
- [x] Give FireRed an explicit full-input-on-disagreement contract; `OnDisagreement` no longer becomes an unconditional skip merely because bounded windows are unsupported.
- [x] Feed FireRed as challenger evidence into transcript fusion with stable provider preference after consensus/calibrated evidence, instead of appending it after fusion.
- [x] Implement deterministic sequence-edit reconciliation for sufficiently matching reference text while preserving generated versus caller-canonical authority.
- [x] Keep downloaded/search-result lyrics in the review editor until an explicit save adopts them as canonical.
- [x] Pass forced-alignment word boundaries into GAME's real `known_boundaries` tensor for every overlapping chunk; remove the conditioned path's hard-coded all-false mask.
- [x] Test canonical lyrics, imperfect reference lyrics, no lyrics, low-confidence ASR, language mismatch, FireRed skip/run/failure and melisma identity.

### F. Expressive DSP baseline

- [x] Extend Engine-owned `acoustic-dsp-v2` evidence with vibrato, glide/portamento, ornament/melisma, breath and voicing-transition observations.
- [x] Keep all DSP activations explicitly source-local and uncalibrated.
- [x] Use expressive continuity to reject marginal false semitone chatter/splits without manufacturing final technique confidence.
- [x] Keep STARS technique optional and dependency-correlated; technique-only execution does not create Candidate note artifacts.
- [x] Add deterministic generated-audio fixtures for steady tone, vibrato, glide, repeated onset, breath/noise and silence.

### G. Separation quality and vocal topology

- [x] Evaluate instrumental vocal leakage and musical damage against the generated Instrumental artifact, never against the original mix.
- [x] Retain/use `vocal_residual` long enough to measure lead purity and foreground/support ambiguity before trusting monophonic downstream analysis.
- [x] Introduce a typed `VocalTopologyEstimate` with `single_lead`, `alternating_multi_lead`, `overlapping_multi_lead`, `lead_with_support`, `unknown`, plus overlap/support regions.
- [x] Do not equate `singing.is_some()` with independent foreground/topology evidence.
- [x] Run lead-purity/topology checks before monophonic GAME/F0/Fusion is trusted; unknown/ambiguous evidence degrades and marks affected regions.
- [x] Keep `audio.lead_partition` and automatic singer identity future/optional; do not fabricate Singer A/B or Backing/Harmony stems.
- [x] Add clean solo, support leakage, alternating duet (residual-only alternation remains ambiguous), simultaneous overlap, instrumental leakage, musical damage and insufficient-evidence fixtures.

### H. Studio ownership and workflow contract

- [x] Reduce the compiled Studio extension to product/capability intent plus the typed fusion/provider preference fields the Engine explicitly versions.
- [x] Remove concrete runtime executable/recipe truth and private worker parameters from Studio-owned execution authority; Runtime Manager/Engine resolve them.
- [x] Keep stable explicit provider intent sticky only through a versioned Engine preference contract.
- [x] Ensure Plan Preview displays the Engine-resolved DAG/runtime rather than requiring it to equal a Studio-authored private runtime graph.
- [x] Preserve legal Processing Studio topology/reorder/duplicate/analyzer-attachment semantics through capability-level intent.
- [x] Make legal same-branch role-preserving cards pointer-draggable with pointer capture/global cleanup; reject cross-branch or invalid drops and immediately rerender semantic execution order.
- [x] Expose visible Add/Restore controls for product-approved optional cards in stages 01–03, and Delete only when typed graph bypass/removal remains valid.
- [x] Keep every card capability-first while always showing its configured provider; include the provider in otherwise-identical continuous-pitch/note card headings.
- [x] Show independent Vocal-output and BGM/Instrumental-output provider slots on the separation card; use a selector only when more than one Engine-eligible provider really exists.
- [x] Replace the misleading stage 04 presentation with one required Engine-owned fusion-policy card; internal normalization/graph/decode/finalization stages are not draggable/addable/deletable Studio processors.
- [x] Add/restore optional transcript and singing experts as an atomic node + analyzer attachment + evidence edge edit, and allow optional FireRed disable/delete while preserving required Qwen transcription.
- [x] Update independently owned wire DTOs on both sides; never share backend Rust types with Studio.

### I. Settings, run sheet and read-only Preview UX

- [x] Implement the six-section `Settings > Analysis` order from the UX design:
  - [x] Quality & output behavior
  - [x] Audio preparation
  - [x] Lyrics & alignment
  - [x] Pitch, notes & fusion
  - [x] Advanced performance/model-owned parameters
  - [x] Automation
- [x] State that existing chart data changes only after explicit re-analysis.
- [x] Make normal Candidate quantization default On, with explicit BPM/grid readiness behavior.
- [x] Replace the single-output run target selector with the designed multi-output run sheet while preserving independent partial requests.
- [x] Keep model lifecycle/install controls exclusively in Models & runtime.
- [x] Remove or disable every UI control that has no exact request/local-action consumer.
- [x] Keep Preview read-only and side-effect free; explicit install/download/lyrics actions remain separately classified APIs.
- [x] Update EN/zh-CN/ja copy and UI/API capability tests together.

### J. Lifecycle events and typed failures

- [x] Extend the worker protocol with typed `node_started`, measured `node_progress`, `node_completed`, `node_failed`, `artifact`, `warning` and `degraded` frames carrying request/raw-node/presentation-node/capability/model IDs and Engine timestamps.
- [x] Keep Engine-run overall progress indeterminate unless Engine supplies explicit overall work units; show per-node percentages only for measured native-worker fractions and never invent stage-order percentages.
- [x] Persist raw Engine node identity separately from Studio presentation-node identity with backward-compatible optional history fields.
- [x] Render the execution DAG as four horizontal Processing Studio rows and expand every concrete model invocation (including independent Vocal/BGM extraction models) into its own live node.
- [x] Preserve structured Engine error code/resource/capability/request ID through queue/history/UI while retaining readable legacy error text.
- [x] Add cancellation/event ordering, malformed lifecycle-frame, worker-crash/reconnect and old-history compatibility tests.

## Explicitly outside this card

Do not reopen these as completion blockers:

- model Production admission: all effective packaged non-CPU routes are already explicitly admitted;
- learned dynamic weights, general cross-model calibration and a fully probabilistic HSMM;
- automatic `audio.lead_partition`, multi-F0 singer assignment or fabricated Backing/Harmony stems;
- Analysis Engine standalone USTX/MIDI export: product UTZ/UltraStar export remains Studio/app-core-owned under `PROCESS_BOUNDARY_RULES.md`;
- hash-based acceptance/rejection;
- whole-workspace/Nix/final repository release acceptance.

## Primary implementation map

| Phase | Primary owning files |
| --- | --- |
| A | `app-core/src/analysis_engine_adapter.rs`, `app-core/src/backend_cli/{analysis_client,analysis_wire,runtime_client}.rs`, `runtime-manager/src/{state,catalog,resolver}.rs` |
| B | `analysis-engine/src/{engine.rs,candidate_pipeline.rs,workflow.rs}`, `analysis-engine/src/engine/workflow_execution.rs`, `analysis-engine/src/planner/plan.rs` |
| C | `analysis-engine/src/artifact/{vocal_chart,singing_analysis}.rs`, `analysis-engine/src/engine/workflow_execution.rs`, `app-core/src/{vocal_chart,analyzer/engine_run}.rs`, `vendor/utz/src/lib.rs` only when the format implementation itself is wrong |
| D | `analysis-engine/src/fusion/{baseline,hsmm,canonical,review,types}.rs`, `analysis-engine/src/candidate_pipeline.rs` |
| E | `analysis-engine/src/{engine.rs,candidate_pipeline.rs,conditional_scheduler.rs}`, `native-inference/openvino-worker/src/game.rs`, independently owned app-core wire DTOs only if the process contract changes |
| F | `analysis-engine/src/audio/acoustic.rs`, `analysis-engine/src/artifact/singing_analysis.rs`, `analysis-engine/src/fusion/{baseline,hsmm,review}.rs` |
| G | `analysis-engine/src/audio/quality.rs`, `analysis-engine/src/contract/quality.rs`, `analysis-engine/src/engine.rs`, app-core quality-report wire validation |
| H | `app-core/src/workflow/{compiler,wire,definition}.rs`, `analysis-engine/src/{workflow,workflow_executor,planner/plan}.rs` |
| I | `app-core/src/{analysis_experience,analysis_engine_adapter}.rs`, `desktop/src/studio/{settings/analysis,analysis_actions,analysis_preview,processing_studio/mod}.rs` and locale catalogs |
| J | `analysis-engine/src/worker.rs`, `app-core/src/backend_cli/{analysis_client,analysis_wire}.rs`, `app-core/src/analyzer/{engine_run,queue}.rs`, Desktop activity/error projections |

Do not edit Runtime Manager policy in Studio, copy Engine algorithms into app-core, or solve wire changes by linking implementation crates.

## Focused verification matrix

Each completed phase updates tests in the owning package. Before marking 21D `READY`, run only the focused non-inference matrix unless a separately authorized card expands it:

```text
bash dev.sh -c cargo test -p uta-runtime-manager
bash dev.sh -c cargo test -p uta-analysis-engine
bash dev.sh -c cargo test -p uta-studio-core
bash dev.sh -c cargo test -p uta-studio-desktop
bash dev.sh -c cargo test -p utz
```

Also verify:

```text
cargo tree for app-core/desktop contains no backend implementation crates
zero uta_analysis_engine:: / uta_runtime_manager:: imports under app-core/desktop
Preview is read-only and exact request/plan identity survives queueing
all changed application Rust files remain <= 2,000 lines
git diff --check for the focused change
```

Do not run the reserved whole-workspace/Nix release pass from this card.

## Ready condition

21D becomes `READY` only when every checkbox above is either checked with current source/tests or moved to a newer authoritative design as explicitly out of scope. Then rerun card 21 as a new current audit revision and update `tasks/remaining-models/STATE.md` plus `docs/KEY_CONCLUSIONS.md`. Do not create a separate completion log.
