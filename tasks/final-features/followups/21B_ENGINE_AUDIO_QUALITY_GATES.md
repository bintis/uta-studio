# 21B — Engine Audio Quality-Gate Execution Closure

**State:** `READY`
**Parent:** card 21 final-v1 design parity audit
**Task class:** focused Analysis Engine semantic closure; CPU/fake audio fixtures only

## Gap

`EnginePlan.quality_gates` advertises:

```text
timeline_valid
finite_samples
clipping
silence_ratio
energy_ratio
lead_purity
cleanup_consistency
vocal_topology
```

but current source references `lead_purity`, `cleanup_consistency` and `vocal_topology` only while constructing the Plan string list. No execution stage evaluates them and no typed result evidence records their outcome. A Plan must not claim an Engine quality gate that execution does not perform.

The authoritative audio-analysis and separation designs retain conservative lead-purity/topology/cleanup checks in final v1 even though `audio.lead_partition` itself remains future/optional.

## Scope

1. Define a versioned typed separation/audio quality report owned by Analysis Engine.
2. Evaluate every gate advertised by the Plan against the exact semantic artifacts that exist for the request.
3. At minimum preserve deterministic timeline, finite, clipping, silence and energy facts; for Balanced/Maximum, produce truthful lead-purity, raw-vs-clean cleanup-consistency and vocal-topology/overlap evidence from available independent signals.
4. Do not invent calibrated probability, singer identity, BackingVocal or HarmonyVocal. Unknown/insufficient evidence remains typed unknown and must not be presented as a passed measurement.
5. Apply explicit required-failure versus `ok_degraded` policy, and carry report identity/outcomes into diagnostics, provenance and fingerprinting where behavior can change.
6. Ensure conditional experts can consume real review/uncertainty regions rather than Plan-only labels where applicable.
7. Add independently owned app-core wire DTO validation and read-only UI projection only if the result is product-visible; do not share backend implementation types.
8. If a named gate cannot be implemented truthfully in final v1, remove it from executable Plan claims and record it explicitly as outside the final design rather than leaving a label-only gate.

## Focused acceptance

- CPU/generated-audio fixtures cover clean solo, clipping, silence, energy anomaly, cleanup damage and overlapping/ambiguous foreground evidence.
- Gate ordering and output are deterministic; source timeline and media remain unchanged.
- Required failures fail closed; optional uncertainty degrades explicitly.
- Plan and result gate identities cannot drift.
- No model inference, GPU/Vulkan/Level Zero context, download or Nix build.
- Rerun card 20 semantic-audio/Candidate bubbles, then rerun card 21.

## Result

**Result:** `READY`

Analysis Engine now evaluates every gate named by the exact Plan in deterministic order and emits `uta.analysis-engine.audio-quality-report` v1 diagnostics. Timeline, finite-sample, clipping, silence and source-relative energy measurements are typed; Balanced/Maximum routes preserve conservative foreground ambiguity regions and raw-versus-clean cleanup comparisons without claiming calibrated probability, singer identity, BackingVocal or HarmonyVocal. Required timeline/finite/silence/energy failures fail closed. Degrading uncertainty produces `ok_degraded`; suspected cleanup damage is recorded and the analysis branch explicitly returns to the pre-cleanup audio rather than publishing a CleanLead claim.

The quality algorithm identity and exact Plan gate list participate in execution fingerprinting and provenance. app-core independently owns and validates its wire DTO, including exact Plan/result gate binding, ordering, requirements, finite metrics, regions and degraded status. The result remains available read-only in the persisted Engine history projection.

Focused acceptance passed with generated CPU/fake-audio fixtures: clean solo, clipping, silence, non-silent energy anomaly, cleanup damage, non-finite/timeline rejection and overlapping/ambiguous foreground evidence. The complete Analysis Engine suite passed (133 passed, 2 explicitly ignored native-package tests), and the complete app-core suite passed (468 passed). These suites reran card-20 semantic-audio/Candidate fake seams, including timeline-preserving separation, Candidate/Fusion, conditional scheduling and real CLI process contracts. `analysis-engine/src/engine.rs` is 1,977 lines after moving fingerprint identity DTOs to their owning module. `git diff --check`, Studio/backend implementation-import scans and package dependency scans are clean. No model inference, download, GPU/Vulkan/Level Zero context or Nix build was used.
