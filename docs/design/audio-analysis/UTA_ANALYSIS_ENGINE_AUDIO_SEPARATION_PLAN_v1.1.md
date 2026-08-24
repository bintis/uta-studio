# Uta Analysis Engine — Audio Separation Plan v1.1

**Status:** Design baseline
**Scope:** `uta-singing-engine` audio preparation, separation, lead isolation, cleanup, quality gating, and fallback behavior
**Primary goal:** Produce semantically well-defined audio artifacts for both karaoke playback/export and downstream singing analysis without coupling the public Engine contract to specific model implementations.

---

## 1. Design principle

Audio separation is not defined as:

```text
mix -> vocal + instrumental
```

It is defined as a semantic audio preparation pipeline serving two distinct optimization goals:

```text
Karaoke playback/export
    -> high-quality instrumental

Singing analysis
    -> analysis-ready lead vocal
```

These two paths MUST NOT be forced to share one result merely to reduce inference cost.

The Engine defines semantic capabilities and artifact roles. Runtime recipes decide which concrete models, backends, chunking strategies, and post-processing implementations satisfy those capabilities.

---

## 2. Responsibility boundary

### Uta Studio owns

- user workflow;
- source/artifact selection;
- outer DAG orchestration;
- queue/retry/cancel ownership;
- artifact lifecycle and revisions;
- freeze/bypass behavior;
- Candidate/Authored distinction;
- user-facing model installation and runtime UI;
- editor and final authoring decisions.

### Uta Analysis Engine owns

- audio decode and decoded-fact verification;
- channel conversion and resampling;
- model-dependent preprocessing;
- separation;
- lead isolation;
- cleanup;
- analysis-ready audio preparation;
- quality gates;
- runtime fallback decisions;
- singing-analysis execution and evidence generation.

### Runtime Manager owns

- model/runtime catalog;
- install/remove/repair;
- SHA-256 verification;
- runtime recipe validation;
- backend/device compatibility;
- readiness and resolved model/runtime locations.

The Engine MUST NOT download models implicitly.

---

## 3. Stable capability names

The Engine contract should expose semantic capabilities rather than concrete model names.

```text
audio.decode
audio.extract_vocals
audio.extract_instrumental
audio.lead_isolate
audio.lead_partition
audio.denoise
audio.dereverb
```

`audio.lead_partition` is distinct from `audio.lead_isolate`.

- `lead_isolate`: foreground/main vocals vs supporting vocals.
- `lead_partition`: multiple simultaneous foreground singers into separate analysis streams.

`audio.lead_partition` is optional/future-facing in v1 and MUST NOT be silently implied by `audio.lead_isolate`.

Concrete model names belong to Runtime Recipes, not the public Engine contract.

---

## 4. Current reference recipes

The current implementation direction may use the following models:

```text
BS-RoFormer Vocals EP317
    -> vocal extraction

MelBand-RoFormer Inst V2
    -> high-quality instrumental

MelBand-RoFormer Lead / Back (`melband_roformer_harmony`)
    -> foreground/lead isolation

MelBand-RoFormer Denoise
    -> denoise

MelBand-RoFormer Dereverb
    -> dereverb
```

These mappings are implementation recipes only.

The Engine should use one generic RoFormer runtime with per-model weights/configuration metadata where feasible.

Current implementation note: `melband_roformer_harmony` is the current lead/support separation recipe. The stable capability remains `audio.lead_isolate`; the public Engine contract must not depend on the model marketing name.

Future recipe replacements MUST NOT require Studio DAG or Engine protocol changes as long as the semantic capability remains unchanged.

---

# 5. Canonical audio roles

## 5.1 `instrumental`

Purpose:

```text
karaoke playback
UTZ export
external representation export
```

Optimization target:

- low foreground-vocal leakage;
- low musical damage;
- preservation of instruments and arrangement;
- stable timeline aligned to the source song.

`instrumental` is a user-facing/exportable musical asset.

A production instrumental SHOULD be generated using a dedicated high-quality instrumental recipe.

The Engine SHOULD NOT derive the production instrumental by simply subtracting the analysis lead from the original mix.

```text
DO NOT default to:

mix - lead_vocal -> production instrumental
```

---

## 5.2 `guide_vocals`

Purpose:

```text
complete original vocal reference
guide/reference playback
editing/reference
```

It MAY contain:

- lead;
- harmony;
- backing;
- doubles;
- ad-libs;
- multiple singers.

It is not required to be monophonic.

Conceptually:

```text
original mix
≈ instrumental + guide_vocals
```

This is a semantic approximation, not a mathematical invertibility requirement.

---

## 5.3 `lead_vocal`

Purpose:

```text
primary foreground singing target
exchangeable musical stem
primary input candidate for singing analysis
```

Target behavior:

- preserve the primary/foreground singer(s);
- reduce backing/harmony contamination;
- avoid aggressive analysis-only cleanup;
- remain useful as an audible musical stem.

`lead_vocal` is not necessarily guaranteed to contain only one singer.

---

## 5.4 `clean_lead_vocal`

Purpose:

```text
analysis-only working artifact
```

Typical derivation:

```text
lead_vocal
    -> optional denoise
    -> optional dereverb
    -> analysis gain normalization
    -> model-specific conditioning
    -> clean_lead_vocal
```

Primary consumers:

```text
Qwen ASR
Qwen ForcedAligner
RMVPE
GAME
Basic Pitch
ROSVOT
Technique analysis
Fusion
```

`clean_lead_vocal` is normally an Engine-owned working artifact rather than a default authored/export asset.

Important distinction:

```text
lead_vocal       = exchangeable musical stem
clean_lead_vocal = analysis working stem
```

---

## 5.5 `vocal_residual`

Internal Engine artifact representing the vocal content left after foreground lead isolation.

It MUST NOT automatically be labeled:

```text
backing_vocal
```

or:

```text
harmony_vocal
```

until classification and quality gates justify that semantic promotion.

---

## 5.6 `backing_vocal` / `harmony_vocal`

These may be produced when quality is sufficient, but v1 MUST be conservative.

A residual is not automatically a valid backing/harmony stem.

---

# 6. Standard original-mix execution graph

For:

```text
input role = original_mix
```

the conceptual graph is:

```text
                         Original Mix
                              |
                  Decode / canonicalize
                              |
             +----------------+----------------+
             |                                 |
             v                                 v
     Vocal extraction                   HQ instrumental
       capability                        capability
             |                                 |
             v                                 v
       guide_vocals                       instrumental
             |
             v
       Lead isolation
             |
        +----+----------+
        |               |
        v               v
    lead_vocal     vocal_residual
        |               |
        |          classify / quality
        |               |
        |        backing/harmony?
        |
        v
      cleanup
   +----+----+
   |         |
   v         v
denoise   dereverb
   \         /
    \       /
      v
 clean_lead_vocal
        |
        v
 singing analysis
```

The instrumental path and analysis-lead path are independently optimized.

---

# 7. Input semantic routing

The Engine MUST use explicitly declared input roles.

It MUST NOT infer semantic role from filenames.

## 7.1 `original_mix`

Run the full path as required:

```text
decode
-> vocal extraction
-> instrumental extraction if requested
-> lead isolation
-> cleanup
-> analysis
```

---

## 7.2 `vocal_stem`

Assumption:

```text
already separated from instrumental
may contain lead + backing + harmony
```

Engine behavior:

```text
decode
-> lead isolation
-> cleanup
-> analysis
```

Skip mix-to-vocal separation.

---

## 7.3 `guide_vocals`

Same high-level routing as `vocal_stem`:

```text
decode
-> lead isolation
-> cleanup
-> analysis
```

---

## 7.4 `lead_vocal`

Engine behavior:

```text
decode
-> cleanup
-> analysis
```

Skip vocal extraction and lead isolation.

---

## 7.5 `clean_lead_vocal`

Engine behavior:

```text
decode
-> validate
-> analysis
```

Do not perform additional separation or cleanup unless explicitly requested by policy.

---

## 7.6 `instrumental`

An instrumental-only input is insufficient for normal singing analysis.

It may participate as a secondary/reference source but cannot be the sole primary singing source.

---

# 8. Single-lead songs

This is the baseline quality target.

Conceptual path:

```text
guide_vocals
    |
    v
lead isolation
    |
    v
lead_vocal
    |
    v
cleanup
    |
    v
clean_lead_vocal
```

Acceptance signals may include:

- high foreground dominance;
- low estimated polyphony;
- stable lead continuity;
- low separator disagreement;
- low supporting-vocal contamination.

If lead purity is high, downstream analysis proceeds normally.

---

# 9. Lead + backing / harmony songs

Backing/harmony contamination can damage monophonic F0 and note analysis.

Standard path:

```text
guide_vocals
     |
     v
Lead Isolation
     |
 +---+-----------+
 |               |
 v               v
foreground       residual
lead
 |               |
 |               +-> backing evidence
 |               +-> harmony evidence
 |               +-> uncertainty
 v
clean lead
```

The Engine should perform a `Lead Purity Gate`.

Potential evidence sources:

```text
multi-F0 / polyphony evidence
Basic Pitch simultaneous-note activity
foreground/residual correlation
F0 stability
speech dominance
separator consistency
```

Quality behavior:

```text
lead purity = high
    -> accept

lead purity = medium
    -> Balanced/Maximum may run secondary recipe on disagreement windows

lead purity = low
    -> do not pretend the lead is pure
    -> continue only with explicit uncertainty/degraded status when valid
```

Example degraded reason:

```text
lead_isolation_uncertain
```

Downstream Fusion should receive this uncertainty.

---

# 10. Duet and multi-singer songs

A critical distinction:

```text
lead vs backing separation
!=
Singer A vs Singer B separation
```

Foreground isolation does not guarantee singer identity separation.

Therefore Audio Separation Plan v1 MUST NOT promise automatic extraction of:

```text
Singer A.wav
Singer B.wav
```

for arbitrary duet material.

---

## 10.1 Alternating duet

Example:

```text
A: ██████        ██████
B:       ███████       ███████
```

Separate singer source separation is generally unnecessary.

The Engine may analyze one foreground lead stream and use:

```text
lyrics
alignment
phrase/word boundaries
vocal topology
```

to split analysis windows by singer/part.

Example:

```text
0–12 s   Part 1
12–24 s  Part 2
24–35 s  Part 1
```

Within these windows the signal can still be treated as primarily monophonic.

This scenario is supported by v1.

---

## 10.2 Simultaneous duet

Example:

```text
A: █████████████
B: █████████████
```

with distinct pitches:

```text
A = C4
B = E4
```

The foreground vocal mixture is now polyphonic.

A monophonic RMVPE/GAME path cannot reliably represent both singers simultaneously.

The Engine MUST detect overlapping foreground singers when possible and MUST NOT force a falsely certain monophonic answer.

Baseline v1 behavior:

```text
detect overlap
-> mark polyphonic / uncertain
-> preserve region
-> avoid forced monophonic interpretation
```

Future/optional Maximum capabilities may add:

```text
same-class singer separator
speaker-conditioned separation
multi-F0 expert
lead partition
```

producing internal streams such as:

```text
analysis_lead_1
analysis_lead_2
```

---

# 11. Vocal topology evidence

Internal Engine evidence:

```text
VocalTopologyEstimate
├── mode
├── confidence
├── overlap_regions[]
└── support_regions[]
```

Suggested modes:

```text
single_lead
alternating_multi_lead
overlapping_multi_lead
lead_with_support
unknown
```

Example:

```json
{
  "mode": "overlapping_multi_lead",
  "confidence": 0.91,
  "overlap_regions": [
    {
      "start": 62300000,
      "duration": 8700000
    }
  ]
}
```

This is Engine evidence, not a new authored UTZ VocalChart semantic field.

---

# 12. Separation capability split

## `audio.lead_isolate`

Meaning:

```text
complete vocals
-> foreground/main vocals
```

It answers:

> What is the foreground/main singing content?

---

## `audio.lead_partition`

Meaning:

```text
multiple simultaneous foreground singers
-> separate analysis streams
```

It answers:

> Can simultaneous foreground singers be partitioned into separate streams?

v1 policy:

```text
audio.lead_isolate   baseline
audio.lead_partition optional / future
```

The Engine MUST NOT silently claim `lead_partition` capability when only foreground/background separation is available.

---

# 13. Quality profiles

## Fast

Typical behavior:

- separation as required;
- primary lead isolation;
- basic lead purity gate;
- optional cleanup only when clearly needed;
- duet overlap detection;
- no expensive secondary separation;
- no singer partition.

---

## Balanced

Typical behavior:

- full lead purity evaluation;
- adaptive denoise/dereverb;
- vocal topology estimation;
- secondary separation on disagreement windows;
- cleanup consistency checking;
- local reruns where useful.

---

## Maximum

Typical behavior:

- all Balanced behavior;
- multiple validated separation recipes where useful;
- disagreement-window escalation;
- richer vocal topology evidence;
- optional advanced lead partition capability;
- consistency reruns;
- preserve alternative candidates/evidence.

`Maximum` MUST NOT mean “run every model across the entire song unconditionally.”

Cost escalation should be driven by disagreement/uncertainty.

---

# 14. Cleanup safeguards

Denoise and dereverb may improve model readability while damaging genuine vocal characteristics.

Potentially damaged information includes:

```text
breath
rasp
vibrato sidebands
soft consonants
room tail
ornament onset
```

Therefore Balanced/Maximum should preserve both:

```text
lead_vocal
clean_lead_vocal
```

for consistency evaluation.

Possible disagreement check:

```text
RMVPE(raw lead)
vs
RMVPE(clean lead)
```

or equivalent onset/voicing/contour comparison.

If cleanup materially changes:

```text
onset
voicing
pitch contour
```

the Engine may mark:

```text
cleanup_damage_suspected
```

and fall back to the less-processed lead for affected analysis.

---

# 15. Quality gates

Every separation result should be checked before downstream use.

| Gate | Purpose |
|---|---|
| timeline | Verify length/time-zero consistency |
| finite | Detect NaN/Inf/corrupt samples |
| clipping | Detect abnormal clipping |
| silence | Detect accidental all-silence output |
| energy | Detect implausible stem energy |
| lead purity | Detect unresolved foreground polyphony/support vocals |
| vocal leakage | Estimate vocal leakage into instrumental |
| musical damage | Estimate instrumental damage |
| cleanup consistency | Detect cleanup-induced vocal-structure damage |

Quality-gate failures should be typed rather than mapped to a generic retry.

---

# 16. Fallback policy

Fallback is capability- and failure-specific.

Example:

```text
primary vocal separator fails
    |
    v
validated fallback recipe available?
    | yes
    v
fallback recipe

    | no
    v
fail required capability
```

An optional enhancement failure does not necessarily fail the entire analysis.

Example:

```text
Dereverb fails
    |
    v
lead_vocal remains usable
    |
    v
continue without dereverb
    |
    v
ok_degraded
```

For normal:

```text
original_mix -> singing analysis
```

required capability classes are approximately:

```text
decode
usable vocal extraction
usable analysis lead
```

Optional enhancement classes:

```text
denoise
dereverb
secondary separator
support-vocal classification
lead partition
```

---

# 17. Instrumental path independence

Production instrumental generation is a separate optimization problem from analysis-lead generation.

Preferred:

```text
Original Mix
    |
    v
HQ Instrumental Recipe
    |
    v
instrumental
```

Analysis:

```text
Original Mix
    |
    v
vocal extraction
    |
    v
lead isolation
    |
    v
cleanup
```

The Engine SHOULD NOT default to reconstructing production instrumental from the analysis path.

---

# 18. Internal Engine artifacts

Suggested internal artifact taxonomy:

```text
DecodedMix
RawVocalStem
InstrumentalStem
GuideVocalStem
LeadVocalStem
VocalResidual
CleanLeadVocal
VocalTopologyEvidence
SeparationQualityEvidence
```

Internal artifact names MUST NOT expose recipe/model implementation details such as:

```text
raw-vocal-ep317-stage2-final.wav
```

The public semantic roles remain implementation-independent.

---

# 19. Studio DAG mapping

Studio-facing product DAG:

```text
Original
   |
   +------> Instrumental
   |
   v
Vocal Separation
   |
   v
Lead Isolation
   |
   v
Vocal Cleanup
   |
   v
Singing Analysis
```

Studio may control:

```text
freeze
rerun
bypass
run downstream
artifact reuse
```

Engine internally resolves model/runtime recipes and execution details.

Example:

```text
Studio:
    Rerun Lead Isolation

Engine:
    resolve recipe
    -> model frontend
    -> chunking
    -> inference
    -> overlap-add
    -> postprocess
    -> quality gates
```

Studio does not need to know which concrete model was used.

---

# 20. `AudioSeparationPlanV1`

Suggested logical structure:

```text
AudioSeparationPlanV1
├── primary_source
├── available_sources
├── requested_roles
├── analysis_required
├── quality_profile
├── resolved_capabilities
├── execution_nodes
├── quality_gates
└── fallback_policy
```

Example: normal Balanced analysis from original mix:

```text
source:
  original_mix

requested:
  instrumental
  guide_vocals
  lead_vocal

analysis_required:
  true

plan:
  decode
  ├── extract_instrumental
  └── extract_vocals
       └── lead_isolate
            └── cleanup
                 └── lead_purity_check
```

Example: caller already supplies a clean lead:

```text
source:
  clean_lead_vocal

plan:
  decode
  validate
  -> singing analysis
```

---

# 21. Explicit v1 non-goals

Audio Separation Plan v1 does NOT promise:

```text
automatic perfect Singer A.wav + Singer B.wav extraction for arbitrary duet songs

perfect decomposition of every harmony/backing layer

that vocal residual is automatically a valid backing-vocal stem

that every song can be reduced to a perfectly pure monophonic lead stream
```

Future capabilities may address:

```text
lead partition
speaker/source attribution
multi-F0
multi-singer singing analysis
advanced harmony partition
```

v1 must identify difficult regions and propagate uncertainty rather than hiding ambiguity.

---

# 22. UTZ relationship

The Engine's exchangeable audio outputs map cleanly to UTZ audio roles when requested.

Typical examples:

```text
instrumental
guide_vocals
lead_vocal
backing_vocal
harmony_vocal
```

Internal working artifacts such as:

```text
clean_lead_vocal
vocal_residual
separation quality evidence
```

do not need to become default UTZ audio assets.

UTZ remains the data/interchange contract; the Engine remains responsible for how those candidate assets are generated.

---

# 23. Core invariant

The design should preserve this rule:

> **The caller declares what the audio is and what result is wanted. The Engine decides how to transform it into analysis-ready inputs and semantic output artifacts.**

A step that changes when a model/runtime changes belongs inside the Engine.

A step that represents product workflow, user intent, artifact lifecycle, or authored authority belongs in Studio.

---

# 24. v1 acceptance criteria

Audio Separation Plan v1 is considered correctly implemented when:

1. Explicit input roles deterministically select the correct pipeline entry point.
2. Production instrumental and analysis-lead paths can execute independently.
3. `guide_vocals`, `lead_vocal`, and `clean_lead_vocal` are never conflated.
4. Lead purity is evaluated before monophonic downstream analysis is trusted.
5. Alternating duets are supported without requiring singer source separation.
6. Simultaneous multi-singer regions are detected/marked instead of forcibly reduced to one certain pitch track.
7. Optional cleanup failures can degrade gracefully.
8. Required capability failures do not silently produce fake-success artifacts.
9. Studio sees stable semantic nodes/capabilities rather than model-specific implementation details.
10. Engine outputs preserve source timeline alignment and provenance.
