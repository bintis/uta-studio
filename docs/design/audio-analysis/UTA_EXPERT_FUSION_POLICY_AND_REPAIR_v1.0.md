# Uta! Studio — Expert Fusion Policy and Repair v1.0

Status: current design addendum

This document repairs the Processing Studio stage-3/stage-4 contract without changing the frozen Studio / Analysis Engine / Runtime Manager ownership boundaries. It supplements `UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md` for expert enablement, fusion-policy transport, fallback semantics, and final candidate selection.

## 1. Problem being corrected

The first four-stage Processing Studio implementation exposed whole-song owner buttons but did not preserve one coherent execution truth:

- Studio could persist `F0-derived` note lengths while the Engine v1 trust boundary rejected them;
- F0-derived regions were represented as synthetic GAME evidence;
- optional ROSVOT/STARS note results could appear in provenance without becoming candidate states;
- Basic Pitch onset support depended accidentally on Acoustic DSP;
- stage 4 could silently enable an expert disabled in stage 3;
- Plan Preview did not expose the resolved fusion policy;
- internal Fusion / Candidate Graph / Finalization nodes were presented as user-disableable cards even though they are required implementation stages.

These are contract defects, not cosmetic defects.

## 2. Product-stage ownership

### 2.1 Stage 3 — F0 & Singing Experts

Stage 3 owns which evidence producers may execute. Each expert has its own execution policy. Optional experts may be disabled independently, subject to the minimum valid evidence contract:

```text
at least one enabled continuous-F0 producer
```

Stage 4 must not silently change an expert from Disabled, Maximum-only, or disagreement-triggered execution to Always.

### 2.2 Stage 4 — Expert Fusion

Stage 4 owns exactly one product decision: `fusion_mode = algorithm | ai`. It does not own model installation, evidence-expert participation, continuous-F0 ownership, note-length ownership, onset ownership, or the internal execution DAG.

The Engine resolves continuous contour, semantic/fallback segmentation, and available onset/articulation context from the exact Stage-3 execution plan. The resolved policy is visible in Plan Preview but is not separately user-authored in Stage 4. An explicit AI choice is sticky execution intent: it must resolve exactly or block with a precise reason and must never silently select Algorithm.

## 3. Continuous F0 and semantic notes remain separate

A continuous-F0 expert supplies physical performance evidence:

```text
frequency_hz
voicing / observed coverage
confidence when genuinely available
```

It does not become frame-wise target MIDI.

GAME or another note expert may supply semantic note-region and fractional base-pitch proposals. The final target note is selected only after segment-level candidate construction and global decoding.

### 3.1 Engine-resolved F0-derived fallback

When no scheduled semantic note expert produces usable regions, the Engine may use an internal degraded fallback represented as typed F0-derived boundary evidence, never as `GameEvidenceV1`. This is resolved execution behavior, not a separate Stage-4 owner setting.

The fallback derives stable voiced regions from measured gaps, voicing transitions, and persistent material F0 discontinuities. One-frame noise, smooth glissandi without stable plateaus, and transitions across unvoiced gaps do not become cuts. For each already-formed region, the resolved continuous-F0 expert may contribute a separate segment-level pitch proposal. This does not redefine continuous PitchEvidence as target notes.

Every note selected from this fallback is marked uncertain and remains reviewable. Provenance records the F0 segmentation source and its dependency on the continuous-F0 expert.

## 4. Multi-source candidate graph

The selected note-length source supplies primary candidate regions. Enabled compatible note experts such as ROSVOT and STARS contribute challenger candidate states rather than provenance-only records.

```text
primary regions
+ challenger regions
+ pitch alternatives
+ alignment
+ onset / articulation context
+ technique context
→ multi-source candidate graph
→ decision mode: Algorithm = exact second-order coherent-path selection (default) | AI judgment = constrained external candidate selection
```

A challenger is not promoted merely because it exists. Primary regions receive a small structural prior. Challenger paths can win when their segmentation is supported by contextual evidence, such as a measured onset. Stable continuous F0 may add an explicitly typed `F0Consolidation` state spanning unsupported sequential primary fragments, but never across a word edge, caller boundary, measured attack, sustained pitch shift, unstable context, or unvoiced gap. The original primary states remain in the auditable pool. Raw model scores are never compared across models as if they were calibrated probabilities.

Unpitched challenger boundaries that have no local continuous F0 remain disagreement evidence and do not become invalid note states. Caller-authored hard boundaries are a normalized pool-level authority, carried identically to Algorithm and AI judgment, included in the pool digest, persisted with SingingAnalysis, and used by shared structural validation. Candidate-local context and voicing transitions may reset melody scoring but do not become structural barriers.

All candidate construction and decoding work is explicitly bounded: at most `100000` Candidate states after expansion, at most `64` distinct pitch proposals per duration state, at most `10000000` Candidate-to-boundary/word/technique evidence relations before metadata cloning and again after pitch-state expansion (including cloned nested evidence), at most `10000000` conservatively projected Candidate-to-local-F0/Acoustic/Basic-Pitch frame visits counted through sorted interval indexes, `65536` examined second-order pair states, and `2000000` examined pair transitions across the complete graph. The external AI request has an independent serialized limit of `8 MiB`. Reaching a documented limit succeeds; one unit beyond it fails closed.

## 5. Onset and acoustic evidence

Basic Pitch onset evidence is independent of Acoustic DSP. Every compatible source that Stage 3 actually schedules may contribute its own typed onset/articulation context to the shared Candidate Pool; Step 4 has no duplicate onset-owner control. Already-produced Basic Pitch or Acoustic context is not discarded asymmetrically by an obsolete ownership setting.

Source-local Basic Pitch activation is a versioned transition feature, not a calibrated cross-model probability.

## 6. Technique evidence

When STARS technique analysis is enabled, its source-local technique activations are retained as explicit candidate context with model identity and calibration label.

They may influence context-aware transition behavior, for example:

- strong vibrato/glissando evidence discourages an unsupported note split;
- conflicting strong technique activations create a review reason.

Source-local technique activations must not be copied into calibrated final `TechniqueScores`. Final calibrated technique semantics remain empty until a real calibration contract exists.

Disabling STARS note boundaries while leaving STARS technique enabled must not re-enable STARS note candidates. Note-boundary and technique participation are independently scheduled and independently recorded in provenance.

## 7. Typed process-boundary contract

The compiled `uta.workflow-execution` DTO carries a typed optional `fusion_policy`. Legacy node parameters remain only for backward-compatible validation. When both are present, the Analysis Engine trust boundary rejects any disagreement.

The exact Engine plan carries the resolved typed policy and Plan Preview renders it before queueing.

Required properties:

- independently declared DTOs on both sides of the CLI process boundary;
- deny-unknown-field validation;
- exact agreement with enabled stage-3 experts;
- deterministic participation in request fingerprinting through the compiled workflow extension;
- visible resolved policy in Plan Preview;
- no hidden Runtime Manager mutation or model installation.
- typed `fusion_mode` may select the default Algorithm decoder or the explicit AI-judgment selector defined by `UTA_AI_JUDGMENT_FUSION_MODE_v1.0.md`;
- AI judgment may only select verbatim Engine candidates and never manufactures measured evidence;
- Runtime Manager owns `tool:fusion_agent_adapter` path/readiness; the workflow/AnalyzeRequest does not carry a raw adapter executable path;
- AI-judgment failure is a hard failure and never silently falls back to Algorithm.

## 8. Processing Studio presentation

Stage 4 presents user decisions and one managed final-analysis summary. It does not render required internal nodes as disabled-looking cards.

Recommended structure:

```text
Decision mode: Algorithm | AI judgment
Configured / Potential evidence inputs
Managed final analysis
```

Exact Resolved/Participating evidence and internal fallback decisions belong to Plan Preview after compilation, not to duplicate Stage-4 owner controls.

The managed summary may name the internal order:

```text
canonical evidence
→ source-aware expert fusion
→ multi-source candidate graph
→ selected decision mode (exact Algorithm path decoder by default, or explicit constrained AI judgment)
→ canonical singing track
```

Implementation details may be inspected in the advanced DAG and exact Plan Preview, but they are not independent user-disableable choices.

The default managed final analysis remains Algorithm. When the user explicitly selects AI judgment, the Stage-4 card may expose that decision mode, adapter readiness, and the network/candidate-metadata disclosure specified by `UTA_AI_JUDGMENT_FUSION_MODE_v1.0.md`; it still does not expose internal candidate-graph stages as draggable processors.

## 9. Fail-closed rules

The workflow or request must be rejected when:

- no continuous-F0 expert remains enabled;
- a required resolved F0, semantic-note, Acoustic DSP, or Basic Pitch source is not enabled with the required Stage-3 execution policy;
- typed fusion policy and legacy compiled parameters disagree;
- the selected primary evidence source produces no usable evidence;
- candidate construction produces no valid note states;
- evidence contains non-finite values, invalid timelines, or incompatible identities.
- AI judgment is selected but `tool:fusion_agent_adapter` is unresolved/unusable, the adapter fails/times out, or its response contains anything other than a valid subset of the real candidate pool.

Missing optional challenger evidence produces an explicit degraded result when the baseline remains valid.

## 10. Versioning

The initial repair baseline advanced algorithm identities to `fusion-v5` and
`hsmm-v4`. The 2026-08-27 implementation closure subsequently changed
candidate semantics again: calibrated boundary support, correlation-discounted
context constraints, explicit decision traces, F0-derived provenance repair,
and the duration/context decoder all participate in the fingerprint. The
current completion passes additionally promote segment-level peer-expert pitch
proposals into real global decoder states, emit strict UTZ VocalChart 0.3 bytes,
add source-local expressive acoustic continuity observations, and make the
melody path score persistence-, fragmentation-, short-event-, typed soft
phrase-start-, and octave-return-aware without rewriting continuous F0. The
current closure additionally introduces persistent/hysteretic F0 shifts, typed
F0-consolidation candidates, collision-resistant pitch-state identity,
target-relative peer support, bounded endpoint-indexed second-order decoding,
a first-class hard-boundary pool shared by both selectors, confidence-weighted
phrase-start relaxation of melody/octave priors only, target-relative Acoustic
fundamental support, and indexed hard-edge traversal.
The external Fusion Agent request protocol is version 3 because candidates now
carry those expanded typed semantics under the complete pool contract. The
accepted current identities are therefore:

```text
acoustic-dsp-v2
fusion-v16
hsmm-v15
fusion-agent-protocol-v3
finalize-vocal-chart-v3
```

Intermediate identities must not be reused for these semantics. Serialized
canonical evidence adds backward-compatible optional/default fields for
boundary candidate role, calibrated boundary support, decision trace, and
source-local technique context. Legacy GAME field names are accepted only as
deserialize aliases and are not emitted for new artifacts.

## 11. Required regression coverage

The implementation must keep tests for:

- Studio-generated F0 fallback accepted by packaged `uta-analyze` validate / requirements / plan;
- exact Plan Preview fusion policy;
- no silent stage-3 expert enablement from stage 4;
- last continuous-F0 expert cannot be disabled;
- F0-derived evidence is not serialized as GAME evidence;
- Basic Pitch onset works without Acoustic DSP;
- ROSVOT/STARS note results create real challenger candidates;
- contextual onset evidence can select a challenger segmentation path;
- STARS technique context remains source-local and uncalibrated;
- technique-only STARS execution does not contribute STARS note candidates;
- disabled optional experts do not remain active in the compiled graph;
- deterministic fingerprint and Candidate/Authored separation remain intact;
- one-frame F0 octave noise creates neither a fallback split nor a contextual discontinuity;
- unsupported sequential fragments can be consolidated only with stable measured F0 and without crossing a word, attack, caller boundary, sustained shift, or unvoiced gap;
- exact pitch-state identities cannot collide after punctuation normalization or MIDI rounding;
- decoder pair-state/transition bounds succeed at the documented limit and fail one state or transition above it;
- caller-hard boundaries have identical Algorithm/AI request, digest, persistence, and replay-validation meaning.

## 12. Deferred work

This repair does not claim completion of:

- a general user-authored per-window rules language;
- cross-model confidence calibration where models do not expose calibratable scores;
- learned dynamic weights;
- a fully probabilistic HSMM observation model;
- multi-F0 or simultaneous-singer semantic tracks;
- runtime/self-promotion without an explicit versioned catalog release decision. The current repository-owner policy has explicitly admitted every packaged model's effective non-CPU route; that release decision does not turn advisory quality/calibration/provenance/license caveats into calibrated evidence.

Those require separate versioned contracts and validation data. The current implementation must remain truthful about these limits.