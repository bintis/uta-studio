# 20 — Product E2E Feature Bubble

**State:** `READY`
**Precondition:** cards 15–19 and 20A = `READY`; Phase A model cards are terminal, while Production-only model blockers may remain explicitly retained
**Task class:** validation-only integration smoke; fix nothing broad in this card
**GPU:** OpenVINO live workloads only; Vulkan remains unauthorized

## Read

```text
AGENTS.md
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

Using short owned/authorized stereo fixtures and already-accepted model generations, run one workload at a time and verify the truthful final-v1 lane set:

```text
Original Mix
 -> vocal extraction
 -> Lead + VocalResidual

Original Mix
 -> Instrumental
```

Verify independent BackingVocal/HarmonyVocal requests fail closed as future capability work, and that Processing Studio does not advertise `audio.lead_partition` as an executable Backing/Harmony splitter. Editor Backing/Harmony chart roles remain independent authoring semantics.

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
manifest/file-set/byte-size/semantic validation
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

## Current result

**State:** `READY`

Bubbles A–C passed. Real `uta-analyze` process tests preserved immutable Workflow snapshot/digest identity, legal reorder/duplicate behavior, invalid graph rejection, exact analyzer binding, disabled/conditional policies and scheduler-owned `MaximumOnly` / disagreement behavior. Semantic lane fixtures preserved timeline/audio typing, atomic FLAC publication and `LeadVocal + VocalResidual`; Backing/Harmony audio remained fail-closed. Candidate/Fusion fixtures preserved dependency correlation, unknown confidence, continuous F0, uncalibrated STARS evidence, read-only technique projection and symbolic-only quantization.

The actual Runtime Manager inventory was read through machine JSON with all required model identities present. OpenVINO resources were installed/usable under their recorded states. The five RoFormer entries in the default inventory were legacy Vulkan resources and therefore unusable/fail-closed; no new Vulkan execution was authorized or attempted. Their live audio behavior was labeled replay from the accepted card-14 serial model bubble, while this card exercised product semantic topology with deterministic native fake-worker fixtures. No OpenVINO/Level Zero/GPU context or model inference was created.

Bubble D's focused Editor/Workbench/UI actions passed. The initial Studio diagnostic found that a USDX song's indexed chart `.txt` path was incorrectly treated as original audio. The focused fix now resolves the authorized USDX-declared audio instead; an isolated non-audio-source regression test proves ffmpeg failure leaves source bytes untouched and removes partial preview output. The rerun loaded the real editor-ready chart and decoded/prepared all three Editor roles—instrumental, original and vocals—through ffmpeg and native GStreamer. Studio diagnostics now checks every exposed Editor audio role rather than accepting instrumental-only success.

Bubble E used supplied canonical Chinese `月远` and English `sing now` requests. Both crossed the real standalone `uta-analyze validate` and `plan` process boundary and produced Candidate/Pitch/SingingAnalysis/Alignment plans without requesting ASR. This was a live control-plane scenario with the Qwen boundary avoided by canonical input, not a new live Qwen acceptance.

Bubble F passed from real Authored product state: validated UTZ with two decoded audio assets, one vocal track, 587 notes and 34,791 pitch frames, plus a parsed UltraStar chart with two decoded audio assets. Existing-target overwrite refusal passed and the unique temporary export directory was removed.

Bubble G passed focused fake-worker failure/cancellation coverage: protocol pollution and correlation failures fail closed, cancellation is request-correlated, stalled decoder/worker process groups are killed and reaped, staged Workflow and Candidate results are discarded, and pre-cancelled requests perform no resource or output work. The previously accepted card-18 OpenVINO cancellation evidence remains replay-only. No worker, ffmpeg, partial compatibility preview or temporary export remained.

Focused app-core/Desktop/diagnostics checks, process-boundary scans, line limits and `git diff --check` pass. The 21B affected semantic-audio/Candidate rerun also passed through the complete Analysis Engine CPU/fake suite and complete app-core suite: exact Plan quality gates produced typed results, cleanup damage fell back explicitly, and required quality failures remained fail-closed. Card 21 revision 2 then reran the affected suites after removing hash-based acceptance/rejection while preserving path, regular-file, file-set, byte-size, schema, semantic, correlation and atomic-publication checks. Separately blocked model promotion states remain unchanged and do not weaken this feature-integration result.
