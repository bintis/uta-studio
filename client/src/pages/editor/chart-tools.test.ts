import assert from "node:assert/strict";
import test from "node:test";
import type { PitchNote } from "@/types/PitchGuide";
import type { Transcript } from "@/types/Transcript";
import {
  analyzeChart,
  copyNotes,
  mergeNotes,
  mergePhraseWithNext,
  mergeWordWithNext,
  pasteNotes,
  quantizeNotes,
  repairChart,
  splitNotes,
  splitPhraseAfterWord,
  splitWord,
} from "./chart-tools.ts";
import {
  assignTimedItemLanes,
  clampPitchCenter,
  clampPitchSpan,
  clampViewStart,
  pitchRange,
  zoomTimeAroundPointer,
} from "./chart-viewport.ts";

const notes: PitchNote[] = [
  { start: 1, end: 1.5, midi: 60, confidence: 0.9, kind: "golden" },
  { start: 1.6, end: 2, midi: 62, confidence: 0.8 },
];

const transcript: Transcript = {
  language: "en",
  segments: [
    {
      text: "hello world",
      start: 1,
      end: 2,
      words: [
        { word: "hello", start: 1, end: 1.5 },
        { word: "world", start: 1.6, end: 2 },
      ],
    },
  ],
};

test("split and merge preserve the authored range and primary note metadata", () => {
  const split = splitNotes(notes, new Set([0]), 1.25);
  assert.equal(split.notes.length, 3);
  assert.deepEqual([...split.selected], [0, 1]);
  assert.equal(split.notes[0].end, 1.25);
  assert.equal(split.notes[1].start, 1.25);

  const merged = mergeNotes(split.notes, split.selected, 0);
  assert.ok(merged);
  assert.equal(merged.notes.length, 2);
  assert.equal(merged.notes[0].start, 1);
  assert.equal(merged.notes[0].end, 1.5);
  assert.equal(merged.notes[0].kind, "golden");
});

test("copy and paste retain relative rhythm and select inserted notes", () => {
  const copied = copyNotes(notes, new Set([0, 1]));
  assert.equal(copied[0].start, 0);
  assert.equal(copied[1].start, 0.6);
  const pasted = pasteNotes(notes, copied, 3);
  assert.equal(pasted.selected.size, 2);
  assert.deepEqual(
    [...pasted.selected].map((index) => pasted.notes[index].start),
    [3, 3.6],
  );
});

test("quantization snaps both boundaries while keeping valid durations", () => {
  const quantized = quantizeNotes(
    [{ start: 1.023, end: 1.071, midi: 60, confidence: 1 }],
    null,
    0.05,
  );
  assert.equal(quantized[0].start, 1);
  assert.equal(quantized[0].end, 1.05);
});

test("word and phrase editing rebuilds readable segment text", () => {
  const split = splitWord(transcript, { segment: 0, word: 0 }, 1.25);
  assert.ok(split);
  assert.equal(split.transcript.segments[0].words.length, 3);
  const mergedWord = mergeWordWithNext(split.transcript, { segment: 0, word: 0 });
  assert.ok(mergedWord);
  assert.equal(mergedWord.transcript.segments[0].words.length, 2);

  const splitPhrase = splitPhraseAfterWord(transcript, { segment: 0, word: 0 });
  assert.ok(splitPhrase);
  assert.equal(splitPhrase.transcript.segments.length, 2);
  const mergedPhrase = mergePhraseWithNext(splitPhrase.transcript, 0);
  assert.ok(mergedPhrase);
  assert.equal(mergedPhrase.transcript.segments.length, 1);
  assert.equal(mergedPhrase.transcript.segments[0].text, "hello world");
});

test("issue analysis finds overlaps and safe repair resolves them", () => {
  const overlapping = [
    { start: 1, end: 1.7, midi: 60, confidence: 1 },
    { start: 1.5, end: 2, midi: 61, confidence: 1 },
  ];
  assert.ok(analyzeChart(transcript, overlapping).some((issue) => issue.id.startsWith("overlap")));
  const repaired = repairChart(transcript, overlapping);
  assert.ok(repaired.notes[0].end <= repaired.notes[1].start);
  assert.equal(
    analyzeChart(repaired.transcript, repaired.notes).filter((issue) =>
      issue.id.startsWith("overlap"),
    ).length,
    0,
  );
});

test("issue analysis marks lyrics that have no overlapping note", () => {
  const sparseNotes = [{ start: 1, end: 1.5, midi: 60, confidence: 1 }];
  const issues = analyzeChart(transcript, sparseNotes);
  const missing = issues.find((issue) => issue.id === "unpitched-word-0-1");
  assert.equal(missing?.severity, "warning");
  assert.deepEqual(missing?.wordSelection, { segment: 0, word: 1 });
});

test("chart viewport clamps time and pitch at every media boundary", () => {
  assert.equal(clampViewStart(-10, 120, 600, 60), 0);
  assert.equal(clampViewStart(999, 120, 600, 60), 110);
  assert.equal(clampPitchSpan(2), 8);
  assert.equal(clampPitchSpan(200), 72);
  assert.equal(clampPitchCenter(-20, 24), 12);
  assert.equal(clampPitchCenter(200, 24), 115);
  assert.deepEqual(pitchRange(4, 24), { min: 0, max: 24 });
  assert.deepEqual(pitchRange(124, 24), { min: 103, max: 127 });
});

test("time zoom preserves the song time beneath the pointer", () => {
  const result = zoomTimeAroundPointer({
    viewStart: 20,
    pointerX: 300,
    currentZoom: 60,
    nextZoom: 120,
    duration: 180,
    width: 900,
  });
  assert.equal(result.zoom, 120);
  assert.equal(result.viewStart, 22.5);

  const clamped = zoomTimeAroundPointer({
    viewStart: 0,
    pointerX: 0,
    currentZoom: 60,
    nextZoom: 1,
    duration: 180,
    width: 900,
  });
  assert.equal(clamped.zoom, 30);
  assert.equal(clamped.viewStart, 0);
});

test("overlapping lyric timings are assigned separate visual lanes", () => {
  const layout = assignTimedItemLanes(
    [
      { start: 1, end: 1.05 },
      { start: 1.04, end: 1.2 },
      { start: 1.5, end: 1.7 },
    ],
    0.1,
  );
  assert.equal(layout.count, 2);
  assert.notEqual(layout.lanes[0], layout.lanes[1]);
  assert.equal(layout.lanes[0], layout.lanes[2]);
});
