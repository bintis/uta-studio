import type { PitchNote, PitchTrack } from "@/types/PitchGuide";
import type { Transcript } from "@/types/Transcript";
import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Minus, Move, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { noteKind, noteKindSymbol } from "./chart-tools";
import {
  assignTimedItemLanes,
  clampPitchCenter,
  clampPitchSpan,
  clampTimeZoom,
  clampViewStart,
  maximumViewStart,
  pitchRange,
  visibleTime,
  zoomTimeAroundPointer,
} from "./chart-viewport";
import type { AudioWaveform } from "./use-audio-waveform";

const RULER_HEIGHT = 28;
const WAVEFORM_HEIGHT = 58;
const PITCH_HEIGHT = 260;
const WORD_HEIGHT = 68;
const WORD_BOX_HEIGHT = 36;
const WORD_LANE_GAP = 4;
const WORD_AREA_PADDING = 8;
const PITCH_TOP = RULER_HEIGHT + WAVEFORM_HEIGHT;
const WORD_TOP = PITCH_TOP + PITCH_HEIGHT;
const TOTAL_HEIGHT = WORD_TOP + WORD_HEIGHT;

type NoteDrag = {
  kind: "notes";
  indices: number[];
  primary: number;
  mode: "move" | "start" | "end";
  pointerX: number;
  pointerY: number;
  originals: Map<number, PitchNote>;
  started: boolean;
};

type MarqueeDrag = {
  kind: "marquee";
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
  base: Set<number>;
};

type PanDrag = {
  kind: "pan";
  pointerX: number;
  pointerY: number;
  viewStart: number;
  pitchCenter: number;
};

type DragState = NoteDrag | MarqueeDrag | PanDrag | { kind: "seek" };

type ChartTimelineProps = {
  notes: PitchNote[];
  track: PitchTrack;
  transcript: Transcript;
  waveform: AudioWaveform | null;
  waveformLoading: boolean;
  playhead: number;
  playing: boolean;
  selectedNotes: ReadonlySet<number>;
  primaryNote: number | null;
  selectedWord: { segment: number; word: number } | null;
  zoom: number;
  duration: number;
  snapSeconds: number;
  expanded?: boolean;
  onSeek: (time: number, immediate?: boolean) => void;
  onZoomChange: (zoom: number) => void;
  onSelectNotes: (indices: Set<number>, primary: number | null) => void;
  onSelectWord: (selection: { segment: number; word: number } | null) => void;
  onChangeNotes: (changes: Array<{ index: number; note: PitchNote }>, begin: boolean) => void;
  onInteractionEnd: () => void;
  onAddNote: (time: number) => void;
};

const cssColor = (name: string, fallback: string): string => {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
};

const formatTime = (seconds: number): string => {
  const minutes = Math.floor(seconds / 60);
  const remainder = Math.max(0, seconds - minutes * 60);
  const value =
    remainder < 10
      ? remainder.toFixed(1).padStart(4, "0")
      : Math.round(remainder).toString().padStart(2, "0");
  return `${minutes}:${value}`;
};

const midiName = (midi: number): string => {
  const names = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
  return `${names[((midi % 12) + 12) % 12]}${Math.floor(midi / 12) - 1}`;
};

const noteColor = (note: PitchNote, primary: string, muted: string): string => {
  switch (noteKind(note)) {
    case "golden":
      return "#f4bc43";
    case "freestyle":
      return muted;
    case "rap":
      return "#c084fc";
    case "golden_rap":
      return "#fb923c";
    default:
      return primary;
  }
};

export const ChartTimeline = ({
  notes,
  track,
  transcript,
  waveform,
  waveformLoading,
  playhead,
  playing,
  selectedNotes,
  primaryNote,
  selectedWord,
  zoom,
  duration,
  snapSeconds,
  expanded = false,
  onSeek,
  onZoomChange,
  onSelectNotes,
  onSelectWord,
  onChangeNotes,
  onInteractionEnd,
  onAddNote,
}: ChartTimelineProps) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const shellRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const activePointerIdRef = useRef<number | null>(null);
  const manualScrollUntilRef = useRef(0);
  const wheelFrameRef = useRef<number | null>(null);
  const pendingWheelDeltaRef = useRef(0);
  const finishPointerRef = useRef<() => void>(() => undefined);
  const initialPitchRange = useMemo(() => {
    if (notes.length === 0) return { center: 60, span: 24 };
    const values = notes.map((note) => note.midi);
    const minimum = Math.min(...values);
    const maximum = Math.max(...values);
    return {
      center: (minimum + maximum) / 2,
      span: Math.max(12, Math.min(48, maximum - minimum + 7)),
    };
    // The initial viewport is intentionally captured once. Editing or moving
    // notes must never snap the user's view back to the automatic range.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const [width, setWidth] = useState(900);
  const [height, setHeight] = useState(TOTAL_HEIGHT);
  const [viewStart, setViewStart] = useState(0);
  const [pitchCenter, setPitchCenter] = useState(initialPitchRange.center);
  const [pitchSpan, setPitchSpan] = useState(initialPitchRange.span);
  const [dragVersion, setDragVersion] = useState(0);
  const playbackAnchorRef = useRef({ position: playhead, time: performance.now() });
  const playheadLineRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    playbackAnchorRef.current = { position: playhead, time: performance.now() };
  }, [playhead]);

  const midiRange = useMemo(() => pitchRange(pitchCenter, pitchSpan), [pitchCenter, pitchSpan]);
  const visibleSeconds = visibleTime(width, zoom);
  const maxViewStart = maximumViewStart(duration, width, zoom);
  const clampedViewStart = clampViewStart(viewStart, duration, width, zoom);

  const panTime = useCallback(
    (seconds: number) =>
      setViewStart((current) => clampViewStart(current + seconds, duration, width, zoom)),
    [duration, width, zoom],
  );
  const panPitch = useCallback(
    (semitones: number) =>
      setPitchCenter((current) => clampPitchCenter(current + semitones, pitchSpan)),
    [pitchSpan],
  );
  const zoomPitch = useCallback((delta: number) => {
    setPitchSpan((current) => {
      const next = clampPitchSpan(current + delta);
      setPitchCenter((center) => clampPitchCenter(center, next));
      return next;
    });
  }, []);

  useEffect(() => {
    const shell = shellRef.current;
    if (!shell) return;
    const observer = new ResizeObserver(([entry]) => {
      setWidth(Math.max(320, entry.contentRect.width));
      setHeight(expanded ? Math.max(TOTAL_HEIGHT, entry.contentRect.height) : TOTAL_HEIGHT);
    });
    observer.observe(shell);
    return () => observer.disconnect();
  }, [expanded]);

  const wordLayout = useMemo(() => {
    const lanes = new Map<string, number>();
    const words = transcript.segments
      .flatMap((segment, segmentIndex) =>
        segment.words.map((word, wordIndex) => ({ segmentIndex, wordIndex, word })),
      )
      .sort((left, right) => left.word.start - right.word.start || left.word.end - right.word.end);
    const layout = assignTimedItemLanes(
      words.map(({ word }) => word),
      8 / zoom,
    );
    words.forEach(({ segmentIndex, wordIndex }, index) =>
      lanes.set(`${segmentIndex}:${wordIndex}`, layout.lanes[index]),
    );

    return { lanes, count: layout.count };
  }, [transcript.segments, zoom]);
  const wordAreaHeight = Math.max(
    WORD_HEIGHT,
    WORD_AREA_PADDING * 2 +
      wordLayout.count * WORD_BOX_HEIGHT +
      (wordLayout.count - 1) * WORD_LANE_GAP,
  );
  const pitchHeight = Math.max(
    PITCH_HEIGHT,
    height - RULER_HEIGHT - WAVEFORM_HEIGHT - wordAreaHeight,
  );
  const wordTop = PITCH_TOP + pitchHeight;
  const totalHeight = wordTop + wordAreaHeight;

  useEffect(() => {
    if (
      performance.now() >= manualScrollUntilRef.current &&
      (playhead < clampedViewStart || playhead > clampedViewStart + visibleSeconds)
    ) {
      setViewStart(Math.max(0, Math.min(maxViewStart, playhead - visibleSeconds * 0.18)));
    }
  }, [clampedViewStart, maxViewStart, playhead, visibleSeconds]);

  const timeToX = useCallback(
    (time: number) => (time - clampedViewStart) * zoom,
    [clampedViewStart, zoom],
  );
  const xToTime = useCallback(
    (x: number) => Math.max(0, Math.min(duration, clampedViewStart + x / zoom)),
    [clampedViewStart, duration, zoom],
  );

  useEffect(() => {
    const line = playheadLineRef.current;
    if (!line) return;
    let frame = 0;
    const draw = (now: number) => {
      const anchor = playbackAnchorRef.current;
      const position = playing
        ? Math.min(duration, anchor.position + (now - anchor.time) / 1000)
        : playhead;
      const visible = position >= clampedViewStart && position <= clampedViewStart + visibleSeconds;
      line.style.visibility = visible ? "visible" : "hidden";
      if (visible) line.style.transform = `translate3d(${timeToX(position)}px, 0, 0)`;
      if (playing) frame = requestAnimationFrame(draw);
    };
    draw(performance.now());
    return () => cancelAnimationFrame(frame);
  }, [clampedViewStart, duration, playhead, playing, timeToX, visibleSeconds]);
  const snapTime = useCallback(
    (time: number) => (snapSeconds > 0 ? Math.round(time / snapSeconds) * snapSeconds : time),
    [snapSeconds],
  );
  const midiToY = useCallback(
    (midi: number) => {
      const span = Math.max(1, midiRange.max - midiRange.min + 1);
      return PITCH_TOP + ((midiRange.max + 0.5 - midi) / span) * pitchHeight;
    },
    [midiRange, pitchHeight],
  );
  const yToMidi = useCallback(
    (y: number) => {
      const span = Math.max(1, midiRange.max - midiRange.min + 1);
      return Math.round(midiRange.max + 0.5 - ((y - PITCH_TOP) / pitchHeight) * span);
    },
    [midiRange, pitchHeight],
  );

  const wordsWithoutNotes = useMemo(() => {
    const missing = new Set<string>();
    transcript.segments.forEach((segment, segmentIndex) => {
      segment.words.forEach((word, wordIndex) => {
        if (!word.word.trim()) return;
        const hasPitch = notes.some(
          (note) => Math.min(note.end, word.end) - Math.max(note.start, word.start) > 0.015,
        );
        if (!hasPitch) missing.add(`${segmentIndex}:${wordIndex}`);
      });
    });
    return missing;
  }, [notes, transcript.segments]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const scale = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * scale);
    canvas.height = Math.round(totalHeight * scale);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${totalHeight}px`;
    const context = canvas.getContext("2d");
    if (!context) return;
    context.setTransform(scale, 0, 0, scale, 0, 0);

    const background = cssColor("--card", "#161622");
    const foreground = cssColor("--foreground", "#f4f4f8");
    const muted = cssColor("--muted-foreground", "#a6a6b7");
    const border = cssColor("--border", "#3e3e52");
    const primary = cssColor("--primary", "#758fff");
    context.clearRect(0, 0, width, totalHeight);
    context.fillStyle = background;
    context.fillRect(0, 0, width, totalHeight);

    const major = zoom >= 110 ? 1 : zoom >= 55 ? 2 : 5;
    const firstTick = Math.floor(clampedViewStart / major) * major;
    context.font = "11px ui-sans-serif, system-ui";
    context.textBaseline = "middle";
    for (let time = firstTick; time <= clampedViewStart + visibleSeconds + major; time += major) {
      const x = timeToX(time);
      context.strokeStyle = border;
      context.globalAlpha = 0.5;
      context.beginPath();
      context.moveTo(Math.round(x) + 0.5, 0);
      context.lineTo(Math.round(x) + 0.5, totalHeight);
      context.stroke();
      context.globalAlpha = 1;
      context.fillStyle = muted;
      context.fillText(formatTime(time), x + 5, RULER_HEIGHT / 2);
    }

    context.fillStyle = cssColor("--muted", "#242432");
    context.globalAlpha = 0.44;
    context.fillRect(0, RULER_HEIGHT, width, WAVEFORM_HEIGHT);
    context.globalAlpha = 1;
    const waveMiddle = RULER_HEIGHT + WAVEFORM_HEIGHT / 2;
    context.strokeStyle = border;
    context.globalAlpha = 0.5;
    context.beginPath();
    context.moveTo(0, waveMiddle + 0.5);
    context.lineTo(width, waveMiddle + 0.5);
    context.stroke();
    if (waveform && waveform.duration > 0) {
      const bucketCount = waveform.peaks.length / 2;
      context.strokeStyle = primary;
      context.globalAlpha = 0.52;
      context.lineWidth = 1;
      context.beginPath();
      for (let x = 0; x < width; x += 1) {
        const time = xToTime(x);
        const bucket = Math.max(
          0,
          Math.min(bucketCount - 1, Math.floor((time / waveform.duration) * bucketCount)),
        );
        const minimum = waveform.peaks[bucket * 2];
        const maximum = waveform.peaks[bucket * 2 + 1];
        context.moveTo(x + 0.5, waveMiddle + minimum * (WAVEFORM_HEIGHT / 2 - 5));
        context.lineTo(x + 0.5, waveMiddle + maximum * (WAVEFORM_HEIGHT / 2 - 5));
      }
      context.stroke();
    } else if (waveformLoading) {
      context.fillStyle = muted;
      context.globalAlpha = 0.8;
      context.fillText("Decoding waveform…", 10, waveMiddle);
    }
    context.globalAlpha = 1;

    const pitchSpan = Math.max(1, midiRange.max - midiRange.min + 1);
    for (let midi = Math.ceil(midiRange.min); midi <= Math.floor(midiRange.max); midi += 1) {
      const y = midiToY(midi);
      const pitchClass = ((midi % 12) + 12) % 12;
      context.strokeStyle = border;
      context.globalAlpha = pitchClass === 0 ? 0.66 : pitchClass === 1 ? 0.3 : 0.17;
      context.beginPath();
      context.moveTo(0, y);
      context.lineTo(width, y);
      context.stroke();
      if (pitchClass === 0) {
        context.fillStyle = muted;
        context.globalAlpha = 0.72;
        context.fillText(midiName(midi), 6, y - 7);
      }
    }
    context.globalAlpha = 1;

    context.strokeStyle = muted;
    context.globalAlpha = 0.42;
    context.lineWidth = 1.4;
    context.beginPath();
    let drawing = false;
    for (const frame of track.frames) {
      if (frame.time < clampedViewStart || frame.time > clampedViewStart + visibleSeconds) continue;
      if (!frame.hz || frame.hz <= 0) {
        drawing = false;
        continue;
      }
      const midi = 69 + 12 * Math.log2(frame.hz / 440);
      const x = timeToX(frame.time);
      const y = PITCH_TOP + ((midiRange.max + 0.5 - midi) / pitchSpan) * pitchHeight;
      if (drawing) context.lineTo(x, y);
      else {
        context.moveTo(x, y);
        drawing = true;
      }
    }
    context.stroke();
    context.globalAlpha = 1;
    context.lineWidth = 1;

    notes.forEach((note, index) => {
      if (note.end < clampedViewStart || note.start > clampedViewStart + visibleSeconds) return;
      const x = timeToX(note.start);
      const noteWidth = Math.max(4, (note.end - note.start) * zoom);
      const rowHeight = pitchHeight / pitchSpan;
      const y = midiToY(note.midi) - rowHeight * 0.38;
      const height = Math.max(7, rowHeight * 0.76);
      const selected = selectedNotes.has(index);
      context.fillStyle = selected ? foreground : noteColor(note, primary, muted);
      context.globalAlpha = selected ? 0.98 : 0.48 + note.confidence * 0.48;
      context.beginPath();
      context.roundRect(x, y, noteWidth, height, Math.min(5, height / 2));
      context.fill();
      if (selected) {
        context.strokeStyle = index === primaryNote ? primary : noteColor(note, primary, muted);
        context.globalAlpha = 1;
        context.lineWidth = index === primaryNote ? 2.5 : 1.5;
        context.stroke();
        context.lineWidth = 1;
      }
      const symbol = noteKindSymbol(noteKind(note));
      if (symbol && noteWidth >= 18) {
        context.fillStyle = selected ? background : foreground;
        context.globalAlpha = 0.9;
        context.font = "bold 9px ui-sans-serif, system-ui";
        context.fillText(symbol, x + 5, y + height / 2 + 0.5);
      }
    });
    context.globalAlpha = 1;

    context.fillStyle = cssColor("--muted", "#242432");
    context.fillRect(0, wordTop, width, wordAreaHeight);
    transcript.segments.forEach((segment, segmentIndex) => {
      segment.words.forEach((word, wordIndex) => {
        if (word.end < clampedViewStart || word.start > clampedViewStart + visibleSeconds) return;
        const x = timeToX(word.start);
        const wordWidth = Math.max(8, (word.end - word.start) * zoom);
        const lane = wordLayout.lanes.get(`${segmentIndex}:${wordIndex}`) ?? 0;
        const wordY = wordTop + WORD_AREA_PADDING + lane * (WORD_BOX_HEIGHT + WORD_LANE_GAP);
        const active = selectedWord?.segment === segmentIndex && selectedWord.word === wordIndex;
        context.fillStyle = active ? primary : background;
        context.strokeStyle = active ? primary : border;
        context.globalAlpha = active ? 0.92 : 0.9;
        const missingNote = wordsWithoutNotes.has(`${segmentIndex}:${wordIndex}`);
        context.beginPath();
        context.roundRect(x, wordY, wordWidth, WORD_BOX_HEIGHT, 7);
        context.fill();
        if (missingNote && !active) {
          context.strokeStyle = "#f59e0b";
          context.globalAlpha = 0.82;
          context.setLineDash([3, 2]);
        }
        context.stroke();
        context.setLineDash([]);
        context.save();
        context.beginPath();
        context.rect(x + 5, wordY, Math.max(0, wordWidth - 10), WORD_BOX_HEIGHT);
        context.clip();
        context.fillStyle = active ? "white" : foreground;
        context.globalAlpha = 1;
        context.font = "12px ui-sans-serif, system-ui";
        context.fillText(word.word || "…", x + 7, wordY + WORD_BOX_HEIGHT / 2);
        context.restore();
        if (missingNote && wordWidth >= 14) {
          context.fillStyle = "#f59e0b";
          context.globalAlpha = 1;
          context.font = "bold 10px ui-sans-serif, system-ui";
          context.fillText("!", x + Math.max(7, wordWidth - 9), wordY + 9);
        }
      });
      const phraseX = timeToX(segment.end);
      context.strokeStyle = primary;
      context.globalAlpha = 0.45;
      context.setLineDash([3, 3]);
      context.beginPath();
      context.moveTo(phraseX, wordTop + 5);
      context.lineTo(phraseX, totalHeight - 5);
      context.stroke();
      context.setLineDash([]);
    });
    context.globalAlpha = 1;

    const drag = dragRef.current;
    if (drag?.kind === "marquee") {
      const left = Math.min(drag.startX, drag.currentX);
      const top = Math.min(drag.startY, drag.currentY);
      const selectionWidth = Math.abs(drag.currentX - drag.startX);
      const selectionHeight = Math.abs(drag.currentY - drag.startY);
      context.fillStyle = primary;
      context.globalAlpha = 0.13;
      context.fillRect(left, top, selectionWidth, selectionHeight);
      context.globalAlpha = 0.85;
      context.strokeStyle = primary;
      context.setLineDash([5, 3]);
      context.strokeRect(left + 0.5, top + 0.5, selectionWidth, selectionHeight);
      context.setLineDash([]);
      context.globalAlpha = 1;
    }
  }, [
    clampedViewStart,
    dragVersion,
    midiRange,
    midiToY,
    notes,
    primaryNote,
    selectedNotes,
    selectedWord,
    timeToX,
    track.frames,
    transcript.segments,
    visibleSeconds,
    waveform,
    waveformLoading,
    pitchHeight,
    totalHeight,
    width,
    wordTop,
    wordLayout,
    wordAreaHeight,
    wordsWithoutNotes,
    xToTime,
    zoom,
  ]);

  const pointerPosition = (
    event: React.PointerEvent<HTMLCanvasElement> | React.MouseEvent<HTMLCanvasElement>,
  ) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return { x: event.clientX - bounds.left, y: event.clientY - bounds.top };
  };

  const noteAt = (x: number, y: number): number | null => {
    if (y < PITCH_TOP || y > wordTop) return null;
    const pitchSpan = Math.max(1, midiRange.max - midiRange.min + 1);
    const hitRadius = Math.max(12, (pitchHeight / pitchSpan) * 0.58);
    for (let index = notes.length - 1; index >= 0; index -= 1) {
      const note = notes[index];
      const left = timeToX(note.start);
      const right = timeToX(note.end);
      const noteY = midiToY(note.midi);
      if (x >= left - 6 && x <= right + 6 && Math.abs(y - noteY) <= hitRadius) return index;
    }
    return null;
  };

  const notesInRectangle = (startX: number, startY: number, endX: number, endY: number) => {
    const left = Math.min(startX, endX);
    const right = Math.max(startX, endX);
    const top = Math.min(startY, endY);
    const bottom = Math.max(startY, endY);
    return notes.flatMap((note, index) => {
      const noteLeft = timeToX(note.start);
      const noteRight = timeToX(note.end);
      const y = midiToY(note.midi);
      return noteRight >= left && noteLeft <= right && y >= top - 10 && y <= bottom + 10
        ? [index]
        : [];
    });
  };

  const wordAt = (x: number, y: number) => {
    if (y < wordTop) return null;
    const time = xToTime(x);
    for (let segment = 0; segment < transcript.segments.length; segment += 1) {
      const words = transcript.segments[segment].words;
      for (let word = 0; word < words.length; word += 1) {
        const lane = wordLayout.lanes.get(`${segment}:${word}`) ?? 0;
        const top = wordTop + WORD_AREA_PADDING + lane * (WORD_BOX_HEIGHT + WORD_LANE_GAP);
        if (
          y >= top &&
          y <= top + WORD_BOX_HEIGHT &&
          time >= words[word].start &&
          time <= Math.max(words[word].end, words[word].start + 8 / zoom)
        )
          return { segment, word };
      }
    }
    return null;
  };

  const handlePointerDown = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (!event.isPrimary || (event.button !== 0 && event.button !== 1)) return;
    event.preventDefault();
    activePointerIdRef.current = event.pointerId;
    const capture = () => {
      try {
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {
        // Pointer capture can race with WebView focus changes. The lost-capture
        // handler and the next pointer down still leave the editor recoverable.
      }
    };
    const point = pointerPosition(event);
    if (event.button === 1 || event.altKey) {
      dragRef.current = {
        kind: "pan",
        pointerX: point.x,
        pointerY: point.y,
        viewStart: clampedViewStart,
        pitchCenter,
      };
      capture();
      return;
    }
    if (point.y < PITCH_TOP) {
      onSeek(xToTime(point.x));
      dragRef.current = { kind: "seek" };
      capture();
      return;
    }
    const noteIndex = noteAt(point.x, point.y);
    if (noteIndex !== null) {
      if (event.shiftKey) {
        activePointerIdRef.current = null;
        const next = new Set(selectedNotes);
        if (next.has(noteIndex)) next.delete(noteIndex);
        else next.add(noteIndex);
        onSelectNotes(next, next.has(noteIndex) ? noteIndex : (next.values().next().value ?? null));
        onSelectWord(null);
        return;
      }
      const indices = selectedNotes.has(noteIndex) ? [...selectedNotes] : [noteIndex];
      const note = notes[noteIndex];
      const left = timeToX(note.start);
      const right = timeToX(note.end);
      const mode =
        indices.length > 1
          ? "move"
          : Math.abs(point.x - left) <= 7
            ? "start"
            : Math.abs(point.x - right) <= 7
              ? "end"
              : "move";
      dragRef.current = {
        kind: "notes",
        indices,
        primary: noteIndex,
        mode,
        pointerX: point.x,
        pointerY: point.y,
        originals: new Map(indices.map((index) => [index, { ...notes[index] }])),
        started: false,
      };
      onSelectNotes(new Set(indices), noteIndex);
      onSelectWord(null);
      capture();
      return;
    }
    const word = wordAt(point.x, point.y);
    if (word) {
      activePointerIdRef.current = null;
      onSelectWord(word);
      onSelectNotes(new Set(), null);
      onSeek(transcript.segments[word.segment].words[word.word].start, true);
      return;
    }
    if (event.shiftKey && point.y < wordTop) {
      dragRef.current = {
        kind: "marquee",
        startX: point.x,
        startY: point.y,
        currentX: point.x,
        currentY: point.y,
        base: new Set(selectedNotes),
      };
      capture();
      setDragVersion((value) => value + 1);
      return;
    }
    onSelectNotes(new Set(), null);
    onSelectWord(null);
    onSeek(xToTime(point.x));
    activePointerIdRef.current = null;
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (activePointerIdRef.current !== event.pointerId) return;
    const drag = dragRef.current;
    if (!drag) return;
    event.preventDefault();
    const point = pointerPosition(event);
    if (drag.kind === "seek") {
      onSeek(xToTime(point.x));
      return;
    }
    if (drag.kind === "marquee") {
      drag.currentX = point.x;
      drag.currentY = point.y;
      setDragVersion((value) => value + 1);
      return;
    }
    if (drag.kind === "pan") {
      const deltaX = point.x - drag.pointerX;
      const deltaY = point.y - drag.pointerY;
      setViewStart(clampViewStart(drag.viewStart - deltaX / zoom, duration, width, zoom));
      setPitchCenter(
        clampPitchCenter(
          drag.pitchCenter + (deltaY / Math.max(1, pitchHeight)) * pitchSpan,
          pitchSpan,
        ),
      );
      return;
    }

    const rawDelta = (point.x - drag.pointerX) / zoom;
    const primary = drag.originals.get(drag.primary);
    if (!primary) return;
    const targetPrimaryStart = snapTime(primary.start + rawDelta);
    const earliest = Math.min(...[...drag.originals.values()].map((note) => note.start));
    const deltaTime = Math.max(-earliest, targetPrimaryStart - primary.start);
    const midiDelta = yToMidi(point.y) - yToMidi(drag.pointerY);
    const changes: Array<{ index: number; note: PitchNote }> = [];

    drag.indices.forEach((index) => {
      const original = drag.originals.get(index);
      if (!original) return;
      const next = { ...original };
      if (drag.mode === "move") {
        next.start = Math.max(0, original.start + deltaTime);
        next.end = next.start + (original.end - original.start);
        next.midi = Math.max(0, Math.min(127, original.midi + midiDelta));
      } else if (drag.mode === "start") {
        next.start = Math.max(
          0,
          Math.min(original.end - 0.03, snapTime(original.start + rawDelta)),
        );
      } else {
        next.end = Math.max(original.start + 0.03, snapTime(original.end + rawDelta));
      }
      changes.push({ index, note: next });
    });
    onChangeNotes(changes, !drag.started);
    drag.started = true;
  };

  const finishPointer = (event?: React.PointerEvent<HTMLCanvasElement>) => {
    if (
      event &&
      activePointerIdRef.current !== null &&
      event.pointerId !== activePointerIdRef.current
    )
      return;
    const drag = dragRef.current;
    if (!drag && activePointerIdRef.current === null) return;
    if (drag?.kind === "marquee") {
      const selected = new Set(drag.base);
      const inRectangle = notesInRectangle(drag.startX, drag.startY, drag.currentX, drag.currentY);
      inRectangle.forEach((index) => selected.add(index));
      onSelectNotes(selected, inRectangle[0] ?? selected.values().next().value ?? null);
    }
    dragRef.current = null;
    activePointerIdRef.current = null;
    onInteractionEnd();
    setDragVersion((value) => value + 1);
  };
  finishPointerRef.current = () => finishPointer();

  useEffect(() => {
    const finishOutsideCanvas = () => finishPointerRef.current();
    window.addEventListener("pointerup", finishOutsideCanvas);
    window.addEventListener("pointercancel", finishOutsideCanvas);
    window.addEventListener("blur", finishOutsideCanvas);
    return () => {
      window.removeEventListener("pointerup", finishOutsideCanvas);
      window.removeEventListener("pointercancel", finishOutsideCanvas);
      window.removeEventListener("blur", finishOutsideCanvas);
      if (wheelFrameRef.current !== null) cancelAnimationFrame(wheelFrameRef.current);
    };
  }, []);

  return (
    <div
      ref={shellRef}
      className={`relative order-1 min-h-0 min-w-0 overflow-hidden border-b border-border/55 bg-card/46 shadow-lg shadow-black/8 backdrop-blur-xl ${expanded ? "flex-1" : "shrink-0"}`}
      style={{ height: expanded ? undefined : totalHeight }}
    >
      <canvas
        ref={canvasRef}
        className="block touch-none cursor-crosshair outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-foreground/25"
        style={{ height: totalHeight }}
        tabIndex={0}
        role="application"
        aria-label="Chart timeline. Drag notes to change timing and pitch. Shift-click or Shift-drag to select several notes. Double-click empty pitch space to add a note."
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={finishPointer}
        onPointerCancel={finishPointer}
        onLostPointerCapture={finishPointer}
        onDoubleClick={(event) => {
          const point = pointerPosition(event);
          if (point.y >= PITCH_TOP && point.y < wordTop && noteAt(point.x, point.y) === null) {
            onAddNote(snapTime(xToTime(point.x)));
          }
        }}
        onWheel={(event) => {
          event.preventDefault();
          const unit =
            event.deltaMode === WheelEvent.DOM_DELTA_LINE
              ? 18
              : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
                ? width
                : 1;
          manualScrollUntilRef.current = performance.now() + 1400;
          if (event.ctrlKey || event.metaKey) {
            const point = pointerPosition(event);
            const direction = event.deltaY > 0 ? -1 : 1;
            const result = zoomTimeAroundPointer({
              viewStart: clampedViewStart,
              pointerX: point.x,
              currentZoom: zoom,
              nextZoom: clampTimeZoom(zoom + direction * 10),
              duration,
              width,
            });
            setViewStart(result.viewStart);
            onZoomChange(result.zoom);
            return;
          }
          if (event.shiftKey) {
            const delta = (event.deltaY || event.deltaX) * unit;
            panPitch((-delta / 90) * 3);
            return;
          }
          pendingWheelDeltaRef.current += (event.deltaY + event.deltaX) * unit;
          if (wheelFrameRef.current !== null) return;
          wheelFrameRef.current = requestAnimationFrame(() => {
            const delta = pendingWheelDeltaRef.current;
            pendingWheelDeltaRef.current = 0;
            wheelFrameRef.current = null;
            setViewStart((current) =>
              clampViewStart(current + delta / zoom, duration, width, zoom),
            );
          });
        }}
      />
      <div className="absolute right-3 top-9 z-20 flex items-center gap-0.5 rounded-full border border-border/50 bg-background/72 p-1 text-muted-foreground shadow-sm backdrop-blur-xl">
        <span
          className="flex items-center gap-1 px-1.5 text-[9px] uppercase tracking-[0.14em]"
          title="Move the chart viewport"
        >
          <Move className="size-3" /> View
        </span>
        <button
          type="button"
          className="grid size-7 place-items-center rounded-full hover:bg-foreground/[0.06] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-foreground/25"
          aria-label="Move chart left"
          title="Move chart left"
          onClick={() => panTime(-visibleSeconds * 0.45)}
        >
          <ArrowLeft className="size-3.5" />
        </button>
        <button
          type="button"
          className="grid size-7 place-items-center rounded-full hover:bg-foreground/[0.06] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-foreground/25"
          aria-label="Move chart right"
          title="Move chart right"
          onClick={() => panTime(visibleSeconds * 0.45)}
        >
          <ArrowRight className="size-3.5" />
        </button>
        <button
          type="button"
          className="grid size-7 place-items-center rounded-full hover:bg-foreground/[0.06] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-foreground/25"
          aria-label="Move to higher pitches"
          title="Move to higher pitches"
          onClick={() => panPitch(4)}
        >
          <ArrowUp className="size-3.5" />
        </button>
        <button
          type="button"
          className="grid size-7 place-items-center rounded-full hover:bg-foreground/[0.06] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-foreground/25"
          aria-label="Move to lower pitches"
          title="Move to lower pitches"
          onClick={() => panPitch(-4)}
        >
          <ArrowDown className="size-3.5" />
        </button>
        <span className="mx-0.5 h-4 w-px bg-border/65" />
        <button
          type="button"
          className="grid size-7 place-items-center rounded-full hover:bg-foreground/[0.06] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-foreground/25"
          aria-label="Show a wider pitch range"
          title="Show more pitches"
          onClick={() => zoomPitch(4)}
        >
          <Minus className="size-3.5" />
        </button>
        <span className="min-w-9 text-center text-[9px] tabular-nums">
          {Math.round(pitchSpan)} st
        </span>
        <button
          type="button"
          className="grid size-7 place-items-center rounded-full hover:bg-foreground/[0.06] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-foreground/25"
          aria-label="Zoom into the pitch range"
          title="Show fewer pitches"
          onClick={() => zoomPitch(-4)}
        >
          <Plus className="size-3.5" />
        </button>
      </div>
      <div
        ref={playheadLineRef}
        className="pointer-events-none absolute top-0 z-10 w-0.5 bg-[#ff506e] will-change-transform"
        style={{ height: totalHeight }}
        aria-hidden="true"
      >
        <span className="absolute -left-1 top-0 size-0 border-x-[5px] border-t-[7px] border-x-transparent border-t-[#ff506e]" />
      </div>
      <div className="pointer-events-none absolute bottom-2 right-3 rounded-full bg-background/75 px-2 py-1 text-[10px] text-muted-foreground backdrop-blur">
        Scroll: time · Shift-scroll: pitch · Alt/middle-drag: move · Ctrl-scroll: zoom
      </div>
    </div>
  );
};
