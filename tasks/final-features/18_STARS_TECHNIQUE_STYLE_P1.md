# 18 — STARS Technique / Style P1

**Precondition:** Phase A model cards 01–13 are terminal and card 12 reports `integration_ready=yes`. Card 14 may be `SKIPPED_PRECONDITION` for unrelated Production-only blockers.
**Task class:** one-model feature extension; OpenVINO only; no Vulkan
**Resource:** accepted STARS Chinese generation from card 12
**Owner:** STARS native/OpenVINO backend + Analysis Engine typed evidence/Fusion + Studio read-only evidence presentation

## Read

```text
AGENTS.md
docs/agent-tasks/MODEL_GPU_WORK_POLICY.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
tasks/final-features/18_STARS_TECHNIQUE_STYLE_P1.md
docs/KEY_CONCLUSIONS.md
tasks/remaining-models/STATE.md
```

Consult `docs/agent-tasks/STARS_IR_CONVERSION_AGENT_RUNBOOK.md` selectively for technique/style/VQ sections only. Do not reread the entire runbook unless a concrete implementation question requires it.

## Goal

Complete the final-v1 technique-evidence design using the already-audited STARS architecture without reopening P0 note/alignment work.

Target semantic role:

```text
STARS = lyric/phoneme-conditioned singing expert
  P0: alignment / note boundary / note pitch evidence
  P1: technique evidence and optional style/global attributes
```

## Non-negotiable semantics

Technique is evidence, not note segmentation authority.

Examples such as:

```text
vibrato
glissando
ornament / melisma-related technique
airy/breathy or other upstream-defined technique classes
```

must not automatically create extra MIDI notes or rewrite GAME boundaries.

Raw STARS technique/style logits are not calibrated correctness probabilities. Preserve raw/source-local scores and explicit calibration state.

STARS remains dependency-correlated with RMVPE and transcript/phoneme/alignment inputs. Every technique artifact must preserve those dependency IDs so Fusion does not count it as an independent confirmation of conditioning evidence.

## Backend architecture

Reuse the source-verified STARS generation, native frontend, bucket strategy, host Viterbi/regulation/grouping, runtime identity, and worker protocol proven by card 12.

Do not export a monolithic upstream forward. Add only the tensor-only technique/style neural island(s) required by the pinned checkpoint/config and keep deterministic host logic outside IR.

Training-only VQ behavior must remain disabled/frozen. Checkpoint `global_steps` behavior remains pinned from provenance; do not expose it as a runtime knob.

## Typed output

Define/extend a versioned STARS evidence contract that can express technique evidence without forcing it into Candidate notes.

Required properties:

```text
canonical timeline or exact source interval
raw logits/source-local scores where available
class/taxonomy identity
model generation
runtime generation
checkpoint/source hashes
dependencies/correlation groups
calibration status
```

Style/global attributes, if implemented, must have a separate typed scope from per-note/per-frame technique evidence.

Do not make a global style label look like a per-note technique.

## Analysis Engine / Fusion

Wire real technique evidence into the existing canonical singing/evidence domain.

The Engine may project source-local technique evidence onto Candidate note ranges for review/display, but it must preserve the original raw evidence and provenance.

`technique.analyze implementation_exists=true` only after:

```text
real STARS technique worker output exists
backend parser validates it
Fusion/canonical track can carry it without fabricating confidence
Review/evidence output exposes it
Engine::analyze() calls the stage when requested/reachable
```

Do not set the flag because the checkpoint merely contains technique head weights.

## Studio

Studio/app-core only consumes the typed Engine artifact through its local wire/artifact DTOs. It must not load STARS, interpret raw tensors, or reproduce technique classification logic.

Editor Evidence Workbench should expose technique layers read-only and allow explicit suggestion/authoring actions through existing commands where supported. Re-analysis must not overwrite Authored technique edits.

## Validation

One STARS workload at a time, OpenVINO only. Stop on any GPU/display instability.

Required:

```text
PyTorch/reference technique/style output baseline for owned fixture(s)
ORT parity for new neural island(s)
OpenVINO CPU parity
bounded Intel GPU parity
finite raw outputs
taxonomy/index mapping exactness
no technique-induced extra MIDI note regression
dependency/correlation metadata preserved
Engine typed artifact/result validation
fresh-process repeat/cancellation cleanup
```

Do not require an unversioned heuristic probability calibration to call the technical path ready. Unknown calibration remains explicit.

## Capability gate

Only after real execution/wiring:

```text
technique.analyze implementation_exists=true
```

If style has no current public capability, keep it as a typed optional evidence field/artifact rather than inventing an unrelated capability name without design need.

## Durable completion update

Set card 18's current state/result in `tasks/remaining-models/STATE.md` and update `docs/KEY_CONCLUSIONS.md` with any durable STARS technique/style, dependency or calibration conclusion. Do not create a completion log under `docs/`.

Include graph/artifact hashes, taxonomy, parity summary, dependency contract, capability state, and any retained calibration/license blocker.

Stop after this card and reap all STARS/OpenVINO processes.
