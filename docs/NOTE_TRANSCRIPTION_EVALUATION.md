# Note transcription evaluation

`tools/evaluate-singing-notes.py` compares a predicted chart with human note
annotations. It does not run inference, modify source data, or decide whether
a build may proceed.

The reference input is JSON with microsecond times and fractional MIDI:

```json
{"notes": [{"start": 100000, "end": 350000, "midi": 60.0}]}
```

The prediction may use that representation or VocalChart JSON. VocalChart
times are converted using its declared `timebase`; pitch includes MIDI plus
cents. Only pitched notes are evaluated. Select a solo track explicitly with
`--track-id lead` when the chart contains more than one part.

```sh
python3 tools/evaluate-singing-notes.py reference.json vocal-chart.json report.json --track-id lead
```

The tool reports precision, recall, F1 (`f_measure`) and average overlap for
two independently computed maximum-cardinality one-to-one matchings:
onset plus pitch, and onset plus pitch plus offset. An augmenting path can
reassign an earlier pair, so an unfortunate greedy choice does not lower recall.

Defaults follow the official
[mir_eval transcription definitions](https://github.com/craffel/mir_eval/blob/main/mir_eval/transcription.py):
onset distance at most 50 ms; pitch distance at most 50 cents; offset distance
at most the larger of 50 ms and 20% of the reference note's duration.
Time distances are rounded to four decimal places in seconds, matching the
documented implementation. The tool compares fractional MIDI directly in cents,
instead of converting MIDI to Hertz and back. These are configurable **measurement
tolerances**, not application restrictions or acceptance gates.

Use `--onset-tolerance-ms`, `--pitch-tolerance-cents`,
`--offset-min-tolerance-ms`, `--offset-ratio` and `--strict` to report a
different measurement. The output records every value. As in mir_eval, empty
reference or prediction inputs produce zero precision, recall and F1.

Every match includes signed onset, offset and pitch errors. Each score includes
their mean and median absolute error, 90th and 95th absolute percentiles and
maximum error. When multiple maximum matchings exist, deterministic local
edge ordering resolves ties; the error distribution does not claim globally
minimum matching error. The overlap ratio uses mir_eval's signed
intersection/span formula: a very short pair that passes the timing tolerances
without actually overlapping can have a negative ratio.

Additional per-reference diagnostics are deliberately separate from mir_eval:

- Each prediction belongs to the reference note with the greatest positive
  temporal overlap. Pitch and onset differences break overlap ties.
- Extra same-pitch segments count owned same-pitch predictions after the first.
  Extra internal splits count their distinct later onsets strictly inside the
  reference interval. Exact duplicate onsets remain excess segments, not new
  internal cuts.
- Union coverage reports unobserved time, correct-pitch time and time covered
  only by wrong-pitch predictions. A long note that merges two references can
  have coverage while still missing a required onset.
- No note is removed for being short. A true 30 ms annotated note can match
  perfectly; splitting a held reference into several pieces reduces note
  precision even when its pitch coverage remains complete.

These diagnostics describe the supplied annotation's note convention. They
cannot establish whether annotations omitted ornamentation or used different
musical phrasing. Evaluate public annotated songs in addition to the reported
user song, and inspect precision, recall, offsets and coverage together before
interpreting a lower fragment count as better transcription.

## Boundary and independent-lyric follow-up — 2026-09-14

This follow-up compares the preceding `d5d1df49` result with the revised
decoder, lyric representation and newly executed native Qwen alignment.
The five recordings and human annotations below are unchanged. The two original
calibration clips remain the tuning set; the three previously scored validation
recordings are now **regression samples, not a new heldout set**.

### Implemented mechanisms

The selected melody path now includes explicit unpitched intervals. A pitched
candidate pays duration-proportional loss where both valid RMVPE and FCPE
observations lack voice and valid DSP provides no reliable periodicity. Missing
model coverage remains unknown rather than being treated as silence. The decoder
can choose termination, rest and recovery candidates without trimming or dropping
notes afterward. All original candidates and the raw continuous pitch trace remain
available. There is no minimum-note-duration filter or production RMS cutoff.

GAME and the other note experts contribute localized native onset evidence.
Credit is deduplicated by source/event, including events that recover after
an unsupported interval. The native-event utility remains 0.6: a calibration
simulation increasing it to 0.8 recovers more onset matches but increases false
cuts from nine to thirteen and lowers both F1 measures. These alternate weights
were CPU diagnostic simulations, not separate model runs or production changes.

A second cause of fragmentation was downstream lyric projection: every measured
word onset could cut an already selected melody note. Words now have independent
absolute timing in UTZ. Multiple words can share a note while retaining separate
editable intervals; one word can span multiple notes. The final projection keeps
the selected pitched-note geometry intact. Unresolved text stays explicit and
does not receive fabricated measured timing. Editor seek, drag, resize, split,
merge, clipboard and explicit alignment commands distinguish lyric timing from
note timing. Explicit Align MIDI binds the lyric to note time; independent lyric
time survives ordinary note edits and quantization.

### Measurements against human annotations

These are previous → follow-up results. No short predictions are filtered.

| Recording | Use | Human notes | Predicted notes | Extra cuts | Onset/pitch F1 | With offset F1 |
|---|---|---:|---:|---:|---:|---:|
| CSD kr001a | Calibration | 106 | 106 → 97 | 8 → 3 | 82.1% → 85.7% | 50.0% → 54.2% |
| CSD kr003a | Calibration | 105 | 117 → 118 | 5 → 6 | 87.4% → 86.1% | 52.3% → 62.8% |
| CSD kr002a | Regression | 231 | 247 → 237 | 13 → 5 | 89.5% → 91.0% | 69.9% → 78.2% |
| CSD kr005a | Regression | 141 | 160 → 155 | 13 → 9 | 86.4% → 86.5% | 54.5% → 57.4% |
| Vocadito A1 | Regression | 59 | 68 → 71 | 9 → 11 | 66.1% → 64.6% | 31.5% → 30.8% |

Calibration micro F1 improves **84.8% → 85.9%** for onset/pitch and
**51.2% → 58.7%** including offset; extra cuts fall **13 → 9**.
However, onset recall falls **87.2% → 86.7%**, with one fewer matched onset.
Uncovered reference time remains 0.219 seconds and wrong-pitch-only coverage
increases 1.582 → 1.621 seconds. Standalone GAME still has higher calibration
onset/pitch F1 (**90.6%**); its offset F1 is **50.4%**.

The three regression recordings together reduce cuts **35 → 25** and improve
onset/pitch F1 **85.2% → 85.7%**, offset F1 **59.4% → 64.4%**.
Onset recall decreases **89.6% → 88.9%**; uncovered reference time increases
**1.433 → 1.631 seconds**, entirely in Vocadito. Wrong-pitch-only coverage
increases **5.268 → 5.323 seconds**. Across all five, cuts fall **48 → 34** and
offset F1 improves **56.7% → 62.6%**. These averages do not erase the Vocadito
regression or demonstrate uniformly better singing coverage.

Vocadito's second annotation remains a separate evaluation; it is never combined
with A1 to select favorable matches. The follow-up result has 71 predicted notes
against A2's 64, with onset/pitch F1 78.5%. Detailed matches and offset results are
in `evaluations/vocadito-1/final-annotator-second.json`.

The comparison retains the same six non-alignment native-model outputs for each
public recording and reruns their candidate construction and fusion on CPU.
Qwen was actually rerun on B580 with the corrected native implementation.
Parallel `final-retained-alignment` replays hold Qwen evidence fixed as well,
allowing the decoder/projection effect to be inspected separately.
Raw RMVPE curves and the six non-alignment input artifacts are unchanged.

### Imported LRC and native alignment

The original song did include timed LRC: all **26 exact line texts and start/end
ranges** match the retained caller input. LRC supplies line ranges, not measured
word or mora boundaries. All **361 non-whitespace characters** remain in the
original-song chart.

The native Qwen implementation now follows the corresponding full audio/text
context rather than pairing a long transcript with only the first eight seconds.
Both native backends use the official block-diagonal encoder attention structure.
Native Japanese segmentation now uses the already available ICU dictionary
instead of individual characters. ICU is not identical to the official Nagisa
segmenter, especially on rare words. See the
[official Qwen implementation](https://github.com/QwenLM/Qwen3-ASR)
for the reference architecture; retained source comparisons are under
`qwen-full-context/`.

On the original character segmentation, unresolved units decrease **179 → 168**
and unresolved characters **181 → 170**. After dictionary segmentation, the
236 word units contain **98 unresolved words covering 144 characters**.
The 179 and 98 counts use different units and must not be presented as an
81-item timing-accuracy improvement. The original 26 LRC scopes and all 361
characters are unchanged. `目覚める` is now one measured word interval
51.620–53.380 seconds; `切れ` and `に` retain separate intervals
104.240–104.640 and 104.640–105.040 seconds even when they share one note.

The full-context change helps four public clips' unresolved-unit counts, but the
138-second repetitive Korean kr002a case worsens **169 → 186**. Its head/context
limits are not saturated, and applying official timestamp postprocessing to the
new raw outputs matches the native result. Real-audio tensor/logit parity with
the official implementation remains unproven. A positive word interval alone is
not independent evidence that its timing is correct. The shared encoder ASR path
has relevant unit coverage but no fresh real-ASR qualification in this follow-up.

A fresh complete original-song analysis ran all seven native models on **B580**:
GGML/Vulkan for its selected models and LibTorch/XPU for JBM and Qwen. It finishes
as `ok_degraded`, preserving unresolved timing, with **493 pitched notes and
11 below 100 ms**. This is real inference, not a replay. A matched replay using
the preceding six non-alignment model outputs plus newly measured Qwen produces
**499 notes / 12 below 100 ms**, versus **505 / 20** at `d5d1df49`.
Pitched duration changes 176.760 → 175.800 seconds. The fresh run yields
175.870 seconds; its six newly generated outputs are not identical to the matched
replay inputs. Neither difference should be attributed solely to a score change.
The historical 575-note / 40-short-note chart remains a different-run comparison.

### Execution evidence and remaining scope

Follow-up artifacts are under
`test-artifacts/singing-boundary-alignment-followup/`.
`results-summary.json` contains per-recording and micro scores;
`replay-inputs/`, `replays/` and `evaluations/` retain the exact inputs and
outputs. `qwen-full-context/` holds raw classes, timestamps and alignment
diagnostics. `fresh-execution-summary.json` and `fresh/analysis-result.json`
describe the fresh complete original analysis.

Key operation receipts:

- Native XPU library compilation: `20260914T045424-18960492dbdd`.
- Native Qwen units: 34 passed, six ignored,
  `20260914T045917-d8d3e18412f5`.
- Original full pipeline: `20260914T054056-6afbdd997acd`.
- Calibration final replays: `20260914T054020-06ecdc5af0fb`;
  regression final replays: `20260914T054929-2d935ee86173`.
- Native runtime/worker strict Clippy: `20260914T053345-636508350083`.
- Final UTZ and UltraStar exports: `20260914T055702-ee8222837e89` and
  `20260914T055738-aea16599b241`; semantic verification
  `20260914T055851-7191f07bde38`.

The actual follow-up Vocadito chart exports in an isolated library with all
**71 pitched notes and 129 characters**. UTZ round-trips the entire chart
semantically, including **32 independent lyric intervals** and three unresolved
text tokens. UltraStar retains the note geometry and text, with maximum grid
displacement **20 ms**; its format cannot encode the independent word timeline,
fractional pitch or unresolved flags. It does not split melody notes to simulate
that timeline. Both lossless FLAC streams decode completely through
33.212245 seconds. Evidence:
`export-final/verification/summary.json`.

This is bounded algorithm verification. Installed runtime libraries, configured
model directories, user source media and cached charts were not replaced.
Build artifacts and an isolated native runtime were produced for execution.
No whole-workspace, Nix-package or release pass is claimed. 21J remains
`NEEDS_REVIEW`: GAME's better calibration onset score, remaining lyrics,
Vocadito's regression and broader singing/listening coverage remain open.

## Public singing measurements — 2026-09-14

This section records the preceding d5d1df49 experiment; the follow-up above supersedes its current-result and next-action statements.

The current fusion and lyric projection reduce false internal note cuts on
three recordings whose annotation scores were withheld during tuning. This is
a small diagnostic sample, not qualification for every singer or genre.

### Recordings and experiment

Five complete public excerpts total **383.671 seconds**, with **642 human notes**:

- Four Korean recordings from Children's Song Dataset (CSD), using the
  [human note reannotations](https://github.com/seyong92/CSD_reannotation) and
  [DynamicSuperb's public CSD audio/transcript mirror](https://huggingface.co/datasets/DynamicSuperb/ChildrenSongTranscriptVerification_CSD).
  The annotation follows score notes and vowel onsets; an ornament inside a
  held note is not automatically a separate note.
- One 33.212-second Tagalog recording from
  [Vocadito](https://arxiv.org/abs/2110.05580), retrieved with both annotators'
  files from the [mirdata maintainer repository](https://github.com/mir-dataset-loaders/mirdata/tree/master/tests/resources/mir_datasets/vocadito).
  Annotator A1 is the primary reference; A2 is reported separately.

CSD kr001a and kr003a were selected for calibration before inference.
CSD kr002a, kr005a and Vocadito were reserved for validation. Source
`d5d1df49103eaafa4641d108b55ce5a6bcd26a69` was selected before reading their
quality scores; no later source tuning used those scores. "Validation" here
describes this algorithm experiment, not exclusion from upstream model training.

Every recording ran native RMVPE, FCPE, Basic Pitch, GAME, JBM, ROSVOT and Qwen
alignment. Inputs included the public audio and untimed supplied lyrics;
reference note pitches and times were used only by the evaluator. Dry singing
did not require RoFormer separation. Native GPU inference used the existing
GGML/Vulkan and LibTorch/XPU routes. The final CPU diagnostic reran candidate
construction, fusion and chart projection on those retained raw model outputs.
Control `ad9af564e8f458bd80e6237e0724140b204e29af` and final code use identical
evidence, caller scopes and text; the control already includes text-retention
repairs, so this comparison isolates later note-processing changes.

### Results

F1 columns use the one-to-one onset/pitch and onset/pitch/offset definitions
above. Cuts are extra internal splits inside a human reference note. Each cell
shows control → final; short notes are not discarded to calculate the scores.

| Recording | Use | Human notes | Predicted notes | Extra cuts | Onset/pitch F1 | With offset F1 |
|---|---|---:|---:|---:|---:|---:|
| CSD kr001a | Calibration | 106 | 168 → 106 | 51 → 8 | 66.4% → 82.1% | 32.8% → 50.0% |
| CSD kr003a | Calibration | 105 | 168 → 117 | 44 → 5 | 70.3% → 87.4% | 41.0% → 52.3% |
| CSD kr002a | Validation | 231 | 430 → 247 | 163 → 13 | 62.9% → 89.5% | 40.5% → 69.9% |
| CSD kr005a | Validation | 141 | 272 → 160 | 109 → 13 | 62.0% → 86.4% | 32.0% → 54.5% |
| Vocadito | Validation | 59 | 103 → 68 | 36 → 9 | 50.6% → 66.1% | 19.8% → 31.5% |

Across the three validation recordings, extra cuts fall **308 → 35 (88.6%)**.
Micro precision improves **46.8% → 81.3%**, recall **87.5% → 89.6%**, onset/pitch
F1 **61.0% → 85.2%**, and F1 including offsets **35.0% → 59.4%**. Uncovered
reference time remains **1.433 seconds**, while time covered only by wrong
pitches falls **8.267 → 5.268 seconds**. Each recording improves both F1
measures and recall; reducing cuts did not merely erase more singing coverage.

Calibration is reported separately: extra cuts **95 → 13**, onset/pitch F1
**68.4% → 84.8%**, offset F1 **36.9% → 51.2%**. Recall decreases **88.6% → 87.2%**
on calibration, including four fewer matched onsets on kr001a. The final
projection removes 19 further false cuts versus the preceding implementation
without losing matched onsets, but loses nine net offset matches: some previous
cuts happened near a true note end while leaving a false tail note. Long note
tails remain an error after those cuts are merged.

Vocadito A2's 64-note annotation gives onset/pitch F1 **64.7% → 78.8%**, offset
F1 **31.1% → 48.5%**, and extra cuts **30 → 4**. The two annotation sets are
not combined to select favorable matches.

Across all five recordings, **714/714 supplied non-whitespace characters** remain
in order. Canonical caller token IDs, exact text (including whitespace), measured
word boundaries, full lyric units and continuous pitch agree with their original
inputs and retained raw alignment/RMVPE evidence in both control and final.
The public inputs have no explicit timed caller tokens, so this check does not
establish timed-lyric accuracy or identical displayed whitespace. Details:
`final-text-evidence-preservation.json`, operation
`20260914T042910-993b7e3f28a1`.

### What changed and why

- Basic Pitch contributes resolved peaks, not every high activation frame.
  Source-local peak/valley hysteresis preserves distinct attacks in a sustained
  response, while small later fluctuations cannot move the earlier onset.
- A physical attack pays once across correlated candidate, context and adjacent
  state evidence. The same measured acoustic attack predicate is used for
  candidate creation and nearby onset support.
- Pitch fit integrates credible continuous observations over absolute time.
  Splitting a duration no longer obtains an artificial advantage by repeatedly
  selecting local medians. Independently supported acoustic pitch can protect
  an octave proposal; RMVPE, FCPE and the note experts remain available.
- Ordered adjacent lyric ownership associates words with an existing touching
  note edge when both notes overlap their respective words. The local tolerance
  is the larger of 60 ms and half the shorter adjacent note; other cases retain
  60 ms. Gaps, intervening edges, word order and genuine internal held-note word
  boundaries remain protected. This changes temporary display projection,
  preserving measured word timestamps and the continuous pitch trace.
- Imported text survives failed timing. A Vocadito run exposed a 5 microsecond
  resampling overhang in an unresolved Qwen search scope. Only unresolved scopes
  are intersected with the actual source interval; measured timestamps and their
  existing validity checks are unchanged.

This is still multi-model fusion. The two calibration recordings expose an
unresolved weakness: standalone GAME onset/pitch F1 is **90.6%**, above final
fusion's **84.8%**. Fusion offset F1 is **51.2%**, versus GAME's **50.4%**.
ROSVOT and JBM contribute no correct onset unmatched by GAME on these two
recordings; their candidates are still retained, and this observation cannot
establish their utility on other styles. The next quality work should examine
why some well-supported expert boundaries lose to the fused path and why note
tails extend into silence.

### Original Japanese song

Asphodelos was rerun through the seven native models before the same-evidence
comparison. Final notes fall **588 → 505**, and notes below 100 ms **52 → 20**.
Pitched coverage remains **176.760 seconds**. All **361 non-whitespace lyric
characters**, their order, original word measurements, caller scopes and raw
continuous pitch are preserved. **179 alignment units remain unresolved** and
are explicitly marked; their text is not given fabricated measured timing.

The historical cached chart's 575 notes / 40 short notes came from a different
model run and dropped text, so it is not the same-input control. Original-song
F0 fit is diagnostic evidence used by fusion, not independent human note truth.
Short F0 plateaus near 105.43 and 149.09 seconds still differ from discrete
targets; whether they should be separate scored notes needs musical review.

### Reproduction and verification

Local artifacts are under `test-artifacts/public-singing-validation/`:
`cases.json` records inputs and sources; `inputs/` contains actual requests;
`runs/` retains fresh model outputs; `replays/` contains control/final charts;
`evaluations/` contains all matches, errors, coverage and scoring parameters.
`calibration-selection.json` records the decision before heldout scoring.
`results-summary.json` contains the table and micro aggregates.
The Vocadito successful raw output is `runs/vocadito-source-scope/`.
Original-song artifacts and the four-way comparison are in
`test-artifacts/lyric-note-model-validation/final-regression-report.md`.

Operation receipts under `test-artifacts/operations/` include:

- Fresh original inference: `20260914T023253-887b6ac61b1e`.
- CSD inference: `20260914T032158-3bbd9408a274`,
  `20260914T032402-41b2f69d27c9`, `20260914T032558-06a4b0d53374`,
  `20260914T032849-3ee36c5e1509`.
- Successful Vocadito inference: `20260914T035903-61b8e964d6f1`. The first
  attempt failed on the unresolved-scope overhang; its failure is recorded at
  `20260914T033145-448f016dea63`. Separate native Qwen evidence is retained
  at `20260914T033635-abef30d141c3`.
- Final Engine suite: **364 passed, two ignored**,
  `20260914T040704-9875217203fc`; strict all-target Engine Clippy:
  `20260914T040932-4c58bc3a5b4c`; debug CLI/example build:
  `20260914T040807-085a585c3214`.
- Final validation replays: `20260914T040931-d2a205be04d2`,
  `20260914T040931-ceb384464fc2`, `20260914T040931-1ac71f8a462a`;
  their control/final annotation evaluations are the eight `20260914T041134-*`
  records. Aggregate report: `20260914T041345-fdfe68f24375`; original-song
  final report: `20260914T041357-a6d322aaf2fb`.

Actual final-chart exports also pass in an isolated library:
`export-final/output/public-vocadito.utz` and `public-vocadito.txt` each retain
**68 notes and all 129 non-whitespace caller characters**. UTZ's embedded chart
is semantically identical to the final input, including unresolved-text markers
and fractional pitch. UltraStar uses a 50 ms tick here; measured maximum
start/end displacement is **24.921 ms**, pitch is integer MIDI, and that format
does not carry the unresolved-timing flag. Its empty-text/continuation notes are
rendered as `~`. Both exported FLAC streams decode through the complete
**33.212245 seconds**. The first diagnostic incorrectly expected empty text
to remain blank in UltraStar; correcting that expectation changed no product
code or output. The recorded export itself succeeded on its first execution.

Relevant checks and receipts:

- UltraStar **10/10** existing tests: `20260914T041939-b489d04062fb`;
  UTZ full-text/unresolved-timing round-trip **1/1**:
  `20260914T041959-2221c3871dc6`.
- Current-source export CLI build: `20260914T042036-b7c1dc35d414`;
  actual UTZ export: `20260914T042140-2df911d5c25d`;
  UltraStar export: `20260914T042202-1ee0de745792`.
- Final semantic comparison: `20260914T042517-f0f32815bfad`;
  full FLAC decodes: `20260914T042517-801e85e24a58` and
  `20260914T042517-c3e1aedbcba4`.
  Details are in `export-final/verification/summary.json`.
- A source-size review of 470 application files finds none over 2000 lines:
  `20260914T042325-0e47ee239b55`.

All measurements are ordinary diagnostics, not a frozen comparison requirement
or a release gate. The final code was executed on newly generated real model
evidence; model inference was not repeated for each scoring/projection edit.
User source media, libraries, model directories, installed binaries and cached
charts were not replaced. Continuous audition, final-editor visual acceptance,
mixed-accompaniment coverage, broader singers/languages and production readiness
remain outside this measurement. See
[21J's current status](../tasks/final-features/followups/21J_MELODY_PATH_SCORE_COHERENCE.md#public-data-fusion-repair--2026-09-14-jst).
