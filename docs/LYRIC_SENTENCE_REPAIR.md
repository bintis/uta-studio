# Lyric sentences and Workflow rhythm quantization

2026-09-14 JST. Task verification, not release or model-readiness qualification.

## Corrected behavior

- Generated transcript sentence/newline tokens now survive alignment, fusion and UTZ phrase projection. Audio recognition windows are not presented as measured lyric-line timing. Caller-canonical input keeps its supplied line boundaries instead of being re-split at punctuation.
- Repeated lines have distinct identities. Unresolved alignment entries still participate in text-position mapping, so missing timing does not shift the ownership of later lines.
- A held note crossing a measured next-line onset is divided without changing pitch or total duration. Unowned melody remains present. Quantized notes whose original word no longer overlaps are reassociated with overlapping measured text; an unsplit unowned note stays unowned.
- SingingAnalysis references the actual finalized phrase, note and lyric IDs, including split notes and unpitched lyric placeholders.
- Generated-text splitting covers closing/opening quotes, repeated punctuation, unspaced mixed scripts, decimals, common abbreviations, ellipses, Unicode and explicit newlines. These are textual heuristics, not a guarantee of linguistically perfect sentence segmentation.
- Supplied plain, standard LRC and enhanced LRC input remain caller-canonical. Enhanced prefixes are retained using the supplied line start. Mixed LRC keeps untimed body lines in source order with absent time windows; pure timed repeated tags expand in timestamp order. Saving/importing retains complete input separately from timed display segments. Saving alone does not analyze or replace charts.
- Authored charts retain their existing priority over candidates. Re-analysis does not silently replace a user's edits or import an unselected adjacent LRC file.

## Workflow control

The Workflow sidebar exposes **Rhythm quantization / 节奏量化** regardless of which module is selected. It uses the existing `ui.analysis.set_workflow_parameter` mutation, saves `enable_quantization` on the finalization node, and flows through the normal exact analysis request.

Precedence is explicit run override, saved Workflow preference, song analysis defaults, global defaults. The current engine uses a sixteenth-note grid (125 ms at 120 BPM), not one sixteenth of a beat. Quantization changes candidate-note timing, not continuous pitch or audio. The existing explicit-BPM requirement is unchanged; non-chart outputs do not run quantization. Changes take effect only after the next requested analysis.

## Real Asphodelos result

The authorized normal Studio queue run `studio-auto-1240821-1789313304000695525-1` completed and activated its results. This run used commit `5dd22000`, the existing settings with quantization disabled, cached separated audio, and the existing native worker; ASR, alignment and note analysis ran again. Later quantized-ownership and supplied-LRC cases were checked separately in CPU tests. No automatic failing-GPU retry or unrelated song re-analysis was performed.

| Item | Before | Verified new output |
| --- | ---: | ---: |
| Generated line tokens | 0 | 36 |
| UTZ phrases | 1 | 33 |
| UTZ notes | 546 | 572 after sentence-boundary splits |
| Alignment entries without resolved timing | 237 / 426 | 237 / 426 |

Every prior note's time coverage and pitch is preserved. The new package matches the activated CandidateChart exactly; all phrase/note/lyric references agree. UTZ phrases are ordered, note durations are positive, IDs are unique, and continuation targets resolve. UltraStar has 572 notes and 33 line-end markers, including the final line. Both formats' vocal and instrumental FLAC streams decoded fully.

New files, without overwriting the original package or source media:

- `/home/bintis/Documents/uta!/Asphodelos-sentences-20260914.utz`
- `/home/bintis/Documents/uta!/Asphodelos-sentences-20260914.txt` and its FLAC/cover assets.

The real game loaded the new package under an isolated AMD/Wayland session. At 45 seconds it displayed one current sentence and the next sentence; natural playback from 42.8 seconds advanced to 1:00 with a different current sentence. Screenshots: `/tmp/uta-lyric-visual.A0RFR0/paused.png` and `playing.png`. The test muted audio and disabled microphone/recording; it is visual playback evidence, not sustained audio/xrun qualification. Game logs include pre-existing PipeWire, GStreamer and Japanese segmentation warnings, so they are not claimed warning-free.

## Executed checks and receipts

All operation receipts below are under `test-artifacts/operations/`; they record exact commands, commits, inputs, dirty state and completion. A clean detached worktree at `/tmp/uta-studio-lyric-verification` excluded unrelated staged application/native changes from core and desktop builds.

- Engine: 314 passed, one existing ignored test — `20260913T154229-5743555a9cb3`.
- LRC filter: 24 passed; lyrics module: 16 passed; analysis adapter: 36 passed (groups overlap) — `20260913T154343-1c5b675c33b0`.
- Workflow controls: 25 passed — `20260913T154422-69be8b37b7ab`.
- Prior focused export tests: 94 passed, including binding tests — `20260913T150743-eafd08679431`.
- Real analysis: `20260913T152822-94f07262a10e`; observer evidence in `test-artifacts/asphodelos-lyric-reanalysis-20260914/`.
- New UTZ / UltraStar export: `20260913T153203-7ac0186bcf21`, `20260913T153219-c7284560bc76`.
- Structural comparison, references and full audio decoding: `20260913T154131-538d47c73c74`. The preceding diagnostic assertion incorrectly expected only inter-line separators; it was corrected to include UltraStar's final line-end marker, not by changing the export.
- Post-fix applicability of the real run: `20260913T155249-49e4568d7f1f` inspected all 540 selected candidates, including 300 word-owned notes, and found no stale ownership. The later unsplit-note repair therefore does not change this non-quantized result; no additional GPU execution was needed.
- Actual game captures: `20260913T153346-8a78f295c185`.
- Final grid wording tests: three passed; release desktop build passed — `20260913T154856-2da2f02eb507`. The preceding final engine/desktop build also passed — `20260913T154523-090455078a81`.
- Actual Workflow window persistence: `20260913T155126-615f5e8d8ac1`. The isolated fixture enabled and saved quantization, navigated away/back, restarted with it still enabled, then disabled and saved it. SQLite reads after each launch confirmed the expected per-song value, unchanged global `false`, and an empty analysis queue. Screenshots and command reports are in `test-artifacts/workflow-quantization-ui/` (`enable.png`, `reopen.png`, `disable.png`). No model inference ran during this UI check.
- Case-insensitive product identity scan passed — `20260913T154857-dc9adaa282a2`.

The verified development executables are `target/release/uta-studio` and `target/release/uta-analyze`. The existing Nix `result` link and already-running desktop were not replaced or restarted; opening that previous package does not load this development build's new control.

## Remaining quality boundaries

Sentence layout is repaired; ASR text accuracy and unresolved alignment are not repaired by splitting. Three generated lines have no timed words to publish. Their words remain in transcript/alignment evidence, but this change does not invent timings or recover them into a scored UTZ phrase. Supplied accurate lyrics and a subsequent alignment run remain the appropriate correction route. Unpunctuated, unbroken text has no reliable textual sentence boundaries to infer.

The observed analysis completed with the same boot ID and sampled the configured native GPU libraries. This establishes this operation's completion only, not GPU safety, numerical parity or a speedup. No whole-workspace release checks, Nix packaging, Windows GUI validation or readiness promotion were performed. Other user-staged changes remain uncommitted by this task.
