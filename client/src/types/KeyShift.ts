export interface KeyShift {
  /** Display key, e.g. "Am" or "G" */
  key: string;
  /** Semitones from the original key (negative = lower) */
  keyOffset: number;
  /** Multiplier for PitchShifter.pitch, e.g. 1.0595 for +1 */
  pitchRatio: number;
}
