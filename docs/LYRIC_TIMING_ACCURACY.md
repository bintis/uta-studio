# Lyric chronology and chart accuracy

## Authorized objective and measurement plan — 2026-09-14

The user requests local code/test/model iterations toward at least 80%, preferably
90%, agreement with human chart annotation. Source media, existing analysis
artifacts and configured model installations remain read-only. New outputs go to
`test-artifacts/lyric-order-accuracy/`. Each implementation is committed before
execution and commands are recorded with `tools/record-operation.py`.

The starting tree is `3319ef09`, clean. The previous task's effective evidence is
`docs/NOTE_TRANSCRIPTION_EVALUATION.md`, not a claim of production qualification.
Its five real recordings are now regression data, not an unseen test set.

Observable work:

1. Reproduce and repair chronological lyric placement, including unresolved
   words between adjacent measured words, without deleting text or inventing a
   positive measured duration. Preserve pitched note geometry and measured word
   timestamps. Exercise serialization, editing and exports.
2. Measure lyric timing independently from note accuracy. Count unresolved
   timing as missing, not correct. Retain exact reference/prediction paths,
   tolerances, per-recording results and aggregate precision/recall/F1.
3. Inspect and test alignment and note-boundary mechanisms on calibration data.
   Rerun native models where new evidence is necessary. Reserve additional
   public recordings before inspecting their outputs; do not feed their human
   boundaries into inference.
4. Report actual achieved metrics and regressions separately. A chronology
   invariant or onset/pitch F1 is not full onset/pitch/offset/lyric accuracy.

## Reproduced failure

Operation `20260914T084017-1b830944fb07` audits the previous final-source original
song chart. Its 184 nonempty displayed text tokens contain five adjacent start
inversions. The screenshot case is exact: `を` starts at 42.110 s, then unresolved
`覆う` falls back to the entire LRC line starting at 41.310 s. The measured next
word starts exactly at the preceding word's end, leaving no positive interval
for the missing word. `lyric_timing` consequently fell back to the unbounded
original search scope. Textual ownership order was correct, timeline order was
not. The unresolved scope is not evidence of when the word was sung.

## First implemented repair

Commit `e8f8f78` replaces whole-line fallback with a linear two-pass projection
bounded by preceding/following measured words. A missing word with no available
positive interval keeps an explicitly unresolved point marker; it is not counted
as correct timing and does not create a zero-length or synthetic pitched note.
UTZ permits duration zero only on unresolved lyric timing, not resolved words or
notes. Engine artifact tests: 76 passed (`20260914T084649-088797d89ce3`); UTZ:
17 passed (`20260914T084701-5d50a23ab3db`). Real-evidence replays are recorded
separately; these unit tests do not establish acoustic accuracy.

## Ordered acoustic timestamp experiment

Before inspecting new validation outputs, CSD kr004a, kr006a, kr007a and kr008a
were reserved (`20260914T085410-c5eca6fe72af`, `data/selection.json`). Calibration
remains kr001a/kr003a. The older five recordings are not new holdouts.

The next native experiment replaces interpolation of independent classifier
peaks with the exact maximum-total-logit nondecreasing timestamp sequence. It
uses all classifier alternatives, preserves raw independent peaks, allows equal
timestamps (missing words remain unresolved), and imposes no invented positive
duration. This is a new decoding algorithm, **not** official Qwen postprocessing
parity. Both native GGML and LibTorch routes use the same Rust implementation.
Exhaustive small-state tests check its optimum independently. Changes in resolved
coverage alone will not be reported as accuracy improvements.
