# 21H — Editor Artifact Audition Lifecycle Closure

**State:** `READY`

**Parent:** card 21 final design-parity audit revision 6

**Task class:** focused source/test closure; no model inference or accelerator use

## Mission

Close the final current-source lifecycle gap found by the focused 21F/21G reread: immutable artifact A/B and waveform bindings must reconcile with the currently authorized, non-invalidated revision set instead of retaining stale playback state.

## A. Authorization reconciliation

- [x] Reconcile A, B, active-slot and waveform bindings against current Editor source-context authorization.
- [x] Include immutable non-invalidated historical audio revisions while excluding invalidated or missing revisions.
- [x] Normalize the active slot when a bound revision disappears.
- [x] Clear an artifact waveform binding when its revision is no longer authorized.

## B. Playback safety

- [x] Stop or move to a valid native fallback when the active artifact revision becomes unauthorized; stale artifact audio must not continue.
- [x] Revalidate artifact authorization during Editor audio status sync and before artifact actions.
- [x] Keep artifact waveform reads blocked while playback is running.
- [x] Preserve source media and immutable artifact bytes as read-only.

## C. UI, API and evidence

- [x] Keep explicit A/B bind and activate actions discoverable through the typed UI API.
- [x] Show actionable localized copy when an active revision disappears.
- [x] Add focused state/action tests for pruning, active-slot normalization, historical revision availability and playback-time waveform refusal.
- [x] Synchronize EN / zh-CN / ja catalogs.

## Verification

```text
bash dev.sh -c cargo test -p uta-studio-desktop --bin uta-studio
bash dev.sh -c cargo fmt --all -- --check
bash dev.sh -c cargo xtask docs check
git diff --check
```

## Ready condition

Set 21H to `READY` only when sections A–C are complete and a focused independent reread finds no remaining High/Medium lifecycle gap. Then close 21G and rerun the affected Card 21 Editor/Evidence row.

## Verification outcome — 2026-08-28

- A/B, active-slot, direct artifact playback and waveform bindings reconcile against current authorized revisions, including non-invalidated history and verified immutable backing files; missing/invalidated revisions are pruned and stale playback cannot continue.
- Waveform reads require freshly confirmed stopped native playback. Deferred and initial reads stay pending when status is unknown, Linux GStreamer pause/stop waits for the real current state with no pending transition, and failed stop cannot be mistaken for a cleared/stopped backend.
- Focused independent rereads found no remaining High/Medium issue after the final stop-state correction (`/tmp/acp-delegate/del_mtcwpu4n_ezz7.out`).
- Current evidence: Desktop `175 passed / 0 failed`; native audio `10 passed / 0 failed / 1 ignored`; EN / zh-CN / ja parity and focused lifecycle tests pass.
