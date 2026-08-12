export type PitchFrame = {
  time: number;
  hz: number | null;
  confidence: number;
};

export type PitchTrack = {
  format_version: number;
  model: { id: string; version: string };
  hop_seconds: number;
  frames: PitchFrame[];
};

export type PitchNoteKind = "normal" | "golden" | "freestyle" | "rap" | "golden_rap";

export type PitchNote = {
  start: number;
  end: number;
  midi: number;
  confidence: number;
  /** Optional authoring metadata. Older generated charts omit it and are normal notes. */
  kind?: PitchNoteKind;
};

export type PitchGuide = {
  track: PitchTrack;
  notes: { format_version: number; notes: PitchNote[] };
};
