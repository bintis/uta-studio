# 19 — Engine Rhythm Quantization Contract Closure

**Precondition:** model cards 01–13 are terminal and no machine-level safety stop is active. Production-only model blockers do not block this CPU-only symbolic feature card.
**Task class:** CPU-only deterministic symbolic processing
**Owner:** Analysis Engine; Studio owns user-facing edit/export UX

## Read

```text
AGENTS.md
docs/agent-tasks/MODEL_GPU_WORK_POLICY.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
tasks/final-features/19_ENGINE_RHYTHM_QUANTIZATION.md
analysis-engine/src/contract/request.rs
analysis-engine/src/planner/plan.rs
analysis-engine/src/artifact/vocal_chart.rs
```

## Problem

The request contract exposes:

```text
analysis.enable_quantization
musical_context.bpm / time_signature
```

and the Planner currently contains an optional `rhythm.quantize` placeholder, but the capability is not implemented and the current node ordering/output contract is incomplete.

This card closes the feature truthfully rather than leaving an inert request knob.

## Semantic rules

Quantization is a transformation of **symbolic Candidate note timing**, not continuous pitch evidence.

Never quantize or overwrite:

```text
RMVPE continuous F0
raw GAME/STARS/ROSVOT boundary evidence
alignment raw evidence
Authored chart revisions
```

The unquantized SingingAnalysis/evidence remains available for review.

Quantization may affect Candidate note start/duration only when the request explicitly enables it and sufficient musical context exists.

Do not guess BPM. If quantization is requested without a valid tempo/grid contract, fail/degrade explicitly according to the chosen versioned contract rather than using 120 BPM or another hidden default.

## Required design

Reconcile the current Planner stage with actual execution semantics. Preferred pipeline:

```text
candidate graph / canonical singing candidates
  -> optional rhythm.quantize symbolic stage
  -> finalize Candidate VocalChart
```

If the implementation instead keeps both unquantized and quantized Candidate artifacts, define the result fields/media types explicitly and mirror them in Studio local wire DTOs. Do not leave a Planner output semantic type that cannot be represented by the result contract.

The final implementation must make these three things consistent:

```text
Planner DAG
capability registry semantic types
AnalysisResult artifact contract
```

## Algorithm requirements

Implement a deterministic, versioned grid quantizer based on explicit musical context.

At minimum define and test:

```text
beat duration from BPM
subdivision/grid policy
rounding/tie policy
minimum note duration
non-overlap invariant
song/source timeline bounds
handling of notes near grid boundaries
handling of rests/gaps
handling of hard boundary/manual constraints
```

Do not silently cross hard caller/manual boundary constraints.

Quantization version must participate in Engine provenance/fingerprint. Existing `QUANTIZATION_VERSION` should represent the actual algorithm version, not a placeholder.

## Candidate authority

Any quantized output remains Candidate authority. Studio may compare/merge/accept it into Authored via explicit authoring commands. Re-analysis/quantization never overwrites Authored.

## Studio/process boundary

Studio may expose the existing `enable_quantization` intent and render the resulting Candidate timing. It must not run the quantization algorithm in app-core/desktop as a second backend implementation.

Editor interactive quantize commands may continue to exist as explicit human authoring operations; they are conceptually separate from Engine Candidate quantization and operate on Authored/working-copy state through existing editor commands.

## Tests

CPU only. Required tests:

```text
quantization disabled -> bit-for-bit symbolic timing preserved
valid BPM/grid -> deterministic expected timing
no BPM when required -> explicit fail/degraded contract
hard boundary constraint not crossed
notes remain ordered/non-overlapping/positive duration
continuous F0 byte/semantic values unchanged
SingingAnalysis retains unquantized evidence where contract says it should
Candidate authority remains Candidate
same request/fingerprint inputs -> same output
quantization version changes fingerprint when algorithm version changes
wire DTO/result validation across uta-analyze process
```

## Capability gate

Set:

```text
rhythm.quantize implementation_exists=true
```

only when the Planner invokes a real deterministic stage and result semantics are representable/validated.

If design reconciliation concludes Engine quantization should not exist because final-v1 defines quantization solely as an Editor authoring command, retire the Engine request knob/node/capability coherently rather than leaving a false placeholder. Record that durable decision in `tasks/remaining-models/STATE.md` and `docs/KEY_CONCLUSIONS.md`; all request/UI surfaces must stop advertising Engine quantization. Do not half-implement both interpretations.

## Durable completion update

Set card 19's current state/result in `tasks/remaining-models/STATE.md`. Update `docs/KEY_CONCLUSIONS.md` with the chosen durable quantization contract. Do not create a completion log under `docs/`.

Include chosen contract, algorithm/version, Planner/result changes, CPU test matrix, and process-boundary scan.

Stop after this card.
