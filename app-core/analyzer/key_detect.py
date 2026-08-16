"""Musical key detection, plus a few extra Essentia descriptors.

Structured output only (`{"tonic", "scale", "confidence"}`) — nothing here
guesses C major on failure or silence. `format_key` is the one place a
`"F#m"`-style string gets built, for code that still wants the old shape.
"""

import math

import numpy as np
import whisperx

try:
    import essentia.standard as es

    HAS_ESSENTIA = True
except Exception:
    # No wheel ships for Windows, so this is a best-effort dependency
    # (see `step_install_packages_for_backend` in vendor.rs) — every caller
    # here falls back to the dependency-free estimator below when it's
    # missing or fails to import.
    HAS_ESSENTIA = False

NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
KRUMHANSL_MAJOR = np.array([6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88])
KRUMHANSL_MINOR = np.array([6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17])

UNKNOWN_KEY = {"tonic": None, "scale": None, "confidence": 0.0}


def format_key(structured: dict):
    """`{"tonic": "F#", "scale": "minor", ...}` -> `"F#m"`; `None` when the
    tonic is unknown. The one place old callers that want a plain key string
    (e.g. the transcript's compat `key` field) should go through."""
    tonic = structured.get("tonic")
    if not tonic:
        return None
    return f"{tonic}m" if structured.get("scale") == "minor" else str(tonic)


def _pc_profile(audio: np.ndarray, sr: int = 16000) -> np.ndarray:
    frame = 4096
    hop = 1024
    if len(audio) < frame:
        return np.zeros(12, dtype=np.float64)

    win = np.hanning(frame)
    freqs = np.fft.rfftfreq(frame, 1.0 / sr)
    min_hz = 40.0
    max_hz = 5000.0
    chroma = np.zeros(12, dtype=np.float64)

    for start in range(0, len(audio) - frame, hop):
        segment = audio[start : start + frame]
        mags = np.abs(np.fft.rfft(segment * win))
        if mags.size == 0:
            continue
        for idx, mag in enumerate(mags):
            hz = freqs[idx]
            if hz < min_hz or hz > max_hz:
                continue
            midi = int(round(69 + 12 * math.log2(hz / 440.0)))
            chroma[midi % 12] += float(mag)

    total = float(chroma.sum())
    if total > 0:
        chroma /= total
    return chroma


def _essentia_key(audio_path: str):
    audio = es.MonoLoader(filename=audio_path)()
    if audio is None or len(audio) == 0:
        return None
    tonic, scale, strength = es.KeyExtractor()(audio)
    if not tonic:
        return None
    return {
        "tonic": str(tonic),
        "scale": "minor" if scale == "minor" else "major",
        "confidence": round(min(max(float(strength), 0.0), 1.0), 3),
    }


def _chroma_key(audio_path: str):
    """Krumhansl-Schmuckler correlation over a hand-rolled chroma profile —
    used when Essentia isn't installed. Never defaults to C major: silence,
    a too-short clip, or an exception all produce the explicit unknown key."""
    audio = whisperx.load_audio(audio_path)
    if audio is None or len(audio) == 0:
        return None

    profile = _pc_profile(audio)
    total = float(profile.sum())
    if total <= 0:
        return None

    best_score = float("-inf")
    best_tonic = None
    best_scale = None
    for i, note in enumerate(NOTE_NAMES):
        score_major = float(np.dot(profile, np.roll(KRUMHANSL_MAJOR, i)))
        if score_major > best_score:
            best_score, best_tonic, best_scale = score_major, note, "major"
        score_minor = float(np.dot(profile, np.roll(KRUMHANSL_MINOR, i)))
        if score_minor > best_score:
            best_score, best_tonic, best_scale = score_minor, note, "minor"

    if best_tonic is None:
        return None

    # Correlation scores aren't naturally bounded; normalize against the
    # best score this exact profile could achieve against either template
    # (any permutation) so `confidence` is at least a bounded, comparable
    # fraction rather than a claim of statistical rigor.
    sorted_profile = np.sort(profile)[::-1]
    max_template = max(
        float(np.dot(sorted_profile, np.sort(KRUMHANSL_MAJOR)[::-1])),
        float(np.dot(sorted_profile, np.sort(KRUMHANSL_MINOR)[::-1])),
    )
    confidence = 0.0 if max_template <= 0 else max(0.0, min(1.0, best_score / max_template))

    return {"tonic": best_tonic, "scale": best_scale, "confidence": round(confidence, 3)}


def detect_key_structured(audio_path: str) -> dict:
    """Returns `{"tonic": str|None, "scale": "major"|"minor"|None,
    "confidence": float}`. Prefers Essentia's `KeyExtractor` when installed,
    falling back to the chroma method above. `tonic`/`scale` are `None` and
    `confidence` is `0.0` when nothing could be determined — this is never
    papered over as C major."""
    if HAS_ESSENTIA:
        try:
            result = _essentia_key(audio_path)
            if result is not None:
                return result
        except Exception as e:
            print(f"[uta-studio:LOG] Key detection failed (essentia): {e}", flush=True)

    try:
        result = _chroma_key(audio_path)
        if result is not None:
            return result
    except Exception as e:
        print(f"[uta-studio:LOG] Key detection failed (chroma): {e}", flush=True)

    return dict(UNKNOWN_KEY)


def analyze_extra_descriptors(audio_path: str):
    """A few additional Essentia descriptors with no dependency-free
    fallback (danceability, dynamic range, loudness) — shown read-only in
    the song settings panel. `None` when Essentia isn't installed or the
    analysis fails; callers must treat that as "no extra descriptors",
    never block on it."""
    if not HAS_ESSENTIA:
        return None
    try:
        audio = es.MonoLoader(filename=audio_path)()
        if audio is None or len(audio) < 44100 * 3:
            return None
        danceability, _dfa = es.Danceability()(audio)
        dynamic_complexity, loudness_db = es.DynamicComplexity()(audio)
        return {
            "danceability": round(float(danceability), 3),
            "dynamic_complexity_db": round(float(dynamic_complexity), 2),
            "loudness_db": round(float(loudness_db), 2),
        }
    except Exception as e:
        print(f"[uta-studio:LOG] Extra descriptors unavailable: {e}", flush=True)
        return None
