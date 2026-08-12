import { invoke } from "@tauri-apps/api/core";
import type { ChartDocument, PitchNotesDocument } from "@/types/Chart";
import type { Transcript } from "@/types/Transcript";
import type { ChartReadiness } from "@/types/ChartReadiness";

export const getChartReadiness = (fileHash: string): Promise<ChartReadiness> =>
  invoke<ChartReadiness>("chart_readiness", { fileHash });

export const loadChart = (fileHash: string): Promise<ChartDocument> =>
  invoke<ChartDocument>("load_chart", { fileHash });

export const loadChartAudio = async (
  fileHash: string,
  source: "vocals" | "instrumental" | "original",
): Promise<ArrayBuffer> => invoke<ArrayBuffer>("load_chart_audio", { fileHash, source });

export type EditorAudioStatus = {
  loaded: boolean;
  playing: boolean;
  position_secs: number;
  duration_secs: number;
  ended: boolean;
  error: string | null;
};

export const loadEditorAudio = (
  fileHash: string,
  source: "vocals" | "instrumental" | "original",
): Promise<EditorAudioStatus> =>
  invoke<EditorAudioStatus>("editor_audio_load", { fileHash, source });

export const playEditorAudio = (): Promise<EditorAudioStatus> =>
  invoke<EditorAudioStatus>("editor_audio_play");

export const pauseEditorAudio = (): Promise<EditorAudioStatus> =>
  invoke<EditorAudioStatus>("editor_audio_pause");

export const seekEditorAudio = (positionSecs: number): Promise<EditorAudioStatus> =>
  invoke<EditorAudioStatus>("editor_audio_seek", { positionSecs });

export const getEditorAudioStatus = (): Promise<EditorAudioStatus> =>
  invoke<EditorAudioStatus>("editor_audio_status");

export const stopEditorAudio = (): Promise<EditorAudioStatus> =>
  invoke<EditorAudioStatus>("editor_audio_stop");

export const loadTranscript = (fileHash: string): Promise<Transcript> =>
  invoke<Transcript>("load_transcript", { fileHash });

export const saveChart = (
  fileHash: string,
  transcript: Transcript,
  pitchNotes: PitchNotesDocument,
): Promise<void> => invoke("save_chart", { fileHash, transcript, pitchNotes });
