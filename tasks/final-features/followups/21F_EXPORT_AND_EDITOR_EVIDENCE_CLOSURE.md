# 21F — Export and Editor Evidence Parity Closure

**State:** `READY`

**Parent:** card 21 final design-parity audit revision 6

**Task class:** focused source/test closure; no model inference or accelerator use

## Mission

Close two concrete current-source blockers found by the revision-6 Card 21 reread before Card 21 returns to `READY`.

## A. UltraStar publication safety

- [x] Stage chart and referenced assets outside their final names.
- [x] Publish every final file with no-replace filesystem operations; a concurrent file must never be overwritten.
- [x] Treat the chart as the final bundle commit marker and roll back only files created by the failed export.
- [x] Clean staging and failed publication output.
- [x] Add focused tests for target races/failure cleanup while preserving source media.

## B. Editor SingingAnalysis evidence boundary

- [x] Stop deserializing Engine `uta.analysis-engine.singing-analysis` JSON as an unrelated all-default `SingingEvidenceBundle`.
- [x] Add an app-owned, independently declared SingingAnalysis projection with exact contract/timebase/selected-candidate validation.
- [x] Define Fused F0 units truthfully and project real selected candidate evidence plus review regions.
- [x] Preserve unknown review confidence as unknown; never fabricate a measured zero.
- [x] Keep evidence read-only and retain explicit user acceptance plus undo for suggestions.
- [x] Add malformed-contract/unit/selection/review projection tests.

## Verification

```text
bash dev.sh -c cargo test -p uta-studio-core --lib
bash dev.sh -c cargo test -p uta-studio-desktop --bin uta-studio
bash dev.sh -c cargo fmt --all -- --check
bash dev.sh -c cargo xtask docs check
git diff --check
```

## Verification outcome — 2026-08-28

UltraStar exports stage every asset and chart under a unique sibling directory, validate staged output, publish assets with no-replace operations, publish the chart last as the logical commit marker, roll back only files created by the failed export and always clean staging. Race/failure tests preserve competitor bytes and source media.

Editor evidence now uses an app-owned strict projection of current `uta.analysis-engine.singing-analysis` artifacts, validates exact contract/format/timebase/full candidate-set digest and mode-specific decision provenance, projects selected measured-Hz evidence, retains review regions and preserves unknown confidence. Malformed/tampered artifacts fail closed; suggestions remain explicit and undoable. Focused UltraStar, Editor, app-core and Desktop suites pass.

## Ready condition

Set 21F to `READY`, rerun the affected Card 21 Export and Editor/Evidence rows, then record Card 21 revision 6 and durable state only when both sections are closed.
