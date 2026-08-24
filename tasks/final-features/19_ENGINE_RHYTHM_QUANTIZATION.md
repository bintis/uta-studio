# 19 — Engine Rhythm Quantization Contract Closure

**State:** `READY`
**Precondition:** model cards 01–13 are terminal and no machine-level safety stop is active. Production-only model blockers do not block this CPU-only symbolic feature card.
**Task class:** CPU-only deterministic symbolic processing
**Owner:** Analysis Engine; Studio owns user-facing edit/export UX

## Read

```text
AGENTS.md
AGENTS.md
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

## Current result

**State:** `READY`

Engine quantization remains in final-v1 as the real optional `rhythm.quantize` symbolic stage. The reconciled DAG and capability semantics are:

```text
fusion.candidate_graph: canonical_singing_track
  -> rhythm.quantize: quantized_canonical_singing_track
  -> finalize.vocal_chart: candidate_vocal_chart
```

The selected single-output contract keeps `SingingAnalysis` and all raw evidence unquantized while finalizing the optionally quantized track into the one Candidate VocalChart. Candidate chart provenance and `AnalysisDiagnosticsV1.quantization` both carry a typed `QuantizationReportV1`; app-core owns an independent wire DTO and rejects missing, unsolicited, mismatched, malformed or artifact-less reports across the `uta-analyze` process boundary. Authority remains `Candidate`; Authored revisions are never read or overwritten by this stage.

The actual fingerprinted algorithm is `rhythm-grid-dp-v1`. BPM means quarter-note beats per minute; the Studio toggle supplies an explicit sixteenth-note grid only when a Full Candidate chart is requested and the song has a valid explicit BPM. The grid is anchored to canonical time zero. A deterministic dynamic program chooses globally ordered non-overlapping ranges, resolves equal-cost ties toward the earlier range, requires one full grid step of duration, preserves positive rests, confines all endpoints to the authorized source timeline, and refuses to move an endpoint across—or away from an exact—caller hard boundary. Missing BPM/grid, missing Candidate output, impossible hard constraints, source escape and arithmetic overflow fail explicitly without mutating the input.

Only Candidate note start/end ranges can change. Global and note-local continuous F0, pitch bends, raw GAME/STARS/ROSVOT/alignment evidence, the unquantized SingingAnalysis, and disabled-stage timing remain unchanged. Quantization report identity and `QUANTIZATION_VERSION` participate in Candidate provenance, result provenance and the Engine execution fingerprint.

Settings > Analysis now exposes the persisted `Quantize candidate notes` switch. It maps through Global/Song/Run resolution to `analysis.enable_quantization`; Plan Preview blocks a quantized request without explicit song BPM. The separate Editor Quantize command remains an explicit human operation on the Authored/working-copy document and does not duplicate the Engine implementation.

CPU acceptance covers deterministic expected geometry, half-grid tie handling, minimum duration, positive rests, source bounds, hard-boundary refusal with no mutation, missing context, non-overlap, exact continuous-evidence preservation, raw SingingAnalysis versus quantized Candidate publication, Candidate authority, algorithm-version fingerprint changes, Planner ordering, capability semantic types, typed backend/local result validation and a real `uta-analyze` Preview/Plan round trip with the compiled Workflow extension. Analysis Engine and app-core suites pass, including stdout-pure CLI tests. Desktop compiles without a backend implementation dependency.

The process-boundary scan is clean: app-core/desktop contain no `uta_analysis_engine::` or `uta_runtime_manager::` imports, neither Studio crate depends on those backend crates, Desktop has no direct `uta-analyze`/`uta-runtime` process launch, and machine-protocol tests remain stdout-pure. No model, GPU, Vulkan or OpenVINO execution was used for card 19.
