import type { PitchNote, PitchNoteKind } from "@/types/PitchGuide";
import type { Transcript, Word } from "@/types/Transcript";

export type WordSelection = { segment: number; word: number };

export type ChartIssue = {
  id: string;
  severity: "error" | "warning" | "info";
  title: string;
  description: string;
  time: number;
  noteIndices?: number[];
  wordSelection?: WordSelection;
  autoFixable?: boolean;
};

export const NOTE_KIND_OPTIONS: ReadonlyArray<{ value: PitchNoteKind; label: string }> = [
  { value: "normal", label: "Normal" },
  { value: "golden", label: "Golden" },
  { value: "freestyle", label: "Freestyle" },
  { value: "rap", label: "Rap" },
  { value: "golden_rap", label: "Golden rap" },
];

const MIN_NOTE_SECONDS = 0.03;
const NOTE_GAP_SECONDS = 0.01;

const round = (value: number, digits = 3): number => Number(value.toFixed(digits));
const clamp = (value: number, minimum: number, maximum: number): number =>
  Math.max(minimum, Math.min(maximum, value));

export const noteKind = (note: PitchNote): PitchNoteKind => note.kind ?? "normal";

export const noteKindSymbol = (kind: PitchNoteKind): string => {
  switch (kind) {
    case "golden":
      return "★";
    case "freestyle":
      return "F";
    case "rap":
      return "R";
    case "golden_rap":
      return "G";
    default:
      return "";
  }
};

export function quantizeNote(note: PitchNote, gridSeconds: number): PitchNote {
  if (gridSeconds <= 0) return { ...note };
  const start = round(Math.max(0, Math.round(note.start / gridSeconds) * gridSeconds));
  const snappedEnd = round(Math.round(note.end / gridSeconds) * gridSeconds);
  return {
    ...note,
    start,
    end: round(Math.max(start + Math.max(MIN_NOTE_SECONDS, gridSeconds), snappedEnd)),
  };
}

export function quantizeNotes(
  notes: PitchNote[],
  indices: ReadonlySet<number> | null,
  gridSeconds: number,
): PitchNote[] {
  return notes.map((note, index) =>
    indices === null || indices.has(index) ? quantizeNote(note, gridSeconds) : { ...note },
  );
}

export function splitNotes(
  notes: PitchNote[],
  indices: ReadonlySet<number>,
  playhead: number,
): { notes: PitchNote[]; selected: Set<number> } {
  const next: PitchNote[] = [];
  const selected = new Set<number>();
  notes.forEach((note, index) => {
    if (!indices.has(index) || note.end - note.start < MIN_NOTE_SECONDS * 2) {
      next.push({ ...note });
      if (indices.has(index)) selected.add(next.length - 1);
      return;
    }
    const splitAt =
      playhead > note.start + MIN_NOTE_SECONDS && playhead < note.end - MIN_NOTE_SECONDS
        ? playhead
        : (note.start + note.end) / 2;
    next.push({ ...note, end: round(splitAt) });
    selected.add(next.length - 1);
    next.push({ ...note, start: round(splitAt) });
    selected.add(next.length - 1);
  });
  return { notes: next, selected };
}

export function mergeNotes(
  notes: PitchNote[],
  indices: ReadonlySet<number>,
  primaryIndex: number | null,
): { notes: PitchNote[]; selected: Set<number> } | null {
  const ordered = [...indices].filter((index) => notes[index]).sort((a, b) => a - b);
  if (ordered.length < 2) return null;
  const selectedNotes = ordered.map((index) => notes[index]);
  const primary =
    primaryIndex !== null && indices.has(primaryIndex) ? notes[primaryIndex] : notes[ordered[0]];
  const totalDuration = selectedNotes.reduce((sum, note) => sum + note.end - note.start, 0);
  const confidence =
    totalDuration > 0
      ? selectedNotes.reduce((sum, note) => sum + note.confidence * (note.end - note.start), 0) /
        totalDuration
      : primary.confidence;
  const merged: PitchNote = {
    ...primary,
    start: round(Math.min(...selectedNotes.map((note) => note.start))),
    end: round(Math.max(...selectedNotes.map((note) => note.end))),
    confidence: round(confidence, 4),
  };
  const first = ordered[0];
  const output = notes.filter((_, index) => !indices.has(index)).map((note) => ({ ...note }));
  const insertion = notes.slice(0, first).filter((_, index) => !indices.has(index)).length;
  output.splice(insertion, 0, merged);
  return { notes: output, selected: new Set([insertion]) };
}

export function copyNotes(notes: PitchNote[], indices: ReadonlySet<number>): PitchNote[] {
  const copied = [...indices]
    .filter((index) => notes[index])
    .map((index) => ({ ...notes[index] }))
    .sort((left, right) => left.start - right.start);
  if (copied.length === 0) return [];
  const origin = copied[0].start;
  return copied.map((note) => ({
    ...note,
    start: round(note.start - origin),
    end: round(note.end - origin),
  }));
}

export function pasteNotes(
  notes: PitchNote[],
  clipboard: PitchNote[],
  at: number,
): { notes: PitchNote[]; selected: Set<number> } {
  const inserted = clipboard.map((note) => ({
    ...note,
    start: round(Math.max(0, at + note.start)),
    end: round(Math.max(MIN_NOTE_SECONDS, at + note.end)),
  }));
  const combined = [...notes.map((note) => ({ ...note })), ...inserted].sort(
    (left, right) => left.start - right.start || left.end - right.end,
  );
  return {
    notes: combined,
    selected: new Set(combined.flatMap((note, index) => (inserted.includes(note) ? [index] : []))),
  };
}

export function shiftNotes(
  notes: PitchNote[],
  indices: ReadonlySet<number> | null,
  seconds: number,
  semitones = 0,
): PitchNote[] {
  const selected = notes.filter((_, index) => indices === null || indices.has(index));
  const earliest = selected.length > 0 ? Math.min(...selected.map((note) => note.start)) : 0;
  const safeSeconds = Math.max(seconds, -earliest);
  return notes.map((note, index) => {
    if (indices !== null && !indices.has(index)) return { ...note };
    return {
      ...note,
      start: round(note.start + safeSeconds),
      end: round(note.end + safeSeconds),
      midi: clamp(Math.round(note.midi + semitones), 0, 127),
    };
  });
}

export function setNoteKind(
  notes: PitchNote[],
  indices: ReadonlySet<number>,
  kind: PitchNoteKind,
): PitchNote[] {
  return notes.map((note, index) => (indices.has(index) ? { ...note, kind } : { ...note }));
}

export function shiftTranscript(transcript: Transcript, seconds: number): Transcript {
  const earliest = transcript.segments.length > 0 ? transcript.segments[0].start : 0;
  const safeSeconds = Math.max(seconds, -earliest);
  return {
    ...transcript,
    segments: transcript.segments.map((segment) => ({
      ...segment,
      start: round(segment.start + safeSeconds),
      end: round(segment.end + safeSeconds),
      words: segment.words.map((word) => ({
        ...word,
        start: round(word.start + safeSeconds),
        end: round(word.end + safeSeconds),
      })),
    })),
  };
}

const compactLanguage = (language: string): boolean => /^(zh|ja|ko)/i.test(language);

export function rebuildSegmentText(words: Word[], language: string): string {
  return compactLanguage(language)
    ? words.map((word) => word.word.trim()).join("")
    : words
        .map((word) => word.word.trim())
        .join(" ")
        .replace(/\s+([,.!?;:])/g, "$1");
}

export function splitWord(
  transcript: Transcript,
  selection: WordSelection,
  playhead: number,
): { transcript: Transcript; selected: WordSelection } | null {
  const output = structuredClone(transcript);
  const segment = output.segments[selection.segment];
  const word = segment?.words[selection.word];
  if (!word || word.end - word.start < 0.04) return null;
  const splitAt =
    playhead > word.start + 0.02 && playhead < word.end - 0.02
      ? playhead
      : (word.start + word.end) / 2;
  const characters = Array.from(word.word);
  const textIndex = Math.max(1, Math.min(characters.length - 1, Math.round(characters.length / 2)));
  const left = characters.length > 1 ? characters.slice(0, textIndex).join("") : word.word;
  const right = characters.length > 1 ? characters.slice(textIndex).join("") : "";
  segment.words.splice(
    selection.word,
    1,
    { ...word, word: left, end: round(splitAt) },
    { ...word, word: right, start: round(splitAt) },
  );
  segment.start = segment.words[0].start;
  segment.end = segment.words[segment.words.length - 1].end;
  segment.text = rebuildSegmentText(segment.words, output.language);
  return { transcript: output, selected: { ...selection, word: selection.word + 1 } };
}

export function mergeWordWithNext(
  transcript: Transcript,
  selection: WordSelection,
): { transcript: Transcript; selected: WordSelection } | null {
  const output = structuredClone(transcript);
  const segment = output.segments[selection.segment];
  const left = segment?.words[selection.word];
  const right = segment?.words[selection.word + 1];
  if (!left || !right) return null;
  const joiner = compactLanguage(output.language) ? "" : " ";
  segment.words.splice(selection.word, 2, {
    ...left,
    word: `${left.word.trim()}${joiner}${right.word.trim()}`,
    end: right.end,
  });
  segment.end = segment.words[segment.words.length - 1].end;
  segment.text = rebuildSegmentText(segment.words, output.language);
  return { transcript: output, selected: selection };
}

export function splitPhraseAfterWord(
  transcript: Transcript,
  selection: WordSelection,
): { transcript: Transcript; selected: WordSelection } | null {
  const output = structuredClone(transcript);
  const segment = output.segments[selection.segment];
  if (!segment || selection.word >= segment.words.length - 1) return null;
  const leftWords = segment.words.slice(0, selection.word + 1);
  const rightWords = segment.words.slice(selection.word + 1);
  output.segments.splice(
    selection.segment,
    1,
    {
      ...segment,
      words: leftWords,
      start: leftWords[0].start,
      end: leftWords[leftWords.length - 1].end,
      text: rebuildSegmentText(leftWords, output.language),
    },
    {
      ...segment,
      words: rightWords,
      start: rightWords[0].start,
      end: rightWords[rightWords.length - 1].end,
      text: rebuildSegmentText(rightWords, output.language),
    },
  );
  return { transcript: output, selected: { segment: selection.segment + 1, word: 0 } };
}

export function mergePhraseWithNext(
  transcript: Transcript,
  segmentIndex: number,
): { transcript: Transcript; selected: WordSelection } | null {
  const output = structuredClone(transcript);
  const left = output.segments[segmentIndex];
  const right = output.segments[segmentIndex + 1];
  if (!left || !right) return null;
  const leftWordCount = left.words.length;
  const words = [...left.words, ...right.words].sort((a, b) => a.start - b.start);
  output.segments.splice(segmentIndex, 2, {
    ...left,
    words,
    start: words[0]?.start ?? left.start,
    end: words[words.length - 1]?.end ?? right.end,
    text: rebuildSegmentText(words, output.language),
  });
  return {
    transcript: output,
    selected: { segment: segmentIndex, word: Math.max(0, leftWordCount - 1) },
  };
}

export function analyzeChart(transcript: Transcript, notes: PitchNote[]): ChartIssue[] {
  const issues: ChartIssue[] = [];
  const lowConfidence: number[] = [];
  const shortNotes: number[] = [];
  const pitchJumps: number[] = [];

  notes.forEach((note, index) => {
    if (note.confidence < 0.55) lowConfidence.push(index);
    if (note.end - note.start < 0.06) shortNotes.push(index);
    const previous = notes[index - 1];
    if (previous && note.start < previous.start) {
      issues.push({
        id: `order-${index}`,
        severity: "error",
        title: "Notes are out of order",
        description: "Chronological order is required for reliable editing and export.",
        time: note.start,
        noteIndices: [index - 1, index],
        autoFixable: true,
      });
    }
    if (previous && note.start < previous.end - 0.001) {
      issues.push({
        id: `overlap-${index}`,
        severity: "error",
        title: "Pitch notes overlap",
        description: `${Math.round((previous.end - note.start) * 1000)} ms overlap can produce ambiguous scoring.`,
        time: note.start,
        noteIndices: [index - 1, index],
        autoFixable: true,
      });
    }
    if (previous && note.start - previous.end < 0.25 && Math.abs(note.midi - previous.midi) > 12) {
      pitchJumps.push(index);
    }
  });

  if (lowConfidence.length > 0) {
    issues.push({
      id: "low-confidence",
      severity: "warning",
      title: `${lowConfidence.length} low-confidence note${lowConfidence.length === 1 ? "" : "s"}`,
      description: "Compare these bars with the raw pitch trace and isolated vocals.",
      time: notes[lowConfidence[0]].start,
      noteIndices: lowConfidence,
    });
  }
  if (shortNotes.length > 0) {
    issues.push({
      id: "short-notes",
      severity: "warning",
      title: `${shortNotes.length} very short note${shortNotes.length === 1 ? "" : "s"}`,
      description:
        "Notes shorter than 60 ms are difficult to sing and may be accidental fragments.",
      time: notes[shortNotes[0]].start,
      noteIndices: shortNotes,
    });
  }
  if (pitchJumps.length > 0) {
    issues.push({
      id: "pitch-jumps",
      severity: "warning",
      title: `${pitchJumps.length} abrupt pitch jump${pitchJumps.length === 1 ? "" : "s"}`,
      description:
        "Check octave-sized jumps against the pitch evidence; they are common detector errors.",
      time: notes[pitchJumps[0]].start,
      noteIndices: pitchJumps,
    });
  }

  transcript.segments.forEach((segment, segmentIndex) => {
    segment.words.forEach((word, wordIndex) => {
      if (!word.word.trim()) {
        issues.push({
          id: `empty-word-${segmentIndex}-${wordIndex}`,
          severity: "error",
          title: "Empty lyric word",
          description: "Enter text or merge this timing block with the next word.",
          time: word.start,
          wordSelection: { segment: segmentIndex, word: wordIndex },
        });
      }
      const previous = segment.words[wordIndex - 1];
      if (previous && word.start < previous.end - 0.001) {
        issues.push({
          id: `word-overlap-${segmentIndex}-${wordIndex}`,
          severity: "error",
          title: "Lyric timings overlap",
          description: "Adjacent word blocks should have a single, unambiguous boundary.",
          time: word.start,
          wordSelection: { segment: segmentIndex, word: wordIndex },
          autoFixable: true,
        });
      }
      const hasPitch = notes.some(
        (note) => Math.min(note.end, word.end) - Math.max(note.start, word.start) > 0.015,
      );
      if (!hasPitch) {
        issues.push({
          id: `unpitched-word-${segmentIndex}-${wordIndex}`,
          severity: "warning",
          title: "Lyric has no pitch note",
          description: "Add or extend a note here so this sung lyric is represented in the chart.",
          time: word.start,
          wordSelection: { segment: segmentIndex, word: wordIndex },
        });
      }
    });
  });

  return issues.sort(
    (left, right) =>
      ({ error: 0, warning: 1, info: 2 })[left.severity] -
        { error: 0, warning: 1, info: 2 }[right.severity] || left.time - right.time,
  );
}

export function repairChart(
  transcript: Transcript,
  notes: PitchNote[],
): { transcript: Transcript; notes: PitchNote[] } {
  const repairedNotes = notes
    .map((note) => ({
      ...note,
      start: round(Math.max(0, note.start)),
      end: round(Math.max(note.start + MIN_NOTE_SECONDS, note.end)),
      midi: clamp(Math.round(note.midi), 0, 127),
      confidence: clamp(note.confidence, 0, 1),
    }))
    .sort((left, right) => left.start - right.start || left.end - right.end);

  for (let index = 1; index < repairedNotes.length; index += 1) {
    const previous = repairedNotes[index - 1];
    const current = repairedNotes[index];
    if (current.start >= previous.end) continue;
    const roomForBoundary = previous.start + MIN_NOTE_SECONDS + NOTE_GAP_SECONDS;
    if (roomForBoundary <= current.end - MIN_NOTE_SECONDS) {
      const boundary = clamp(
        (previous.end + current.start) / 2,
        previous.start + MIN_NOTE_SECONDS,
        current.end - MIN_NOTE_SECONDS - NOTE_GAP_SECONDS,
      );
      previous.end = round(boundary);
      current.start = round(boundary + NOTE_GAP_SECONDS);
    } else {
      current.start = round(previous.end + NOTE_GAP_SECONDS);
      current.end = round(Math.max(current.end, current.start + MIN_NOTE_SECONDS));
    }
  }

  const repairedTranscript = structuredClone(transcript);
  repairedTranscript.segments.forEach((segment) => {
    segment.words.sort((left, right) => left.start - right.start);
    segment.words.forEach((word, index) => {
      word.start = round(Math.max(0, word.start));
      word.end = round(Math.max(word.start + 0.02, word.end));
      const previous = segment.words[index - 1];
      if (previous && word.start < previous.end) {
        const boundary = round((previous.end + word.start) / 2);
        previous.end = Math.max(previous.start + 0.01, boundary);
        word.start = Math.min(word.end - 0.01, boundary);
      }
    });
    if (segment.words.length > 0) {
      segment.start = segment.words[0].start;
      segment.end = segment.words[segment.words.length - 1].end;
      segment.text = rebuildSegmentText(segment.words, repairedTranscript.language);
    }
  });
  repairedTranscript.segments.sort((left, right) => left.start - right.start);
  return { transcript: repairedTranscript, notes: repairedNotes };
}
