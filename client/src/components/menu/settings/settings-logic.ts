export const clampModelSettingNumber = (raw: string | number, fallback: number): number => {
  const normalized = typeof raw === "string" ? raw.trim() : raw;
  if (normalized === "") return Math.max(1, Math.min(16, Math.round(fallback)));
  const parsed = typeof normalized === "number" ? normalized : Number(normalized);
  const safe = Number.isFinite(parsed) ? Math.round(parsed) : fallback;
  return Math.max(1, Math.min(16, safe));
};

export const modelSettingsVisibility = (engine: string) => ({
  // Parakeet still falls back to Whisper for unsupported languages and empty
  // output, so all of the fallback controls must stay configurable and visible.
  whisperModel: true,
  whisperSearch: true,
  batchSize: true,
  // Parakeet words already contain timings, but Whisper fallback still runs
  // the configured aligner. Pitch extraction is shared by every engine.
  wordAlignment: true,
  pitchModel: true,
  activeEngine: engine === "parakeet" ? "parakeet" : "whisper",
});
