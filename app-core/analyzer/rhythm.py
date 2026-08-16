"""Tempo and beat-position analysis.

Essentia is the only real beat tracker available to the analyzer (see
`key_detect.HAS_ESSENTIA` — it has no Windows wheel). Without it there is no
honest way to produce individual beat timestamps, so `analyze_rhythm` falls
back to a dependency-free BPM-only estimate (reused from the pre-Essentia
autocorrelation method) with an empty `beats` list rather than fabricating
beat positions from a constant-tempo assumption.
"""

import math

import numpy as np
import whisperx

try:
    import essentia.standard as es

    HAS_ESSENTIA = True
except Exception:
    HAS_ESSENTIA = False

UNKNOWN_RHYTHM = {"bpm": None, "confidence": 0.0, "beats": []}


def _clean_beats(beats) -> list:
    """Drops NaN/Infinity/negative/duplicate timestamps and enforces a
    strictly increasing sequence — never trusts a backend's output shape."""
    cleaned = []
    last = None
    for raw in beats:
        try:
            value = float(raw)
        except (TypeError, ValueError):
            continue
        if not math.isfinite(value) or value < 0.0:
            continue
        if last is not None and value <= last:
            continue
        cleaned.append(value)
        last = value
    return cleaned


def _essentia_rhythm(audio_path: str):
    audio = es.MonoLoader(filename=audio_path)()
    if audio is None or len(audio) < 44100 * 3:
        return None
    bpm, beats, beats_confidence, _, _ = es.RhythmExtractor2013(method="multifeature")(
        audio
    )
    beats = _clean_beats(beats)
    if not bpm or not math.isfinite(float(bpm)) or bpm <= 0 or not beats:
        return None
    return {
        "bpm": round(float(bpm), 2),
        # `RhythmExtractor2013(method="multifeature")`'s own confidence
        # scale, not normalized to 0-1 by Essentia — clamp defensively so a
        # consumer treating it as a fraction doesn't see something absurd.
        "confidence": round(min(max(float(beats_confidence), 0.0), 5.32) / 5.32, 3),
        "beats": beats,
    }


def _autocorrelation_bpm(audio_path: str):
    """Spectral-flux onset envelope, autocorrelated over 60-200 BPM. No beat
    tracking, so it never returns individual beat positions — only a global
    tempo estimate, for when Essentia is unavailable."""
    audio = whisperx.load_audio(audio_path)
    sr = 16000
    if audio is None or len(audio) < sr * 3:
        return None
    audio = np.asarray(audio, dtype=np.float64)

    frame = 1024
    hop = 256
    if len(audio) < frame:
        return None
    n_frames = 1 + (len(audio) - frame) // hop
    if n_frames < 32:
        return None
    window = np.hanning(frame)
    shape = (n_frames, frame)
    strides = (audio.strides[0] * hop, audio.strides[0])
    frames = np.lib.stride_tricks.as_strided(
        audio, shape=shape, strides=strides, writeable=False
    )
    spectra = np.abs(np.fft.rfft(frames * window, axis=1))

    flux = np.diff(spectra, axis=0)
    onset = np.clip(flux, 0.0, None).sum(axis=1)
    onset = onset - onset.mean()
    onset = np.clip(onset, 0.0, None)
    if not np.any(onset):
        return None

    hop_seconds = hop / sr
    min_bpm, max_bpm = 60.0, 200.0
    min_lag = max(1, int(round(60.0 / max_bpm / hop_seconds)))
    max_lag = min(int(round(60.0 / min_bpm / hop_seconds)), onset.size - 1)
    if max_lag <= min_lag:
        return None

    full_autocorr = np.correlate(onset, onset, mode="full")
    autocorr = full_autocorr[full_autocorr.size // 2 :]
    candidates = autocorr[min_lag : max_lag + 1]
    if candidates.size == 0 or not np.any(candidates):
        return None
    best_lag = min_lag + int(np.argmax(candidates))
    bpm = 60.0 / (best_lag * hop_seconds)

    doubled = bpm * 2.0
    if doubled <= max_bpm:
        doubled_lag = int(round(60.0 / doubled / hop_seconds))
        if (
            min_lag <= doubled_lag <= max_lag
            and autocorr[doubled_lag] >= 0.6 * autocorr[best_lag]
        ):
            bpm = doubled

    # How far the winning lag stands out over the candidate range's average
    # correlation — a bounded, honest peakiness measure, not a claim of
    # statistical confidence.
    mean_score = float(candidates.mean())
    peak_score = float(candidates.max())
    confidence = 0.0 if peak_score <= 0 else max(0.0, min(1.0, 1.0 - mean_score / peak_score))

    return {"bpm": round(float(bpm), 2), "confidence": round(confidence, 3), "beats": []}


def analyze_rhythm(audio_path: str) -> dict:
    """Returns `{"bpm": float|None, "confidence": float, "beats": [float]}`.

    `beats` are absolute seconds into the audio, strictly increasing.
    `bpm` is `None` (never a fabricated default) when nothing could be
    determined — a caller must treat that as "unknown", not "no tempo".
    """
    if HAS_ESSENTIA:
        try:
            result = _essentia_rhythm(audio_path)
            if result is not None:
                return result
        except Exception as e:
            print(f"[uta-studio:LOG] Rhythm analysis failed (essentia): {e}", flush=True)

    try:
        result = _autocorrelation_bpm(audio_path)
        if result is not None:
            return result
    except Exception as e:
        print(f"[uta-studio:LOG] Rhythm analysis failed (autocorrelation): {e}", flush=True)

    return dict(UNKNOWN_RHYTHM)
