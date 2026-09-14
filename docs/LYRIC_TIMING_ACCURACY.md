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

## Multilingual onset/pitch/offset acceptance — 2026-09-14

The user clarified the primary target as **onset + pitch + offset F1**, minimum
80%, preferably 90%, and explicitly authorized compiling/installing the updated
native implementation. Default evaluation tolerances remain onset 50 ms, pitch
50 cents, offset max(50 ms, 20% reference duration). Lyrics are assessed separately.
No reference pitch or word/note times are passed into inference.

Evidence: `test-artifacts/multilingual-chart-accuracy/`. Its `data/selection.json`
was written before inference/accuracy inspection: Chinese, Japanese and English,
two calibration segments of one song and three validation segments of a different
song per language. This is a small multilingual experiment, not universal or
model-training-disjoint qualification. Every selected segment stays in reporting.

The official GTSinger recordings provide original WAV, word/phone annotations,
expert MusicXML scores and JSON. Reference notes use the supplied `note_start` /
`note_end`, never the different quantized `note_dur`. Internal melisma boundaries
may be score-derived: the published generation code combines TextGrid word ranges
with note durations. These are provided benchmark annotations, not proof that
every note edge was independently hand-clicked. The authors also list further
Japanese annotation refinement as pending. Source responses and exact download
URLs are preserved; two English calibration TextGrid URLs returned 404, while
all selected WAV/JSON/MusicXML inputs downloaded. None are generated singing.
Sources: https://github.com/AaronZ345/GTSinger and
https://huggingface.co/datasets/GTSinger/GTSinger .

`tools/evaluate-singing-lyrics.py` compares observed boundaries at normalized
transcript character positions. It never picks a favorable repeated word by
nearby time. Coarser model segmentation contributes no invented inner boundaries;
unresolved points, absent independent timing and missing boundary positions stay
in reference denominators. Text mismatches are explicit and not rematched for
favorable timing scores. This word-boundary diagnostic is not the note F1 target.

Build/install `20260914T090936-27485e50ac87` completed the local product executable
set and updated `result/bin`. Native app-model library build
`20260914T091933-3f566568093c` succeeded; explicit atomic installation
`20260914T092118-27de12fdab61` retains the prior library/manifest under `install/`.
Trained weights, the existing Torch SDK and user media were not replaced.
The first six invocation attempts stopped before inference because uta-analyze
requires an already-created authorized output directory. The error and corrected
preparation remain recorded; failed invocations are not model measurements.
