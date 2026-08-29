# 21G — Final Audit Runtime and Audition Closure

**State:** `READY`

**Parent:** card 21 final design-parity audit revision 6

**Task class:** focused source/test closure; no model inference or accelerator use

## Mission

Close the remaining concrete High/Medium findings from the revision-6 independent audit before Card 21 returns to `READY`.

## A. Fusion Adapter Request Supervision

- [x] Bound the serialized adapter request before spawn.
- [x] Move stdin writing under the same cancellation/timeout/process-tree supervision as stdout and process exit.
- [x] Ensure an adapter that never reads stdin or fills stdout first cannot hang analysis.
- [x] Add backpressure tests proving timeout and active cancellation terminate/reap the adapter.

## B. Already-Semantic Lead Inputs

- [x] For `LeadVocal` and `CleanLeadVocal` primary inputs, satisfy an explicitly requested `LeadVocal` output from the declared source without another separation pass.
- [x] Do not require the harmony-isolation model solely to retain/export an already-semantic lead source.
- [x] Preserve explicit cleanup policy semantics and exact analyzer route reporting.
- [x] Add requirements/plan tests for both semantic source roles.

## C. Editor Artifact A/B and Waveform Selection

- [x] Represent primary and comparison audition sources independently.
- [x] Allow immutable workflow audio artifact revisions to be selected for playback and waveform inspection.
- [x] Provide an explicit A/B toggle that preserves playhead and play/pause state.
- [x] Keep source media and artifact revisions read-only.
- [x] Add state/action tests for source selection, A/B toggling, and artifact waveform routing.

## Verification

```text
bash dev.sh -c cargo test -p uta-analysis-engine
bash dev.sh -c cargo test -p uta-studio-core --lib
bash dev.sh -c cargo test -p uta-studio-desktop --bin uta-studio
bash dev.sh -c cargo fmt --all -- --check
bash dev.sh -c cargo xtask docs check
git diff --check
```

## Ready condition

Set 21G to `READY`, rerun the affected Card 21 Analysis and Editor/Evidence rows, and record revision 6 only after sections A–C are closed.

## Verification outcome — 2026-08-28

- Fusion Agent Adapter request serialization is bounded before spawn; stdin, stdout, cancellation, timeout and process-tree cleanup share one supervised lifecycle, with backpressure and oversized-request/response tests.
- Already-semantic `LeadVocal` / `CleanLeadVocal` inputs materialize the requested lossless lead stem without another model pass while preserving exact analyzer-route and cleanup semantics.
- Editor artifact A/B bindings, explicit active-slot switching, immutable historical revision selection and independent artifact waveform routing are implemented through typed UI actions. Follow-up 21H closed the lifecycle/reconciliation edge cases found by the focused reread.
- Current focused/full evidence: Analysis Engine `205 passed / 0 failed / 2 ignored` plus CLI integration `4 passed`; Desktop `175 passed / 0 failed`; native audio `10 passed / 0 failed / 1 ignored`; formatting and generated documentation checks pass.
