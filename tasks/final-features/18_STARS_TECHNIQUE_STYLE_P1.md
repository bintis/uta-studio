# 18 — STARS Technique / Style P1

**State:** `READY`
**Precondition:** Phase A model cards 01–13 are terminal and card 12 reports `integration_ready=yes`. Card 14 may be `SKIPPED_PRECONDITION` for unrelated Production-only blockers.
**Task class:** one-model feature extension; OpenVINO only; no Vulkan
**Resource:** accepted STARS Chinese generation from card 12
**Owner:** STARS native/OpenVINO backend + Analysis Engine typed evidence/Fusion + Studio read-only evidence presentation

## Read

```text
AGENTS.md
AGENTS.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
tasks/final-features/18_STARS_TECHNIQUE_STYLE_P1.md
docs/KEY_CONCLUSIONS.md
tasks/remaining-models/STATE.md
```

Consult current worker/conversion source plus `docs/research/non-game-model-readiness/OPTIONAL_EXPERTS.md` selectively for technique/style/VQ details when a concrete implementation question requires it.

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

## Current result

**State:** `READY`

The exact checkpoint and pinned upstream source were converted into a five-island P1 package and imported through Runtime Manager:

```text
upstream revision: f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167
checkpoint:        9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c
manifest:          37036e2273ca633f95263b45ca8f2f60652858b8a5db0d03bf85c87a593bef9e
installed gen:     6317f593d745571e7dc69226317e72c60d7be8745064d021c4516ed72cd484b2
conversion recipe: b2d2c9918704c545a9d0ea86524c02f1c790c4ca9f995f8c32b5d71ea6596e1f
Stage D XML/BIN:   eb442efb785e1ce71dd3909d8229daf31ec956817343cfee47d5327eb6634c33 /
                   1b74c51ef8fde04bb1e669644f356e275c5ec25983bb1c5674dd3e1feb05609d
Stage E XML/BIN:   6e29804ea2d46b10769c0a50fb3fc90bde35a5d555f436f950cb307afc6c7dae /
                   d66f8fd79ffb67826a48d86ad9105f0f3c1f53334d7347b24fb8dffb1ff897c3
```

Stage D produces sentence-conditioned frame features, technique attention and seven separately scoped segment-global style heads. Native host code aggregates Stage-D features over deterministic Viterbi phoneme intervals; fixed-bucket Stage E produces all nine technique logits: `bubble`, `breathe`, `pharyngeal`, `vibrato`, `glissando`, `mixed`, `falsetto`, `weak`, and `strong`. The contract preserves raw logits and labels sigmoid scores `source_local_sigmoid_uncalibrated`; style heads remain uncalibrated categorical logits. Checkpoint `global_step=200000` is pinned and no VQ runtime control is exposed.

PyTorch export/reference, ORT CPU and OpenVINO CPU parity cover Stages A–E. ORT CPU reached maximum absolute error `7.05719e-5` / relative L2 `1.80397e-6`; OpenVINO CPU reached `8.39233e-5` / `2.34483e-6`. A real one-second CPU run produced 188 valid frames, 2 unchanged note intervals, 10 phoneme technique intervals and 1 global style interval; fresh-process repeat was identical, active cancellation left no artifact or worker, and enabling technique did not change note boundaries or MIDI semantics.

After explicit user authorization, system inventory selected the sole discovered Intel OpenVINO device, `GPU.0 = Intel Arc B580`, rather than a fixed device index. Bounded OpenVINO parity ran Stages A/D/E on that Intel GPU and accepted maximum errors below `3.1e-5`. The product staged worker then ran one second with A/D/E on the discovered Intel GPU and dynamic B/C on CPU, reporting `openvino_gpu_cpu_staged`; CPU/GPU note boundaries, MIDI semantics and technique interval scopes matched exactly, while 579 raw evidence values differed by at most `3.0e-5`. The process exited cleanly, source bytes and boot identity were unchanged, and no matching Intel GPU fault/reset record appeared.

`notes.stars` and `technique.analyze` are now real capabilities. Engine persistence retains shared-frontend, annotation-RMVPE, TimedTranscript, Chinese-G2P, checkpoint, generation and correlation provenance. Studio exposes technique as an explicitly uncalibrated read-only evidence strip; it cannot create, split, move or replace GAME notes. The effective OpenVINO route is policy-admitted as `ProductionPinned`. Unresolved checkpoint license identity and broad quality/calibration limits remain explicit advisory caveats; they do not create an alternate backend or automatic fallback.
