import {
  getChartReadiness,
  getEditorAudioStatus,
  loadChart,
  loadEditorAudio,
  pauseEditorAudio,
  playEditorAudio,
  saveChart,
  seekEditorAudio,
  stopEditorAudio,
} from "@/bridge/chart";
import { reanalyzePitch } from "@/bridge/analysis";
import { convertFileSrc } from "@/bridge/media";
import { loadAnalysisQueue } from "@/bridge/songs";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { AlbumArt } from "@/components/menu/song-list/shared/album-art";
import type { ChartDocument, PitchNotesDocument } from "@/types/Chart";
import type { PitchNote, PitchNoteKind } from "@/types/PitchGuide";
import type { Song } from "@/types/Song";
import type { Transcript } from "@/types/Transcript";
import {
  ArrowLeft,
  Check,
  ChevronDown,
  ClipboardPaste,
  Combine,
  Copy,
  CopyPlus,
  Grid3X3,
  ListChecks,
  LoaderCircle,
  MoveHorizontal,
  Pause,
  PanelRight,
  PanelBottomClose,
  PanelBottomOpen,
  Play,
  Redo2,
  RefreshCw,
  Save,
  Scissors,
  Split,
  Trash2,
  TriangleAlert,
  Undo2,
  WandSparkles,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { toast } from "sonner";
import { ChartTimeline } from "./chart-timeline";
import {
  NOTE_KIND_OPTIONS,
  analyzeChart,
  copyNotes,
  mergeNotes,
  mergePhraseWithNext,
  mergeWordWithNext,
  noteKind,
  pasteNotes,
  quantizeNotes,
  repairChart,
  setNoteKind,
  shiftNotes,
  shiftTranscript,
  splitNotes,
  splitPhraseAfterWord,
  splitWord,
  type WordSelection,
} from "./chart-tools";
import { useAudioWaveform } from "./use-audio-waveform";

type Snapshot = { transcript: Transcript; pitchNotes: PitchNotesDocument };
const clone = <T,>(value: T): T => structuredClone(value);
const round = (value: number, digits = 3): number => Number(value.toFixed(digits));
const formatClock = (seconds: number): string => {
  const minutes = Math.floor(Math.max(0, seconds) / 60);
  const remainder = Math.max(0, seconds - minutes * 60);
  return `${minutes}:${remainder.toFixed(1).padStart(4, "0")}`;
};

const rebuildSegment = (transcript: Transcript, segmentIndex: number): void => {
  const segment = transcript.segments[segmentIndex];
  segment.words.sort((left, right) => left.start - right.start);
  if (segment.words.length === 0) return;
  segment.start = segment.words[0].start;
  segment.end = Math.max(...segment.words.map((word) => word.end));
  const compactLanguage = /^(zh|ja|ko)/i.test(transcript.language || "");
  segment.text = compactLanguage
    ? segment.words.map((word) => word.word.trim()).join("")
    : segment.words
        .map((word) => word.word.trim())
        .join(" ")
        .replace(/\s+([,.!?;:])/g, "$1");
};

const NumericField = ({
  label,
  value,
  step,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  step: number;
  min?: number;
  max?: number;
  onChange: (value: number) => void;
}) => (
  <label className="space-y-1 text-[11px] font-medium text-muted-foreground">
    <span>{label}</span>
    <Input
      type="number"
      value={value}
      step={step}
      min={min}
      max={max}
      className="h-8 bg-background/60 text-foreground"
      onChange={(event) => {
        const next = Number(event.target.value);
        if (Number.isFinite(next)) onChange(next);
      }}
    />
  </label>
);

export const ChartEditor = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const song = (location.state as { song?: Song } | null)?.song;
  const seekGenerationRef = useRef(0);
  const seekTimerRef = useRef<number | null>(null);
  const playingRef = useRef(false);
  const historyRef = useRef<Snapshot[]>([]);
  const futureRef = useRef<Snapshot[]>([]);
  const dragHistoryArmedRef = useRef(true);
  const clipboardRef = useRef<PitchNote[]>([]);
  const [document, setDocument] = useState<ChartDocument | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [preparingMessage, setPreparingMessage] = useState("Checking chart assets…");
  const [preparingProgress, setPreparingProgress] = useState<number | null>(null);
  const [retryNonce, setRetryNonce] = useState(0);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [savedPulse, setSavedPulse] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [playhead, setPlayhead] = useState(0);
  const [duration, setDuration] = useState(song?.duration_secs ?? 0);
  const [zoom, setZoom] = useState(80);
  const [selectedNote, setSelectedNote] = useState<number | null>(null);
  const [selectedNotes, setSelectedNotes] = useState<Set<number>>(new Set());
  const [clipboardCount, setClipboardCount] = useState(0);
  const [selectedWord, setSelectedWord] = useState<WordSelection | null>(null);
  const [snapSeconds, setSnapSeconds] = useState(0.05);
  const [timingShiftMs, setTimingShiftMs] = useState(50);
  const [showAllIssues, setShowAllIssues] = useState(false);
  const [audioSource, setAudioSource] = useState<"vocals" | "instrumental" | "original">("vocals");
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [lyricsOpen, setLyricsOpen] = useState(true);
  const [audioLoading, setAudioLoading] = useState(false);

  useEffect(() => {
    playingRef.current = playing;
  }, [playing]);

  const backToLibrary = useCallback(() => {
    if (dirty && !window.confirm("Discard unsaved chart changes?")) return;
    navigate("/");
  }, [dirty, navigate]);

  useEffect(() => {
    const warnBeforeClose = (event: BeforeUnloadEvent) => {
      if (!dirty) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", warnBeforeClose);
    return () => window.removeEventListener("beforeunload", warnBeforeClose);
  }, [dirty]);

  useEffect(() => {
    if (!song) {
      setError("Choose a song from the library before opening the chart editor.");
      setLoading(false);
      return;
    }
    let cancelled = false;
    const pause = (milliseconds: number) =>
      new Promise<void>((resolve) => window.setTimeout(resolve, milliseconds));
    const open = async () => {
      setLoading(true);
      setError(null);
      setPreparingProgress(null);
      setPreparingMessage("Checking chart assets…");
      try {
        let readiness = await getChartReadiness(song.file_hash);
        if (cancelled) return;

        if (!readiness.ready && readiness.can_repair_pitch) {
          // Let React's development-only effect cleanup happen before starting
          // the real job, so StrictMode cannot enqueue the same repair twice.
          await pause(120);
          if (cancelled) return;
          setPreparingMessage("Preparing the editable pitch guide…");
          await reanalyzePitch(song.file_hash);

          const deadline = Date.now() + 30 * 60 * 1000;
          while (!cancelled && Date.now() < deadline) {
            const queue = await loadAnalysisQueue().catch(() => null);
            const queueStatus = queue?.entries[song.file_hash];
            if (typeof queueStatus === "object" && "Failed" in queueStatus) {
              throw new Error(`Pitch analysis failed: ${queueStatus.Failed}`);
            }
            if (typeof queueStatus === "object" && "Analyzing" in queueStatus) {
              setPreparingProgress(queueStatus.Analyzing);
              setPreparingMessage(`Analyzing pitch · ${queueStatus.Analyzing}%`);
            } else if (queueStatus === "Queued") {
              setPreparingMessage("Pitch analysis is queued…");
            }

            readiness = await getChartReadiness(song.file_hash);
            if (readiness.ready) break;
            await pause(900);
          }
          if (!readiness.ready && !cancelled) {
            throw new Error("Pitch analysis timed out. You can retry without leaving this screen.");
          }
        }

        if (!readiness.ready) {
          const reason =
            readiness.blocked_reason ??
            `Missing chart assets: ${readiness.missing.join(", ") || "unknown"}`;
          throw new Error(reason);
        }

        setPreparingMessage("Loading chart audio…");
        const chart = await loadChart(song.file_hash);
        if (cancelled) return;
        setDocument(chart);
        setAudioSource(chart.audio.vocals ? "vocals" : "instrumental");
        setDuration(song.duration_secs);
        if (chart.repaired_issues.length > 0) {
          setDirty(true);
          toast.info("Chart timing was repaired", {
            description: `${chart.repaired_issues.join(" · ")}. Save once to keep the fixes.`,
          });
        }
      } catch (reason) {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    void open();
    return () => {
      cancelled = true;
    };
  }, [retryNonce, song]);

  const snapshot = useCallback((): Snapshot | null => {
    if (!document) return null;
    return { transcript: clone(document.transcript), pitchNotes: clone(document.pitch_notes) };
  }, [document]);

  const remember = useCallback(() => {
    const state = snapshot();
    if (!state) return;
    historyRef.current.push(state);
    if (historyRef.current.length > 100) historyRef.current.shift();
    futureRef.current = [];
    setDirty(true);
  }, [snapshot]);

  const restore = useCallback((state: Snapshot) => {
    setDocument((current) =>
      current
        ? { ...current, transcript: clone(state.transcript), pitch_notes: clone(state.pitchNotes) }
        : current,
    );
    setSelectedNote(null);
    setSelectedNotes(new Set());
    setSelectedWord(null);
    setDirty(true);
  }, []);

  const undo = useCallback(() => {
    const previous = historyRef.current.pop();
    const current = snapshot();
    if (!previous || !current) return;
    futureRef.current.push(current);
    restore(previous);
  }, [restore, snapshot]);

  const redo = useCallback(() => {
    const next = futureRef.current.pop();
    const current = snapshot();
    if (!next || !current) return;
    historyRef.current.push(current);
    restore(next);
  }, [restore, snapshot]);

  const updateNotes = useCallback(
    (changes: Array<{ index: number; note: PitchNote }>, begin = true) => {
      if (begin && dragHistoryArmedRef.current) {
        remember();
        dragHistoryArmedRef.current = false;
      }
      setDocument((current) => {
        if (!current) return current;
        const pitchNotes = clone(current.pitch_notes);
        changes.forEach(({ index, note }) => {
          if (!pitchNotes.notes[index]) return;
          pitchNotes.notes[index] = {
            ...note,
            start: round(Math.max(0, note.start)),
            end: round(Math.max(note.start + 0.03, note.end)),
            midi: Math.round(Math.max(0, Math.min(127, note.midi))),
            confidence: Math.max(0, Math.min(1, note.confidence)),
          };
        });
        return { ...current, pitch_notes: pitchNotes };
      });
      setDirty(true);
    },
    [remember],
  );

  const updateNote = useCallback(
    (index: number, note: PitchNote, begin = true) => updateNotes([{ index, note }], begin),
    [updateNotes],
  );

  const selectNotes = useCallback((indices: Set<number>, primary: number | null) => {
    setSelectedNotes(new Set(indices));
    setSelectedNote(primary);
  }, []);

  useEffect(() => {
    const resetDragHistory = () => {
      dragHistoryArmedRef.current = true;
    };
    window.addEventListener("pointerup", resetDragHistory);
    window.addEventListener("pointercancel", resetDragHistory);
    window.addEventListener("blur", resetDragHistory);
    return () => {
      window.removeEventListener("pointerup", resetDragHistory);
      window.removeEventListener("pointercancel", resetDragHistory);
      window.removeEventListener("blur", resetDragHistory);
    };
  }, []);

  const updateWord = (field: "word" | "start" | "end", value: string | number) => {
    if (!document || !selectedWord) return;
    remember();
    setDocument((current) => {
      if (!current) return current;
      const transcript = clone(current.transcript);
      const word = transcript.segments[selectedWord.segment].words[selectedWord.word];
      if (field === "word") word.word = String(value);
      else if (field === "start")
        word.start = round(Math.max(0, Math.min(Number(value), word.end - 0.02)));
      else word.end = round(Math.max(word.start + 0.02, Number(value)));
      rebuildSegment(transcript, selectedWord.segment);
      return { ...current, transcript };
    });
  };

  const commitNotes = (notes: PitchNote[], selection: Set<number>, primary?: number | null) => {
    remember();
    setDocument((current) =>
      current ? { ...current, pitch_notes: { ...current.pitch_notes, notes } } : current,
    );
    selectNotes(
      selection,
      primary === undefined ? (selection.values().next().value ?? null) : primary,
    );
    setSelectedWord(null);
  };

  const addNote = (at = playhead) => {
    if (!document) return;
    const nearby = document.pitch_notes.notes.reduce<PitchNote | null>((best, note) => {
      if (!best) return note;
      return Math.abs(note.start - at) < Math.abs(best.start - at) ? note : best;
    }, null);
    const note: PitchNote = {
      start: round(at),
      end: round(at + 0.5),
      midi: nearby?.midi ?? 60,
      confidence: 1,
      kind: "normal",
    };
    const notes = [...document.pitch_notes.notes, note].sort(
      (left, right) => left.start - right.start,
    );
    const index = notes.indexOf(note);
    commitNotes(notes, new Set([index]), index);
  };

  const deleteSelectedNotes = useCallback(() => {
    if (!document || selectedNotes.size === 0) return;
    remember();
    const notes = document.pitch_notes.notes.filter((_, index) => !selectedNotes.has(index));
    setDocument((current) =>
      current ? { ...current, pitch_notes: { ...current.pitch_notes, notes } } : current,
    );
    selectNotes(new Set(), null);
  }, [document, remember, selectNotes, selectedNotes]);

  const copySelection = useCallback(() => {
    if (!document || selectedNotes.size === 0) return;
    clipboardRef.current = copyNotes(document.pitch_notes.notes, selectedNotes);
    setClipboardCount(clipboardRef.current.length);
    toast.success(
      `${clipboardRef.current.length} note${clipboardRef.current.length === 1 ? "" : "s"} copied`,
    );
  }, [document, selectedNotes]);

  const pasteSelection = useCallback(
    (at = playhead) => {
      if (!document || clipboardRef.current.length === 0) return;
      const result = pasteNotes(document.pitch_notes.notes, clipboardRef.current, at);
      remember();
      setDocument((current) =>
        current
          ? { ...current, pitch_notes: { ...current.pitch_notes, notes: result.notes } }
          : current,
      );
      selectNotes(result.selected, result.selected.values().next().value ?? null);
      setSelectedWord(null);
    },
    [document, playhead, remember, selectNotes],
  );

  const duplicateSelection = () => {
    if (!document || selectedNotes.size === 0) return;
    const copied = copyNotes(document.pitch_notes.notes, selectedNotes);
    clipboardRef.current = copied;
    setClipboardCount(copied.length);
    const selectedEnd = Math.max(
      ...[...selectedNotes].map((index) => document.pitch_notes.notes[index].end),
    );
    pasteSelection(selectedEnd + Math.max(0.02, snapSeconds));
  };

  const splitSelection = () => {
    if (!document || selectedNotes.size === 0) return;
    const result = splitNotes(document.pitch_notes.notes, selectedNotes, playhead);
    commitNotes(result.notes, result.selected);
  };

  const mergeSelection = () => {
    if (!document) return;
    const result = mergeNotes(document.pitch_notes.notes, selectedNotes, selectedNote);
    if (!result) return;
    commitNotes(result.notes, result.selected);
  };

  const changeSelectionKind = (kind: PitchNoteKind) => {
    if (!document || selectedNotes.size === 0) return;
    commitNotes(
      setNoteKind(document.pitch_notes.notes, selectedNotes, kind),
      selectedNotes,
      selectedNote,
    );
  };

  const quantizeSelection = (all = false) => {
    if (!document || snapSeconds <= 0 || (!all && selectedNotes.size === 0)) return;
    const selection = all ? null : selectedNotes;
    const notes = quantizeNotes(document.pitch_notes.notes, selection, snapSeconds);
    commitNotes(notes, all ? new Set() : selectedNotes, all ? null : selectedNote);
    toast.success(all ? "Chart snapped to grid" : "Selection snapped to grid");
  };

  const shiftWholeChart = (direction: -1 | 1) => {
    if (!document) return;
    const requestedSeconds = (timingShiftMs / 1000) * direction;
    const noteStarts = document.pitch_notes.notes.map((note) => note.start);
    const segmentStarts = document.transcript.segments.map((segment) => segment.start);
    const starts = [...noteStarts, ...segmentStarts];
    const earliest = starts.length > 0 ? Math.min(...starts) : 0;
    const seconds = Math.max(requestedSeconds, -earliest);
    remember();
    setDocument((current) =>
      current
        ? {
            ...current,
            transcript: shiftTranscript(current.transcript, seconds),
            pitch_notes: {
              ...current.pitch_notes,
              notes: shiftNotes(current.pitch_notes.notes, null, seconds),
            },
          }
        : current,
    );
    selectNotes(new Set(), null);
    setSelectedWord(null);
  };

  const autoRepair = () => {
    if (!document) return;
    const repaired = repairChart(document.transcript, document.pitch_notes.notes);
    remember();
    setDocument((current) =>
      current
        ? {
            ...current,
            transcript: repaired.transcript,
            pitch_notes: { ...current.pitch_notes, notes: repaired.notes },
          }
        : current,
    );
    selectNotes(new Set(), null);
    setSelectedWord(null);
    toast.success("Safe timing repairs applied");
  };

  const splitSelectedWord = () => {
    if (!document || !selectedWord) return;
    const result = splitWord(document.transcript, selectedWord, playhead);
    if (!result) return;
    remember();
    setDocument((current) => (current ? { ...current, transcript: result.transcript } : current));
    setSelectedWord(result.selected);
  };

  const mergeSelectedWord = () => {
    if (!document || !selectedWord) return;
    const result = mergeWordWithNext(document.transcript, selectedWord);
    if (!result) return;
    remember();
    setDocument((current) => (current ? { ...current, transcript: result.transcript } : current));
    setSelectedWord(result.selected);
  };

  const splitSelectedPhrase = () => {
    if (!document || !selectedWord) return;
    const result = splitPhraseAfterWord(document.transcript, selectedWord);
    if (!result) return;
    remember();
    setDocument((current) => (current ? { ...current, transcript: result.transcript } : current));
    setSelectedWord(result.selected);
  };

  const mergeSelectedPhrase = () => {
    if (!document || !selectedWord) return;
    const result = mergePhraseWithNext(document.transcript, selectedWord.segment);
    if (!result) return;
    remember();
    setDocument((current) => (current ? { ...current, transcript: result.transcript } : current));
    setSelectedWord(result.selected);
  };

  const save = useCallback(async () => {
    if (!document || !song || saving) return;
    setSaving(true);
    try {
      const normalizedNotes = {
        ...document.pitch_notes,
        notes: [...document.pitch_notes.notes].sort((left, right) => left.start - right.start),
      };
      await saveChart(song.file_hash, document.transcript, normalizedNotes);
      setDocument((current) => (current ? { ...current, pitch_notes: normalizedNotes } : current));
      selectNotes(new Set(), null);
      setDirty(false);
      setSavedPulse(true);
      window.setTimeout(() => setSavedPulse(false), 1400);
      toast.success("Chart saved");
    } catch (reason) {
      toast.error(
        `Could not save chart: ${reason instanceof Error ? reason.message : String(reason)}`,
      );
    } finally {
      setSaving(false);
    }
  }, [document, saving, selectNotes, song]);

  const audioPath = document?.audio[audioSource] ?? document?.audio.instrumental;
  const audioUrl = useMemo(() => (audioPath ? convertFileSrc(audioPath) : undefined), [audioPath]);

  const seek = useCallback((time: number, immediate = false) => {
    const next = Math.max(0, time);
    setPlayhead(next);
    seekGenerationRef.current += 1;
    const generation = seekGenerationRef.current;
    if (seekTimerRef.current !== null) window.clearTimeout(seekTimerRef.current);

    const commit = async () => {
      seekTimerRef.current = null;
      try {
        const status = await seekEditorAudio(next);
        if (generation !== seekGenerationRef.current) return;
        // A seek should preserve the user's transport intent. Some pipelines
        // briefly settle in Paused after a flushing seek, so resume explicitly.
        if (playingRef.current && !status.playing) {
          const resumed = await playEditorAudio();
          if (generation === seekGenerationRef.current) setPlaying(resumed.playing);
        }
      } catch (reason) {
        if (generation !== seekGenerationRef.current) return;
        toast.error("Could not seek audio", {
          id: "chart-audio-seek-error",
          description: reason instanceof Error ? reason.message : String(reason),
        });
      }
    };

    if (immediate) void commit();
    else {
      // Pointer scrubbing can produce hundreds of moves per second. Coalesce
      // only those continuous moves; lyric/note jumps use the immediate path.
      seekTimerRef.current = window.setTimeout(() => void commit(), 36);
    }
  }, []);

  useEffect(
    () => () => {
      if (seekTimerRef.current !== null) window.clearTimeout(seekTimerRef.current);
    },
    [],
  );

  const playAudition = useCallback(async () => {
    if (!audioUrl) {
      toast.info(audioLoading ? "Audio preview is still loading" : "Audio preview is unavailable", {
        id: "chart-audio-loading",
      });
      return;
    }
    try {
      const status = await playEditorAudio();
      playingRef.current = status.playing;
      setPlaying(status.playing);
      setAudioLoading(false);
    } catch (reason) {
      playingRef.current = false;
      setPlaying(false);
      toast.error("Audio preview is unavailable", {
        id: "chart-audio-preview-error",
        description:
          reason instanceof Error
            ? reason.message
            : "This system could not decode the selected audio track.",
      });
    }
  }, [audioLoading, audioUrl]);

  const toggleAudition = useCallback(() => {
    if (playingRef.current) {
      playingRef.current = false;
      setPlaying(false);
      void pauseEditorAudio()
        .then((status) => {
          playingRef.current = status.playing;
          setPlaying(status.playing);
          setPlayhead(status.position_secs);
        })
        .catch((reason) => {
          playingRef.current = true;
          setPlaying(true);
          toast.error("Could not pause audio", {
            id: "chart-audio-pause-error",
            description: reason instanceof Error ? reason.message : String(reason),
          });
        });
    } else void playAudition();
  }, [playAudition]);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.matches("input, textarea, select, [contenteditable=true]")) return;
      const command = event.metaKey || event.ctrlKey;
      if (command && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void save();
      } else if (command && event.key.toLowerCase() === "z") {
        event.preventDefault();
        if (event.shiftKey) redo();
        else undo();
      } else if (command && event.key.toLowerCase() === "y") {
        event.preventDefault();
        redo();
      } else if (command && event.key.toLowerCase() === "a" && document) {
        event.preventDefault();
        const all = new Set(document.pitch_notes.notes.map((_, index) => index));
        selectNotes(all, all.values().next().value ?? null);
        setSelectedWord(null);
      } else if (command && event.key.toLowerCase() === "c") {
        event.preventDefault();
        copySelection();
      } else if (command && event.key.toLowerCase() === "x") {
        event.preventDefault();
        copySelection();
        deleteSelectedNotes();
      } else if (command && event.key.toLowerCase() === "v") {
        event.preventDefault();
        pasteSelection();
      } else if (command && event.key.toLowerCase() === "d") {
        event.preventDefault();
        duplicateSelection();
      } else if (event.code === "Space") {
        event.preventDefault();
        if (event.repeat) return;
        toggleAudition();
      } else if (event.key === "Escape" && inspectorOpen) {
        event.preventDefault();
        setInspectorOpen(false);
      } else if ((event.key === "Delete" || event.key === "Backspace") && selectedNotes.size > 0) {
        event.preventDefault();
        deleteSelectedNotes();
      } else if (event.key.toLowerCase() === "s" && selectedNotes.size > 0) {
        event.preventDefault();
        splitSelection();
      } else if (event.key.toLowerCase() === "m" && selectedNotes.size > 1) {
        event.preventDefault();
        mergeSelection();
      } else if (event.key.toLowerCase() === "q" && selectedNotes.size > 0) {
        event.preventDefault();
        quantizeSelection();
      } else if (event.key === "Tab" && document && document.pitch_notes.notes.length > 0) {
        event.preventDefault();
        const direction = event.shiftKey ? -1 : 1;
        const current = selectedNote ?? (direction > 0 ? -1 : 0);
        const next =
          (current + direction + document.pitch_notes.notes.length) %
          document.pitch_notes.notes.length;
        selectNotes(new Set([next]), next);
        setSelectedWord(null);
        seek(document.pitch_notes.notes[next].start);
      } else if (
        selectedNote !== null &&
        document &&
        ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)
      ) {
        event.preventDefault();
        const timeDelta = snapSeconds > 0 ? snapSeconds : 0.01;
        if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
          const direction = event.key === "ArrowLeft" ? -1 : 1;
          if (event.shiftKey) {
            const changes = [...selectedNotes].flatMap((index) => {
              const note = document.pitch_notes.notes[index];
              return note
                ? [
                    {
                      index,
                      note: {
                        ...note,
                        end: Math.max(note.start + 0.03, note.end + direction * timeDelta),
                      },
                    },
                  ]
                : [];
            });
            remember();
            updateNotes(changes, false);
          } else {
            commitNotes(
              shiftNotes(document.pitch_notes.notes, selectedNotes, direction * timeDelta),
              selectedNotes,
              selectedNote,
            );
          }
        } else {
          const semitones = (event.key === "ArrowUp" ? 1 : -1) * (event.shiftKey ? 12 : 1);
          commitNotes(
            shiftNotes(document.pitch_notes.notes, selectedNotes, 0, semitones),
            selectedNotes,
            selectedNote,
          );
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [
    copySelection,
    deleteSelectedNotes,
    document,
    pasteSelection,
    redo,
    save,
    seek,
    selectNotes,
    selectedNote,
    selectedNotes,
    snapSeconds,
    toggleAudition,
    undo,
    updateNotes,
    inspectorOpen,
  ]);

  useEffect(() => {
    if (!song || !audioUrl) return;
    let cancelled = false;
    setPlaying(false);
    playingRef.current = false;
    setAudioLoading(true);
    void stopEditorAudio()
      .then(() => {
        if (cancelled) return null;
        return loadEditorAudio(song.file_hash, audioSource);
      })
      .then((status) => {
        if (cancelled || !status) return;
        setAudioLoading(false);
        setPlayhead(status.position_secs);
        if (status.duration_secs > 0) setDuration(status.duration_secs);
      })
      .catch((reason) => {
        if (cancelled) return;
        setAudioLoading(false);
        toast.error("Audio preview could not be loaded", {
          id: "chart-audio-decode-error",
          description: reason instanceof Error ? reason.message : String(reason),
        });
      });
    return () => {
      cancelled = true;
    };
  }, [audioSource, audioUrl, song]);

  useEffect(
    () => () => {
      void stopEditorAudio();
    },
    [],
  );

  useEffect(() => {
    if (!playing) return;
    let cancelled = false;
    const refresh = async () => {
      try {
        const status = await getEditorAudioStatus();
        if (cancelled) return;
        setPlayhead(status.position_secs);
        if (status.duration_secs > 0) setDuration(status.duration_secs);
        if (!status.playing || status.ended || status.error) {
          playingRef.current = false;
          setPlaying(false);
        }
        if (status.error) {
          toast.error("Audio playback stopped", {
            id: "chart-audio-runtime-error",
            description: status.error,
          });
        }
      } catch (reason) {
        if (cancelled) return;
        playingRef.current = false;
        setPlaying(false);
        toast.error("Audio status could not be read", {
          id: "chart-audio-status-error",
          description: reason instanceof Error ? reason.message : String(reason),
        });
      }
    };
    void refresh();
    // Native status remains the source of truth, while the playhead is
    // interpolated with requestAnimationFrame between these light syncs.
    const interval = window.setInterval(() => void refresh(), 250);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [playing]);

  const { waveform, loading: waveformLoading } = useAudioWaveform(audioUrl, playing);
  const deferredDocument = useDeferredValue(document);
  const issues = useMemo(
    () =>
      deferredDocument
        ? analyzeChart(deferredDocument.transcript, deferredDocument.pitch_notes.notes)
        : [],
    [deferredDocument],
  );
  const errorIssueCount = issues.filter((issue) => issue.severity === "error").length;
  const warningIssueCount = issues.filter((issue) => issue.severity === "warning").length;
  const missingNoteWordCount = issues.filter((issue) =>
    issue.id.startsWith("unpitched-word-"),
  ).length;
  const selectedNoteValue =
    selectedNote === null ? null : (document?.pitch_notes.notes[selectedNote] ?? null);
  const selectedWordValue = selectedWord
    ? document?.transcript.segments[selectedWord.segment]?.words[selectedWord.word]
    : null;

  if (loading) {
    return (
      <main className="grid h-full place-items-center bg-editor-ambient" aria-busy="true">
        <div className="glass-panel w-full max-w-sm rounded-md px-5 py-4 text-sm text-muted-foreground shadow-xl shadow-black/10">
          <div className="flex items-center gap-3">
            <LoaderCircle className="size-5 animate-spin text-primary" />
            <span>{preparingMessage}</span>
          </div>
          {preparingProgress !== null ? (
            <div
              className="mt-3 h-1.5 overflow-hidden rounded-full bg-muted"
              aria-label={`Analysis ${preparingProgress}%`}
            >
              <div
                className="h-full rounded-full bg-primary transition-[width]"
                style={{ width: `${preparingProgress}%` }}
              />
            </div>
          ) : null}
        </div>
      </main>
    );
  }

  if (!document || error || !song) {
    const needsPitchAnalysis = error?.toLowerCase().includes("pitch");
    return (
      <main className="grid h-full place-items-center bg-editor-ambient p-6">
        <section className="glass-panel max-w-lg rounded-md p-7 text-center shadow-xl shadow-black/10">
          <h1 className="text-xl font-semibold">Chart needs attention</h1>
          <p className="mt-2 text-sm text-muted-foreground">{error}</p>
          {needsPitchAnalysis && (
            <p className="mt-3 rounded-md bg-primary/10 p-3 text-xs leading-relaxed text-primary">
              Run <strong>Frequency analysis</strong> from this song's Actions, then reopen the
              editor.
            </p>
          )}
          <div className="mt-5 flex justify-center gap-2">
            <Button variant="outline" onClick={backToLibrary}>
              <ArrowLeft /> Back to library
            </Button>
            <Button onClick={() => setRetryNonce((value) => value + 1)}>
              <RefreshCw /> Retry
            </Button>
          </div>
        </section>
      </main>
    );
  }

  return (
    <main className="roon-library flex h-full min-h-0 flex-col overflow-hidden pt-[var(--titlebar-offset)]">
      <header className="roon-commandbar flex min-h-14 flex-wrap items-center gap-2 border-b border-border/55 px-3 py-2">
        <Button variant="ghost" size="icon-lg" aria-label="Back to library" onClick={backToLibrary}>
          <ArrowLeft />
        </Button>
        <AlbumArt song={song} className="size-9 rounded-sm shadow-md" lazy={false} />
        <div className="mr-auto min-w-0 pl-1">
          <div className="flex items-center gap-2">
            <h1 className="truncate text-sm font-semibold">{song.title}</h1>
            {dirty ? (
              <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] font-medium text-amber-600 dark:text-amber-300">
                Unsaved
              </span>
            ) : savedPulse ? (
              <span className="flex items-center gap-1 text-[10px] font-medium text-emerald-600 dark:text-emerald-300">
                <Check className="size-3" /> Saved
              </span>
            ) : null}
          </div>
          <p className="truncate text-[11px] text-muted-foreground">
            {song.artist || "Unknown artist"} · Chart editor
          </p>
        </div>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Undo"
          disabled={historyRef.current.length === 0}
          onClick={undo}
        >
          <Undo2 />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Redo"
          disabled={futureRef.current.length === 0}
          onClick={redo}
        >
          <Redo2 />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label={inspectorOpen ? "Close inspector" : "Open inspector"}
          aria-pressed={inspectorOpen}
          className={
            inspectorOpen ? "bg-foreground/[0.06] text-foreground" : "text-muted-foreground"
          }
          onClick={() => setInspectorOpen((value) => !value)}
        >
          <PanelRight />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label={lyricsOpen ? "Focus timeline and hide lyrics" : "Show lyrics and timing"}
          aria-pressed={!lyricsOpen}
          className={!lyricsOpen ? "bg-foreground/[0.06] text-foreground" : "text-muted-foreground"}
          onClick={() => setLyricsOpen((value) => !value)}
        >
          {lyricsOpen ? <PanelBottomClose /> : <PanelBottomOpen />}
        </Button>
        <Button disabled={!dirty || saving} onClick={() => void save()}>
          {saving ? <LoaderCircle className="animate-spin" /> : <Save />} Save chart
        </Button>
      </header>

      <section
        className={`relative grid min-h-0 flex-1 grid-cols-1 ${inspectorOpen ? "xl:grid-cols-[minmax(0,1fr)_18rem]" : ""}`}
      >
        <div className="flex min-h-0 min-w-0 flex-col">
          <div className="roon-authoring-dock order-3 flex flex-wrap items-center gap-2 border-t border-border/55 p-2.5">
            <Button
              size="icon-lg"
              aria-label={playing ? "Pause audition" : "Play audition"}
              onClick={toggleAudition}
            >
              {playing ? <Pause /> : <Play />}
            </Button>
            <button
              type="button"
              className="min-w-20 rounded-lg px-2 py-1 text-left font-mono text-xs font-semibold hover:bg-muted focus-visible:ring-1 focus-visible:ring-foreground/20"
              onClick={() => seek(0)}
              aria-label="Return to start"
            >
              {formatClock(playhead)}{" "}
              <span className="font-sans font-normal text-muted-foreground">
                / {formatClock(duration)}
              </span>
            </button>
            <label className="relative ml-1">
              <span className="sr-only">Audition audio source</span>
              <select
                className="h-8 appearance-none rounded-lg border border-border bg-background/60 pl-3 pr-8 text-xs outline-none focus-visible:ring-1 focus-visible:ring-foreground/20"
                value={audioSource}
                onChange={(event) => {
                  setPlaying(false);
                  setAudioSource(event.target.value as typeof audioSource);
                }}
              >
                {document.audio.vocals && <option value="vocals">Vocals</option>}
                <option value="instrumental">Instrumental</option>
                <option value="original">Original</option>
              </select>
              <ChevronDown className="pointer-events-none absolute right-2 top-2 size-4 text-muted-foreground" />
            </label>
            <div className="mx-1 h-5 w-px bg-border" />
            <Button variant="outline" onClick={() => addNote()}>
              <CopyPlus /> Add note
            </Button>
            <Button variant="ghost" disabled={selectedNotes.size === 0} onClick={splitSelection}>
              <Scissors /> Split
            </Button>
            <Button variant="ghost" disabled={selectedNotes.size < 2} onClick={mergeSelection}>
              <Combine /> Merge
            </Button>
            <Button variant="ghost" disabled={selectedNotes.size === 0} onClick={copySelection}>
              <Copy /> Copy
            </Button>
            <Button
              variant="ghost"
              disabled={clipboardCount === 0}
              onClick={() => pasteSelection()}
            >
              <ClipboardPaste /> Paste
            </Button>
            <Button
              variant="ghost"
              className="text-destructive"
              disabled={selectedNotes.size === 0}
              onClick={deleteSelectedNotes}
            >
              <Trash2 /> Delete
            </Button>
            <label className="relative ml-1 flex items-center gap-1.5 text-[10px] text-muted-foreground">
              <Grid3X3 className="size-3.5" />
              <span className="sr-only">Snap grid</span>
              <select
                className="h-8 appearance-none rounded-lg border border-border bg-background/60 pl-2 pr-7 text-xs text-foreground outline-none focus-visible:ring-1 focus-visible:ring-foreground/20"
                value={snapSeconds}
                onChange={(event) => setSnapSeconds(Number(event.target.value))}
                aria-label="Snap grid"
              >
                <option value={0}>Grid off</option>
                <option value={0.01}>10 ms</option>
                <option value={0.025}>25 ms</option>
                <option value={0.05}>50 ms</option>
                <option value={0.1}>100 ms</option>
                <option value={0.25}>250 ms</option>
              </select>
              <ChevronDown className="pointer-events-none absolute right-2 size-3.5" />
            </label>
            <div className="ml-auto flex items-center gap-1">
              <Button
                variant="ghost"
                size="icon"
                aria-label="Zoom out"
                onClick={() => setZoom((value) => Math.max(30, value - 10))}
              >
                <ZoomOut />
              </Button>
              <span className="w-10 text-center text-[10px] text-muted-foreground">{zoom}px/s</span>
              <Button
                variant="ghost"
                size="icon"
                aria-label="Zoom in"
                onClick={() => setZoom((value) => Math.min(180, value + 10))}
              >
                <ZoomIn />
              </Button>
            </div>
          </div>

          <ChartTimeline
            notes={document.pitch_notes.notes}
            track={document.pitch_track}
            transcript={document.transcript}
            waveform={waveform}
            waveformLoading={waveformLoading}
            playhead={playhead}
            playing={playing}
            selectedNotes={selectedNotes}
            primaryNote={selectedNote}
            selectedWord={selectedWord}
            zoom={zoom}
            duration={duration}
            snapSeconds={snapSeconds}
            expanded={!lyricsOpen}
            onSeek={seek}
            onZoomChange={setZoom}
            onSelectNotes={selectNotes}
            onSelectWord={setSelectedWord}
            onChangeNotes={updateNotes}
            onInteractionEnd={() => {
              dragHistoryArmedRef.current = true;
            }}
            onAddNote={addNote}
          />

          {lyricsOpen ? (
            <div className="order-2 min-h-0 flex-1 overflow-hidden border-t border-border/50 bg-card/25 backdrop-blur-xl">
              <div className="flex items-center justify-between border-b border-border/60 px-4 py-2.5">
                <div>
                  <h2 className="text-xs font-semibold">Lyrics & timing</h2>
                  <p className="text-[10px] text-muted-foreground">
                    Choose a word to correct its text or boundaries.
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  {missingNoteWordCount > 0 ? (
                    <span className="inline-flex items-center gap-1 rounded-full bg-amber-500/10 px-2 py-1 text-[10px] text-amber-600 dark:text-amber-300">
                      <TriangleAlert className="size-3" /> {missingNoteWordCount} without notes
                    </span>
                  ) : null}
                  <span className="rounded-full bg-muted px-2 py-1 text-[10px] text-muted-foreground">
                    {document.transcript.segments.reduce(
                      (sum, segment) => sum + segment.words.length,
                      0,
                    )}{" "}
                    words
                  </span>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="text-muted-foreground"
                    aria-label="Hide lyrics and maximize timeline"
                    onClick={() => setLyricsOpen(false)}
                  >
                    <X />
                  </Button>
                </div>
              </div>
              <div className="scrollbar-thin h-full overflow-y-auto p-3 pb-14">
                {document.transcript.segments.map((segment, segmentIndex) => (
                  <div
                    key={`${segmentIndex}-${segment.start}`}
                    className="mb-2 grid min-w-0 grid-cols-[3rem_minmax(0,1fr)] gap-2 rounded-md border border-transparent p-1.5 hover:border-border/70 hover:bg-background/25"
                  >
                    <span className="pt-1.5 text-right font-mono text-[9px] text-muted-foreground">
                      {formatClock(segment.start)}
                    </span>
                    <div className="flex min-w-0 flex-wrap items-start gap-1.5">
                      {segment.words.map((word, wordIndex) => {
                        const active =
                          selectedWord?.segment === segmentIndex && selectedWord.word === wordIndex;
                        const missingNote = issues.some(
                          (issue) => issue.id === `unpitched-word-${segmentIndex}-${wordIndex}`,
                        );
                        return (
                          <button
                            key={`${wordIndex}-${word.start}`}
                            type="button"
                            className={`relative max-w-full min-w-0 break-words rounded-lg border px-2 py-1 text-left text-xs leading-relaxed whitespace-normal transition focus-visible:ring-1 focus-visible:ring-foreground/20 ${active ? "border-primary/65 bg-primary text-primary-foreground shadow-md shadow-primary/15" : missingNote ? "border-amber-500/50 bg-amber-500/[0.055] hover:bg-amber-500/10" : "border-border/60 bg-background/45 hover:border-foreground/22 hover:bg-accent"}`}
                            title={
                              missingNote ? "This lyric has no overlapping pitch note" : undefined
                            }
                            onClick={() => {
                              setSelectedWord({ segment: segmentIndex, word: wordIndex });
                              selectNotes(new Set(), null);
                              seek(word.start, true);
                            }}
                          >
                            {word.word || "…"}
                            {missingNote ? (
                              <TriangleAlert className="ml-1 inline size-2.5 text-amber-500" />
                            ) : null}
                          </button>
                        );
                      })}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ) : null}
        </div>

        {inspectorOpen ? (
          <aside
            className="roon-inspector absolute inset-y-0 right-0 z-30 w-[min(90vw,18rem)] min-h-0 overflow-y-auto border-l border-border/45 p-4 backdrop-blur-2xl xl:static xl:w-auto"
            aria-label="Selection inspector"
          >
            <div className="mb-4 flex items-start gap-3">
              <div className="min-w-0 flex-1">
                <p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-primary">
                  Inspector
                </p>
                <h2 className="mt-1 text-base font-semibold">
                  {selectedNotes.size > 1
                    ? `${selectedNotes.size} pitch notes`
                    : selectedNoteValue
                      ? "Pitch note"
                      : selectedWordValue
                        ? "Lyric word"
                        : "Nothing selected"}
                </h2>
                <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
                  {selectedNotes.size > 1
                    ? "Move, transpose, classify, quantize, split, or merge the selected group."
                    : selectedNoteValue
                      ? "Fine-tune the selected bar. Arrow keys adjust pitch or timing."
                      : selectedWordValue
                        ? "Correct recognition and word-level timing here."
                        : "Select a note bar or lyric word. Shift-drag selects a group."}
                </p>
              </div>
              <Button
                variant="ghost"
                size="icon-sm"
                className="-mr-1 text-muted-foreground"
                aria-label="Close inspector"
                onClick={() => setInspectorOpen(false)}
              >
                <X />
              </Button>
            </div>

            {selectedNotes.size > 1 && (
              <div className="space-y-3">
                <label className="block space-y-1 text-[11px] font-medium text-muted-foreground">
                  <span>UltraStar note type</span>
                  <select
                    className="h-9 w-full rounded-lg border border-border bg-background/60 px-2 text-xs text-foreground outline-none focus-visible:ring-1 focus-visible:ring-foreground/20"
                    value=""
                    onChange={(event) => {
                      if (event.target.value)
                        changeSelectionKind(event.target.value as PitchNoteKind);
                    }}
                  >
                    <option value="">Set type…</option>
                    {NOTE_KIND_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </label>
                <div className="grid grid-cols-2 gap-2">
                  <Button variant="outline" onClick={splitSelection}>
                    <Scissors /> Split
                  </Button>
                  <Button variant="outline" onClick={mergeSelection}>
                    <Combine /> Merge
                  </Button>
                  <Button variant="outline" onClick={() => quantizeSelection()}>
                    <Grid3X3 /> Quantize
                  </Button>
                  <Button variant="outline" onClick={duplicateSelection}>
                    <CopyPlus /> Duplicate
                  </Button>
                </div>
                <div className="rounded-md bg-muted/55 p-3 text-[10px] leading-relaxed text-muted-foreground">
                  Drag any selected bar to move the group. Arrow keys move it; Shift + ↑/↓
                  transposes by an octave, and Shift + ←/→ resizes every ending.
                </div>
              </div>
            )}

            {selectedNotes.size <= 1 && selectedNoteValue && selectedNote !== null && (
              <div className="space-y-3">
                <label className="block space-y-1 text-[11px] font-medium text-muted-foreground">
                  <span>UltraStar note type</span>
                  <select
                    className="h-9 w-full rounded-lg border border-border bg-background/60 px-2 text-xs text-foreground outline-none focus-visible:ring-1 focus-visible:ring-foreground/20"
                    value={noteKind(selectedNoteValue)}
                    onChange={(event) => changeSelectionKind(event.target.value as PitchNoteKind)}
                  >
                    {NOTE_KIND_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </label>
                <div className="grid grid-cols-2 gap-2">
                  <NumericField
                    label="Start · seconds"
                    value={selectedNoteValue.start}
                    min={0}
                    step={0.01}
                    onChange={(value) => {
                      remember();
                      updateNote(selectedNote, { ...selectedNoteValue, start: value }, false);
                    }}
                  />
                  <NumericField
                    label="End · seconds"
                    value={selectedNoteValue.end}
                    min={0}
                    step={0.01}
                    onChange={(value) => {
                      remember();
                      updateNote(selectedNote, { ...selectedNoteValue, end: value }, false);
                    }}
                  />
                  <NumericField
                    label="MIDI pitch"
                    value={selectedNoteValue.midi}
                    min={0}
                    max={127}
                    step={1}
                    onChange={(value) => {
                      remember();
                      updateNote(selectedNote, { ...selectedNoteValue, midi: value }, false);
                    }}
                  />
                  <NumericField
                    label="Confidence"
                    value={round(selectedNoteValue.confidence, 2)}
                    min={0}
                    max={1}
                    step={0.01}
                    onChange={(value) => {
                      remember();
                      updateNote(selectedNote, { ...selectedNoteValue, confidence: value }, false);
                    }}
                  />
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <Button variant="outline" onClick={splitSelection}>
                    <Scissors /> Split
                  </Button>
                  <Button variant="outline" onClick={duplicateSelection}>
                    <CopyPlus /> Duplicate
                  </Button>
                </div>
                <div className="rounded-md bg-muted/55 p-3 text-[10px] leading-relaxed text-muted-foreground">
                  Drag the center to move timing and pitch. Drag either edge to resize. Hold Shift
                  with ↑/↓ for octave steps or with ←/→ to resize.
                </div>
              </div>
            )}

            {selectedWordValue && selectedWord && (
              <div className="space-y-3">
                <label className="block space-y-1 text-[11px] font-medium text-muted-foreground">
                  <span>Text</span>
                  <Input
                    value={selectedWordValue.word}
                    className="h-9 bg-background/60 text-foreground"
                    onChange={(event) => updateWord("word", event.target.value)}
                  />
                </label>
                <div className="grid grid-cols-2 gap-2">
                  <NumericField
                    label="Start · seconds"
                    value={selectedWordValue.start}
                    min={0}
                    step={0.01}
                    onChange={(value) => updateWord("start", value)}
                  />
                  <NumericField
                    label="End · seconds"
                    value={selectedWordValue.end}
                    min={0}
                    step={0.01}
                    onChange={(value) => updateWord("end", value)}
                  />
                </div>
                <Button
                  variant="outline"
                  className="w-full"
                  onClick={() => {
                    seek(selectedWordValue.start);
                    void playAudition();
                  }}
                >
                  <Play /> Audition from word
                </Button>
                <div className="grid grid-cols-2 gap-2">
                  <Button variant="outline" onClick={splitSelectedWord}>
                    <Split /> Split word
                  </Button>
                  <Button
                    variant="outline"
                    disabled={
                      selectedWord.word >=
                      document.transcript.segments[selectedWord.segment].words.length - 1
                    }
                    onClick={mergeSelectedWord}
                  >
                    <Combine /> Merge next
                  </Button>
                  <Button
                    variant="outline"
                    disabled={
                      selectedWord.word >=
                      document.transcript.segments[selectedWord.segment].words.length - 1
                    }
                    onClick={splitSelectedPhrase}
                  >
                    <Scissors /> New phrase
                  </Button>
                  <Button
                    variant="outline"
                    disabled={selectedWord.segment >= document.transcript.segments.length - 1}
                    onClick={mergeSelectedPhrase}
                  >
                    <Combine /> Join phrase
                  </Button>
                </div>
              </div>
            )}

            <div className="mt-6 border-t border-border/60 pt-4">
              <div className="flex items-center justify-between gap-2">
                <div>
                  <h3 className="flex items-center gap-1.5 text-[11px] font-semibold">
                    <ListChecks className="size-3.5 text-primary" /> Chart checks
                  </h3>
                  <p className="mt-0.5 text-[9px] text-muted-foreground">
                    {issues.length === 0
                      ? "No timing or coverage issues found."
                      : `${errorIssueCount} errors · ${warningIssueCount} warnings · ${issues.length} total`}
                  </p>
                </div>
                {issues.some((issue) => issue.autoFixable) && (
                  <Button size="sm" variant="outline" onClick={autoRepair}>
                    <WandSparkles /> Repair
                  </Button>
                )}
              </div>
              {issues.length > 0 && (
                <div className="mt-3 space-y-1.5" role="list" aria-label="Chart issues">
                  {(showAllIssues ? issues : issues.slice(0, 5)).map((issue) => (
                    <button
                      key={issue.id}
                      type="button"
                      role="listitem"
                      className="flex w-full items-start gap-2 rounded-xl border border-border/55 bg-background/35 p-2 text-left transition hover:border-foreground/20 hover:bg-background/60"
                      onClick={() => {
                        seek(issue.time);
                        if (issue.noteIndices?.length) {
                          const selected = new Set(issue.noteIndices);
                          selectNotes(selected, issue.noteIndices[0]);
                          setSelectedWord(null);
                        } else if (issue.wordSelection) {
                          setSelectedWord(issue.wordSelection);
                          selectNotes(new Set(), null);
                        }
                      }}
                    >
                      <TriangleAlert
                        className={`mt-0.5 size-3.5 shrink-0 ${issue.severity === "error" ? "text-destructive" : issue.severity === "warning" ? "text-amber-500" : "text-primary"}`}
                      />
                      <span className="min-w-0">
                        <span className="block text-[10px] font-medium text-foreground">
                          {issue.title}
                        </span>
                        <span className="mt-0.5 block text-[9px] leading-relaxed text-muted-foreground">
                          {issue.description}
                        </span>
                      </span>
                    </button>
                  ))}
                  {issues.length > 5 && (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="w-full"
                      onClick={() => setShowAllIssues((value) => !value)}
                    >
                      {showAllIssues ? "Show fewer" : `Show all ${issues.length}`}
                    </Button>
                  )}
                </div>
              )}
            </div>

            <div className="mt-6 border-t border-border/60 pt-4">
              <h3 className="flex items-center gap-1.5 text-[11px] font-semibold">
                <MoveHorizontal className="size-3.5 text-primary" /> Global timing
              </h3>
              <p className="mt-1 text-[9px] leading-relaxed text-muted-foreground">
                Shift lyrics and pitch together when the whole chart is early or late.
              </p>
              <div className="mt-2 grid grid-cols-[1fr_auto_auto] gap-1.5">
                <Input
                  type="number"
                  min={1}
                  step={10}
                  value={timingShiftMs}
                  aria-label="Global timing step in milliseconds"
                  className="h-8 bg-background/60 text-xs"
                  onChange={(event) => {
                    const value = Number(event.target.value);
                    if (Number.isFinite(value)) setTimingShiftMs(Math.max(1, value));
                  }}
                />
                <Button size="sm" variant="outline" onClick={() => shiftWholeChart(-1)}>
                  − ms
                </Button>
                <Button size="sm" variant="outline" onClick={() => shiftWholeChart(1)}>
                  + ms
                </Button>
              </div>
              {snapSeconds > 0 && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="mt-2 w-full"
                  onClick={() => quantizeSelection(true)}
                >
                  <Grid3X3 /> Quantize whole chart to {Math.round(snapSeconds * 1000)} ms
                </Button>
              )}
            </div>

            <div className="mt-6 border-t border-border/60 pt-4">
              <h3 className="text-[11px] font-semibold">Keyboard</h3>
              <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-2 gap-y-1.5 text-[10px] text-muted-foreground">
                <dt>
                  <kbd>Space</kbd>
                </dt>
                <dd>Play / pause</dd>
                <dt>
                  <kbd>⌘/Ctrl S</kbd>
                </dt>
                <dd>Save chart</dd>
                <dt>
                  <kbd>⌘/Ctrl Z</kbd>
                </dt>
                <dd>Undo · Shift to redo</dd>
                <dt>
                  <kbd>⌘/Ctrl C/V</kbd>
                </dt>
                <dd>Copy / paste notes</dd>
                <dt>
                  <kbd>S / M / Q</kbd>
                </dt>
                <dd>Split / merge / quantize</dd>
                <dt>
                  <kbd>Tab</kbd>
                </dt>
                <dd>Next note</dd>
                <dt>
                  <kbd>← → ↑ ↓</kbd>
                </dt>
                <dd>Move / transpose selection</dd>
              </dl>
            </div>
          </aside>
        ) : null}
      </section>
    </main>
  );
};
