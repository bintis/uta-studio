# Documentation Center and Artifact Workbench walkthrough

Use this checklist on a machine with a display. Automated tests cover the
logic; this file records the remaining manual evidence for Phases 0, 2, and 15.

Interface language is **Settings → General → Interface language**. Repeat the
Documentation steps in English, Simplified Chinese, and Japanese.

## Scenario A — Documentation

1. Switch the UI to Simplified Chinese.
2. Open Settings → User guide, or press F1.
3. Search `对齐`.
4. Jump to the timing section.
5. Follow an internal `guide:` or `node:` link.
6. Navigate back, then forward.
7. Resize to a narrow window and confirm the three columns collapse.
8. Increase font size in Settings and confirm the article still scrolls.
9. Open the Editor with unsaved changes and press F1.
10. Confirm leave protection appears.
11. Repeat in English and Japanese.

## Scenario B — Exact run I/O

1. Analyze a song.
2. Select the completed run.
3. Select `lyrics.align`.
4. Confirm Inputs/Outputs show exact revision IDs when the banner says exact bindings.
5. Run again with cache reuse and confirm a Reused / untracked or Resolved reuse binding.
6. Freeze stems and run; confirm Frozen edge/node styling.
7. Bypass stems and run; confirm Bypassed styling and SourceMedia explanation.

## Scenario C — Immutable revision

1. Record PitchTrack revision A and pin it.
2. Run pitch analysis again to produce B.
3. Confirm A bytes and hash are unchanged.
4. Set B Active.
5. Confirm A remains pinned and readable.
6. Clear generated cache and confirm A survives.

## Scenario D — Lyrics edit

1. Open a RecognizedText revision in the compatible editor.
2. Promote to a lyrics draft, correct text, and choose Save Only.
3. Confirm a new LyricsInput revision exists and nothing was queued.
4. Choose Save and Run Downstream, review Impact, and confirm.
5. Confirm the queued request matches the preview and the Authored Chart stays preserved.

## Scenario E — TimedTranscript

1. Open a word-timed transcript.
2. Move one word boundary and save.
3. Reload the new revision.
4. Confirm unrelated word timings and extension fields are unchanged.

## Scenario F — Historical Artifact

1. Select an old run.
2. Right-click its TimedTranscript artifact node.
3. Confirm the menu references that run’s revision.
4. Switch to the latest run and confirm the revision changes.
5. Confirm Active is labelled separately.

## Scenario G — Lineage and Impact

1. Select AuthoredChart and turn Lineage on.
2. Confirm upstream transcript and pitch paths highlight and unrelated nodes fade.
3. Confirm the same full-workflow graph remains visible while lineage is active.
4. Select a missing legacy input and confirm an explicit GAP.
5. Open Impact on an upstream revision.
6. Cancel and confirm no mutation.
7. Open Impact again and choose Queue this plan only when you intend to run it.

## Packaging notes

- Linux: `nix build path:.#uta-studio` is the supported package path.
- Windows portable ZIP is not verified from this Linux workspace.
- The docs bundle is embedded; no source Markdown is required at runtime.
