# 20 — Product E2E Feature Bubble

**Precondition:** cards 15–19 and 20A = `READY`; Phase A model cards are terminal, while Production-only model blockers may remain explicitly retained
**Task class:** validation-only integration smoke; fix nothing broad in this card
**GPU:** OpenVINO live workloads only; Vulkan remains unauthorized

## Read

```text
AGENTS.md
docs/agent-tasks/MODEL_GPU_WORK_POLICY.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
tasks/final-features/20_PRODUCT_E2E_FEATURE_BUBBLE.md
```

Read completion records for cards 15–19, not their full implementation histories.

## Purpose

Validate that all newly completed features compose through the real process boundary and product UX without reopening implementation scope.

If a concrete defect is found, stop the affected scenario and finish `NEEDS_REVIEW` with the exact owning subsystem. The Master creates a focused follow-up card. Do not turn this smoke card into a broad fixer.

## Bubble A — process-boundary / compiled Workflow

From Processing Studio product intent, prove:

```text
Desktop
  -> app-core local Workflow / wire DTO
  -> AnalysisCliClient
  -> uta-analyze
  -> Analysis Engine compiled Workflow executor
```

Validate:

```text
saved Workflow -> immutable execution snapshot
Plan Preview request/workflow digest == queued execution snapshot
user reorder changes legal execution order
invalid type drop rejected
cycle rejected
duplicate transformation node supported
priority does not create dependency
analyzer consumes exact selected artifact
Disabled does not execute
MaximumOnly obeys quality mode
OnDisagreement is scheduler-driven, not Always
Advanced Graph represents the exact compiled graph
```

No direct Studio backend crate calls are allowed.

## Bubble B — semantic audio lanes

Using short owned/authorized stereo fixtures and already-accepted model generations, run one workload at a time and verify the truthful available lane set:

```text
Original Mix
 -> vocal extraction
 -> Lead
 -> Backing
 -> Harmony

Original Mix
 -> Instrumental
```

Also exercise optional cleanup where product Workflow includes it:

```text
Lead -> Denoise -> Dereverb
```

Validate semantic role, duration/timeline, channel/sample-rate policy, artifact provenance, and no duplicate-role byte relabeling.

## Bubble C — conditional experts + Candidate pipeline

Use bounded typed/live OpenVINO evidence where permitted:

```text
baseline evidence
 -> disagreement region
 -> conditional expert scheduling
 -> dependency-aware Fusion
 -> Candidate Graph
 -> SingingAnalysis
 -> Candidate VocalChart
```

Required assertions:

```text
no disagreement -> optional expert not executed
disagreement -> only intended bounded region scheduled
missing optional expert does not fabricate zero evidence
STARS/ROSVOT dependency correlation is preserved
raw logits are not called calibrated confidence
continuous F0 remains separate from target MIDI
technique evidence does not create extra MIDI notes
quantization, when enabled, changes only symbolic Candidate timing under explicit musical context
```

## Bubble D — Editor evidence / authoring

Verify current real product behavior:

```text
Candidate opens
SingingAnalysis / Evidence layers are read-only
Review Queue navigation reaches the expected region
technique evidence is visible in the appropriate evidence surface
Suggestion accept is an explicit authoring command and is undoable
Candidate/Authored compare works
merge creates/updates Authored only
re-analysis does not silently overwrite Authored
Lead/Harmony/Backing/Adlib chart-track behavior is preserved
A/B audition targets the selected artifact/revision
artifact playback/waveform picker uses the selected artifact
```

Do not judge sustained playback while a large build/model workload is saturating the machine.

## Bubble E — Chinese / non-Chinese semantic scenarios

Run bounded scenarios that cover both Chinese and non-Chinese text/lyrics semantics.

Because Vulkan remains unauthorized, do not create a new Qwen Vulkan context merely to satisfy this bubble. Use one of:

1. canonical/supplied typed inputs that avoid a new Qwen execution while still exercising the downstream process boundary; or
2. exact previously accepted Qwen evidence replay through typed test/integration seams.

Record clearly whether a scenario was live end-to-end or replayed at the Qwen boundary. Do not describe replay as a new live Qwen acceptance.

If a truly live Qwen/full-song scenario is required for release confidence, record `requires separately authorized Qwen Vulkan validation` rather than executing it.

## Bubble F — export ownership

Using Candidate/Authored product state from the scenario, verify Studio-owned export:

```text
UTZ export
manifest/hash validation
UltraStar export
chart parse
exported audio decode
temporary/failed staging cleanup
```

Analysis Engine must not become the product export owner.

## Bubble G — lifecycle/recovery

Sequentially exercise representative cancellation/recovery:

```text
one compiled Workflow run cancellation
one conditional expert task cancellation or fake-worker cancellation seam
one OpenVINO model task cancellation if safely bounded and already accepted
```

Verify process-group reaping, no partial committed artifact, queue/history truth, and a subsequent small request succeeds.

## Decoupling gates

Must be zero:

```text
app-core/Cargo.toml -> uta-analysis-engine
app-core/Cargo.toml -> uta-runtime-manager
desktop/Cargo.toml -> either backend implementation crate
app-core/** -> uta_analysis_engine::
app-core/** -> uta_runtime_manager::
desktop/** -> uta_analysis_engine::
desktop/** -> uta_runtime_manager::
```

Desktop must not directly spawn `uta-analyze` or `uta-runtime`.

## Checks

Run focused package/local suites only. Do not run final Nix packaging or the full repository acceptance here.

`git diff --check` must pass after the scenario. This card should normally make no production-code changes.

## Durable completion update

Set card 20's current state/result in `tasks/remaining-models/STATE.md`. Update `docs/KEY_CONCLUSIONS.md` only if the E2E run changes a durable product/process-boundary conclusion. Do not create a completion log under `docs/`.

In the task handoff, summarize only the scenarios that materially passed/failed, whether any model boundary was live/replayed/fail-closed, process-boundary/cancellation/export conclusions, and the exact retained blocker if one remains.

A fully green **feature-integration** phase requires this card = `READY`. `READY` here means Studio/CLI/Engine/Editor/export composition is truthful and executable, including labeled replay/fail-closed seams at separately blocked live model boundaries. It does not promote or erase any model `production_ready=no` result.
