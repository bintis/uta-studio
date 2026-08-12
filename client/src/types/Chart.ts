import type { PitchNote, PitchTrack } from "./PitchGuide";
import type { Transcript } from "./Transcript";

export type PitchNotesDocument = {
  format_version: number;
  notes: PitchNote[];
};

export type ChartAudio = {
  instrumental: string;
  vocals: string | null;
  original: string;
};

export type ChartDocument = {
  file_hash: string;
  transcript: Transcript;
  pitch_track: PitchTrack;
  pitch_notes: PitchNotesDocument;
  audio: ChartAudio;
  repaired_issues: string[];
};
