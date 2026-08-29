# 21I — Step 4 Final Fusion Policy Convergence

**State:** `READY`

**Parent:** Card 21 final design-parity audit

**Task class:** product/workflow/Engine convergence; focused source/test/UI cleanup

## Mission

Simplify Processing Studio Step 4 so it expresses only the product-level decision the user actually owns:

```text
Final candidate-path selector

Algorithm
or
AI judgment
```

Stage 3 remains the product surface for deciding which evidence experts may participate and under what execution policy.

Step 4 MUST NOT ask the user to manually choose Engine-internal fusion ownership such as:

```text
pitch_owner
boundary_owner
onset_owner
```

Those are algorithm implementation details and must be resolved inside Analysis Engine from the available evidence, with truthful provenance explaining what the Engine actually used.

The target product model is:

```text
Stage 3
    decides which evidence experts may run
        |
        v
Engine builds one deterministic candidate pool
        |
        +-------------------+
        |                   |
        v                   v
 Algorithm selector     AI judgment selector
        |                   |
        +---------+---------+
                  |
                  v
        Canonical Singing Track
```

Algorithm and AI judgment should receive the same Engine-produced candidate pool. The only product-level difference in Step 4 is which selector chooses the final valid candidate path.

---

# 1. Current problem

The current Step 4 UI presents four visually similar groups:

```text
Decision Mode
Continuous F0
Note Lengths
Onset Support
```

This creates the mental model that the user manually assigns ownership of pitch, duration and onset before asking the Engine to perform fusion.

That is not how the implementation actually behaves.

The current source already contains a real candidate-fusion system where multiple evidence sources can generate primary and challenger candidate states and the final selector can choose a challenger. Therefore the current Step 4 ownership controls are both conceptually redundant and, in several places, misleading.

Current product parameters live on `evidence_fusion`:

```text
fusion_mode
pitch_owner
boundary_owner
onset_owner
```

Only `fusion_mode` should remain user-authored workflow intent.

---

# 2. Product ownership after this change

## Stage 3 owns evidence participation

Stage 3 continues to control whether an expert is:

```text
Always
Disabled
Maximum only
On disagreement
Disagreement windows
```

Examples:

```text
RMVPE
FCPE
GAME
Basic Pitch
ROSVOT
STARS
Acoustic DSP
```

These controls answer:

> Which evidence producers may participate, and when may the Engine run them?

They do NOT answer which producer is guaranteed to own the final note.

## Step 4 owns final selector choice only

Step 4 answers exactly one question:

> Which mechanism chooses the final valid path through the Engine-produced candidate pool?

Valid choices:

```text
Algorithm
AI judgment
```

Algorithm remains the default.

AI judgment remains explicit, non-default, requires a usable Runtime Manager `tool:fusion_agent_adapter`, and never silently falls back to Algorithm.

## Analysis Engine owns fusion policy

Analysis Engine owns:

- how RMVPE and FCPE are compared and used;
- how canonical continuous F0 is selected or fused;
- how primary segmentation candidates are created;
- when GAME evidence is primary, supporting, optional or absent;
- when F0-derived segmentation is required as degraded fallback;
- how alignment, F0 transitions, Basic Pitch, Acoustic DSP, ROSVOT and STARS produce contextual/challenger evidence;
- how onset evidence changes candidate generation or transition utility;
- how disagreement and uncertainty are represented;
- how the deterministic Algorithm selector scores and chooses a globally coherent path;
- what provenance is emitted for the final decision.

These rules should be versioned Engine behavior, not user-authored workflow parameters.

---

# 3. Current UI/code mismatches that must be removed

## 3.1 `NOTE LENGTHS -> GAME` is not final ownership

Current UI can display:

```text
NOTE LENGTHS
✓ Note regions -> GAME
```

But the actual Engine uses GAME as a primary/base boundary source while still constructing challenger segmentation candidates from sources including:

```text
ROSVOT
STARS
alignment word boundaries
F0 transitions
Acoustic onset
Basic Pitch onset
hard/context constraints
```

A challenger can win the final candidate-path decode.

There is already an Engine test proving contextual onset evidence can promote a challenger segmentation path over a wider primary candidate.

Therefore the UI phrase `NOTE LENGTHS` / `owner` incorrectly suggests a guarantee the Engine does not provide.

### Required change

Remove the user-selectable Step 4 Note Lengths control.

If Step 4 shows segmentation information at all, it must be read-only descriptive information such as:

```text
Candidate segmentation is resolved by the Engine from participating boundary evidence.
```

Do not expose a second user authority duplicating the Stage 3 GAME execution policy.

---

## 3.2 Stage 3 GAME and Step 4 boundary owner currently duplicate authority

Current app-core behavior has an explicit cross-stage transaction:

```text
GAME Disabled
    -> boundary_owner = f0

GAME Always
    -> boundary_owner = game
```

Step 4 then only permits states that agree with that Stage 3 setting.

This means the Step 4 Note Lengths buttons do not represent an independent product choice. They merely re-display a state already controlled by Stage 3.

### Required change

Remove this dual-authority model.

Stage 3 remains the user control for GAME participation.

Engine internally decides the usable segmentation baseline/fallback from the actual planned/executed evidence.

Do not persist a user-authored `boundary_owner` solely to mirror whether GAME is enabled.

---

## 3.3 `CONTINUOUS F0 -> RMVPE/FCPE` is not final MIDI ownership

The current Continuous F0 selection does affect real behavior:

- canonical continuous F0 curve;
- pitch bend source;
- F0-derived segmentation when needed;
- F0 transition context evidence.

However the final discrete `CanonicalNote.midi_note` may still come from another real candidate state.

For example, GAME may supply a fractional note proposal and RMVPE/FCPE may create alternative pitch states. The candidate graph can select a pitch alternative whose `target_pitch_source` differs from the continuous F0 source.

Therefore a user-facing `F0 -> RMVPE` control looks too much like final pitch ownership.

### Required change

Remove the user-selectable Step 4 Continuous F0 owner.

The Engine must internally determine/fuse its canonical continuous contour according to a versioned algorithm policy using the evidence that actually exists.

If an initial implementation still uses RMVPE as the default baseline and FCPE as challenger, keep that as an Engine algorithm policy, not workflow user intent.

---

# 4. Real correctness bug — FCPE selection currently leaks RMVPE semantics

This must be fixed even if the user-facing owner controls are removed.

Current code can use FCPE as the selected continuous F0 owner, but canonical decision metadata and uncertainty still contain RMVPE-biased logic.

Examples in current `fusion/canonical.rs` include:

```text
continuous_f0_source inferred from whichever evidence fields happen to exist
```

with RMVPE checked first, and uncertainty calculations that directly use:

```text
rmvpe_voiced_ratio
rmvpe_pitch_mad_cents
```

This can produce a contradictory result:

```text
actual canonical F0 curve = FCPE
but
trace says continuous_f0_source = RMVPE
or
uncertainty is determined by secondary RMVPE quality
```

### Required fix

Do not infer the selected/internal continuous contour policy from evidence presence.

Carry explicit Engine-resolved contour provenance into canonical construction.

Whichever Engine policy resolves the canonical continuous contour must truthfully drive:

- decision trace `continuous_f0_source` or successor field;
- low pitch coverage calculation;
- pitch instability calculation;
- any source-specific quality/degradation decision.

If the Engine later constructs a genuinely fused contour rather than selecting one model, provenance should be able to say that truthfully instead of forcing `rmvpe` or `fcpe`.

### Acceptance

Add tests covering at least:

```text
FCPE primary/resolved contour + RMVPE secondary evidence present
-> trace does not falsely report RMVPE as selected continuous F0
-> uncertainty is based on the resolved contour policy, not hard-coded RMVPE fields
```

---

# 5. Onset support must become Engine behavior, not user ownership

Current Step 4 exposes:

```text
Onset -> Automatic
Onset -> Acoustic DSP
Onset -> Basic Pitch
```

This is misleading because onset evidence is contextual evidence inside candidate generation/selection, not a final authored owner.

Current `automatic` behavior is effectively closer to:

```text
use available onset evidence
```

rather than selecting one provider.

### Required change

Remove the user-selectable Step 4 Onset Support control.

When Stage 3 evidence exists and the exact plan executes it, Engine fusion should use relevant validated evidence according to its versioned algorithm policy.

For example, when available:

```text
Acoustic DSP context
Basic Pitch onset/note/contour context
alignment boundaries
F0 transitions
technique continuity
```

may all affect candidate generation or transition utility without the user preselecting one owner.

---

# 6. Real correctness/semantic bug — current onset modes are asymmetric

Current code treats the explicit onset modes asymmetrically.

`onset_owner = acoustic` currently effectively removes Basic Pitch from fusion entirely:

```text
basic_pitch_for_fusion = None
```

That removes more than Basic Pitch onset evidence; it can also remove its note/contour context even if Stage 3 actually ran Basic Pitch.

By contrast, `onset_owner = basic_pitch` keeps Acoustic DSP evidence but suppresses only acoustic onset contribution.

This contradicts the UI concept of selecting only an `ONSET SUPPORT` source.

### Required fix

As part of removing user-authored onset ownership, eliminate this asymmetric evidence deletion.

Stage 3 determines whether evidence exists.

Engine fusion determines which dimensions of that evidence are relevant.

Do not discard a whole already-produced evidence object merely because one onset-specific policy does not prefer that source.

### Acceptance

Add tests proving that when both Acoustic DSP and Basic Pitch actually participate:

- available non-onset context is not silently dropped by an onset policy;
- candidate construction can retain both real evidence sources where semantically relevant;
- final Algorithm and AI modes receive the same candidate pool for the same exact workflow/evidence execution.

---

# 7. Candidate pool must be selector-independent

This is a central acceptance invariant.

For the same exact queued workflow/request and the same completed evidence execution:

```text
Algorithm mode
AI judgment mode
```

must receive the same Engine-produced candidate pool.

The selector may differ; candidate construction must not.

Target architecture:

```text
Evidence execution
    -> deterministic normalization
    -> deterministic candidate construction
    -> candidate pool digest
        -> Algorithm selector
        OR
        -> AI adapter selector
```

Do not make AI/Algorithm mode alter F0 ownership, segmentation ownership, onset gating or candidate generation rules.

### Acceptance

Add a focused test that builds the same evidence fixture under both decision modes and proves the pre-selection candidate-set digest is identical.

---

# 8. Workflow contract convergence

## User-authored workflow target

The product-level Step 4 workflow intent should converge to:

```text
fusion_mode = algorithm | ai
```

Remove user ownership of:

```text
pitch_owner
boundary_owner
onset_owner
```

from the canonical authored workflow contract.

## Migration

Existing saved workflows may contain these parameters.

Migration must be deterministic and safe.

Recommended behavior:

- preserve `fusion_mode`;
- ignore/remove legacy `pitch_owner`, `boundary_owner`, `onset_owner` as authored product controls;
- preserve Stage 3 expert execution policies exactly;
- let the current Engine version resolve internal fusion behavior from the resulting evidence participation policy.

Do not silently enable or disable Stage 3 experts during migration merely to reproduce an old Step 4 owner field.

If exact backwards compatibility requires a temporary internal compatibility projection, keep it backend/internal and versioned; do not keep the obsolete controls visible in the new UI.

---

# 9. Engine planner changes

Planner requirements must follow actual evidence participation and Engine algorithm needs rather than user-authored owners.

Examples:

- do not require FCPE merely because an obsolete `pitch_owner=fcpe` field exists;
- do not require GAME merely because `boundary_owner=game` exists;
- do not require Basic Pitch merely because `onset_owner=basic_pitch` exists;
- requirements should come from the Stage 3 workflow execution policy plus required deterministic fallback rules owned by Engine.

If Engine has a mandatory baseline for a particular capability, represent that baseline explicitly in Engine policy/plan rather than smuggling it through a Step 4 owner parameter.

Plan Preview remains the authority for what will actually execute.

---

# 10. Step 4 UI target

Replace the current large ownership panel with a much smaller product-level card.

Recommended structure:

```text
STEP 4 · FINAL FUSION

Decision method

[ Algorithm ] [ AI judgment ]

Algorithm
Deterministic Engine fusion evaluates participating evidence and selects a globally coherent valid candidate path.

AI judgment
The same Engine-produced candidate pool is sent to the verified Fusion Agent Adapter. AI can select real candidates only; it cannot create evidence. Failure never falls back to Algorithm.
```

Optional read-only summary beneath it:

```text
Configured evidence
RMVPE          Always
FCPE           Disagreement windows
GAME           Always
Basic Pitch    On disagreement
ROSVOT         Maximum only
STARS          Maximum only
Acoustic DSP   Always

Exact participation and runtime readiness -> Plan Preview
```

Do NOT label this summary `Enabled evidence inputs`, because a configured conditional/Maximum-only expert may not execute in the current run.

Prefer wording such as:

```text
Configured evidence
Potential evidence
Evidence policy
```

---

# 11. Fix current misleading Step 4 copy

Remove or replace the following current concepts:

```text
Resolved fusion intent
Continuous F0 owner
Note lengths owner
Onset support owner
Enabled evidence inputs
```

`Resolved` is especially incorrect inside Processing Studio because the card reads mutable workflow configuration, not an exact resolved Engine Plan.

Use `Configured` for editable workflow state.

Reserve `Resolved` for Plan Preview / Engine Plan.

---

# 12. AI selected but adapter unavailable state

Preserve the actual workflow decision mode even when Runtime Manager readiness changes after the workflow was saved.

Current desired behavior:

```text
saved fusion_mode = ai
adapter later becomes missing/unusable
```

Step 4 must still visibly show:

```text
AI judgment = selected
```

with a blocked/unavailable state.

Do NOT make both Algorithm and AI appear unselected.

Recommended presentation:

```text
✓ AI judgment · Adapter unavailable

This workflow remains configured for AI judgment.
Restore the adapter or switch to Algorithm before queueing.
```

Preview must remain fail-closed.

No automatic mode change and no Algorithm fallback.

---

# 13. Pipeline copy must be selector-neutral

Current fixed Step 4 copy includes:

```text
candidate graph -> duration-aware decode -> canonical singing track
```

That is only correct for the Algorithm selector.

AI judgment does not execute the HSMM duration-aware selector.

Use common wording such as:

```text
Evidence normalization
-> candidate construction
-> final path selection
-> canonical singing track
```

Then, if useful, state the concrete selector separately:

```text
Algorithm: duration-aware deterministic decoder
AI judgment: verified Fusion Agent Adapter
```

Do not imply HSMM selected an AI result.

---

# 14. Provenance target

Removing user-authored owners does NOT mean losing explainability.

Instead, improve result provenance so the Engine explains what actually happened.

Useful final trace fields include concepts equivalent to:

```text
candidate construction policy version
continuous contour resolution source/policy
selected boundary source per note
selected target-pitch source per note
considered target-pitch sources
participating onset/context evidence
whether degraded F0 segmentation fallback was used
final selector = algorithm | ai_judgment
```

These are output/provenance facts, not editable workflow controls.

The current `FusionDecisionTraceV1` can evolve to represent this truthfully.

---

# 15. Main source areas

Likely files include:

```text
desktop/src/studio/processing_studio/stage_fusion.rs
desktop/src/studio/processing_studio/tests.rs
desktop/src/studio/analysis_preview.rs
desktop/assets/i18n/en.json
desktop/assets/i18n/zh-CN.json
desktop/assets/i18n/ja.json

app-core/src/workflow/definition.rs
app-core/src/workflow/default_definition.rs
app-core/src/workflow/wire.rs
app-core/src/workflow/compiler.rs
app-core/src/workflow/mod.rs

analysis-engine/src/workflow.rs
analysis-engine/src/planner/plan.rs
analysis-engine/src/engine.rs
analysis-engine/src/candidate_pipeline.rs
analysis-engine/src/fusion/baseline.rs
analysis-engine/src/fusion/hsmm.rs
analysis-engine/src/fusion/canonical.rs
analysis-engine/src/contract/result.rs
```

Do not assume every file must change. Keep the patch focused.

---

# 16. Required tests

Add/update tests for at least the following.

## Workflow/UI

```text
1. Step 4 exposes Algorithm / AI judgment as the only editable fusion policy.
2. New workflow no longer requires authored pitch_owner/boundary_owner/onset_owner.
3. Saved legacy owner parameters migrate without silently changing Stage 3 expert execution policies.
4. AI mode stays visibly selected when the adapter becomes unavailable.
5. Step 4 copy does not claim mutable workflow state is Resolved.
6. Configured/conditional evidence is not mislabeled as definitely participating evidence.
```

## Planner/Engine

```text
7. Requirements are derived from Stage 3 participation + Engine policy, not legacy owner fields.
8. GAME Disabled can still produce a valid degraded Engine-resolved segmentation path without a Step 4 boundary-owner toggle.
9. FCPE-resolved continuous contour with RMVPE secondary evidence produces truthful contour provenance.
10. FCPE-resolved contour uncertainty does not use hard-coded RMVPE quality fields.
11. Acoustic and Basic Pitch evidence are not asymmetrically discarded merely by an onset owner setting.
12. Algorithm and AI modes receive the same candidate pool for the same exact evidence execution.
13. Algorithm/AI differ only at final candidate-path selection.
14. AI failure still never executes Algorithm.
15. Canonical final-path validation remains identical after either selector.
```

## Preview

```text
16. Plan Preview shows exact decision mode.
17. Plan Preview shows exact planned/participating evidence from Engine Plan.
18. Preview never contacts the AI provider.
```

---

# 17. Non-goals

Do NOT use this work to:

- remove Stage 3 expert execution controls;
- let AI generate new evidence or note values;
- introduce a second AI-specific candidate builder;
- rewrite Analysis Engine into a generic DAG runtime;
- weaken exact Preview -> exact queued request semantics;
- add silent model/runtime fallback;
- make AI the default;
- silently switch an unavailable saved AI workflow back to Algorithm;
- remove per-note evidence/provenance explaining the final decision.

---

# 18. Definition of done

This card is complete only when all of the following are true:

```text
[x] Step 4 has one editable product decision: Algorithm vs AI judgment.
[x] Stage 3 remains the only user surface controlling evidence expert participation.
[x] pitch_owner is no longer a user-authored Step 4 workflow setting.
[x] boundary_owner is no longer a user-authored Step 4 workflow setting.
[x] onset_owner is no longer a user-authored Step 4 workflow setting.
[x] Engine resolves continuous contour, segmentation fallback and onset/context use internally.
[x] Candidate construction is identical before Algorithm vs AI selection.
[x] FCPE-resolved contour provenance/uncertainty no longer leaks RMVPE ownership assumptions.
[x] Already-produced Basic Pitch/Acoustic context is not asymmetrically discarded by obsolete onset ownership.
[x] GAME Stage 3 policy no longer has a duplicate Step 4 authority.
[x] Processing Studio uses Configured/Potential wording; Plan Preview owns Resolved/Participating truth.
[x] Saved AI mode remains visibly selected when its adapter is unavailable and remains fail-closed.
[x] AI-mode pipeline copy does not claim HSMM/duration-aware Algorithm decode executed.
[x] Provenance explains actual resolved evidence/selection behavior without reintroducing editable owners.
[x] EN / zh-CN / ja UI copy remains synchronized.
[x] Focused tests pass.
[x] cargo fmt --all -- --check passes.
[x] cargo xtask docs check passes if documentation is touched (no product documentation changed).
[x] git diff --check passes outside explicitly retained test evidence.
```

Recommended verification:

```text
bash dev.sh -c cargo test -p uta-analysis-engine
bash dev.sh -c cargo test -p uta-studio-core
bash dev.sh -c cargo test -p uta-studio-desktop
bash dev.sh -c cargo fmt --all -- --check
bash dev.sh -c cargo xtask docs check
```

Do not mark this `READY` merely because the Step 4 buttons were hidden. The obsolete authored-owner semantics must also be removed/converged in workflow, planner, Engine behavior and provenance, and the FCPE/onset correctness issues above must be closed.
