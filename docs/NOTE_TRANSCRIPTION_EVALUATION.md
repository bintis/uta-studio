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
