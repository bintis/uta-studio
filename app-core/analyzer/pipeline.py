"""Shared analysis pipeline used by both server.py and analyze.py."""

import glob
import json
import os
import subprocess
import tempfile

from gpu import hard_free_gpu, log_vram
from whisper_compat import progress
from key_detect import analyze_extra_descriptors, detect_key_structured, format_key
from rhythm import analyze_rhythm
from stems import (
    separate_stems,
    separate_stems_openvino_demucs,
    separate_stems_uvr,
)
from transcribe import transcribe_vocals
from align import align_lyrics
from pitch import analyze_pitch


def ffmpeg_bin():
    return os.environ.get("FFMPEG_PATH", "ffmpeg")


LOSSLESS_EXTENSIONS = {".flac", ".wav", ".wave", ".aif", ".aiff", ".alac"}


def source_is_lossless(path):
    extension = os.path.splitext(path)[1].lower()
    if extension in LOSSLESS_EXTENSIONS:
        return True
    if extension not in {".m4a", ".mp4", ".mov"}:
        return False
    probe = subprocess.run(
        [ffmpeg_bin(), "-hide_banner", "-i", path],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    return "Audio: alac" in probe.stderr


def convert_to_cache_audio(src, destination, *, lossless):
    codec = ["-c:a", "flac", "-compression_level", "8"] if lossless else ["-c:a", "libmp3lame", "-q:a", "2"]
    subprocess.run(
        [ffmpeg_bin(), "-y", "-i", src, *codec, "-v", "error", destination],
        check=True,
    )
    if os.path.isfile(destination):
        os.remove(src)


def normalize_tempo(tempo):
    try:
        t = float(tempo)
    except (TypeError, ValueError):
        return 1.0
    if t <= 0:
        return 1.0
    return round(t + 1e-8, 1)


def _separator_marker_path(output_dir, file_hash):
    return os.path.join(output_dir, f"{file_hash}_separator.json")


def _active_separator_options(separator, options):
    options = options or {}
    if separator == "karaoke":
        segment_size = options.get("segment_size")
        return {
            "segment_size": None if segment_size is None else max(64, min(1024, int(segment_size))),
            "overlap": max(2, min(32, int(options.get("overlap", 8)))),
            "batch_size": max(1, min(8, int(options.get("batch_size", 1)))),
            "normalization_pct": max(1, min(100, int(options.get("normalization_pct", 90)))),
        }
    if separator == "demucs":
        return {
            "shifts": max(1, min(8, int(options.get("demucs_shifts", 1)))),
            "overlap_pct": max(1, min(95, int(options.get("demucs_overlap_pct", 25)))),
        }
    return {}


def _cached_separator_matches(output_dir, file_hash, separator, options):
    try:
        with open(_separator_marker_path(output_dir, file_hash), encoding="utf-8") as marker:
            data = json.load(marker)
            return data.get("separator") == separator and data.get("options", {}) == options
    except (OSError, ValueError, AttributeError):
        return False


def _write_separator_marker(output_dir, file_hash, separator, options):
    path = _separator_marker_path(output_dir, file_hash)
    with open(path, "w", encoding="utf-8") as marker:
        json.dump({"separator": separator, "options": options}, marker)


def _find_legacy_stem_cache(output_dir, file_hash, extension):
    """Finds a stem cache from before separation was decoupled from
    detected key/tempo (`{file_hash}_vocals_{key}_{tempo}.ext`). Only
    `tempo == 1.0` candidates count — that's what the default separation
    always used; a different tempo means a deliberately shifted variant,
    not the base separation this is trying to recognize.

    Returns `(vocals, instrumental)` or `None`. Never deletes, renames, or
    otherwise touches what it finds — an update to the key/tempo detection
    algorithm must not force a costly re-separation for existing libraries,
    but nothing else may still depend on the old filename either, so it's
    left in place.
    """
    prefix = os.path.join(output_dir, f"{file_hash}_vocals_")
    for vocals in sorted(glob.glob(f"{prefix}*_1.0.{extension}")):
        instrumental = os.path.join(
            output_dir, f"{file_hash}_instrumental_{vocals[len(prefix):]}"
        )
        if os.path.isfile(instrumental):
            return vocals, instrumental
    return None


def separate_and_cache(
    audio_path, output_dir, file_hash, separator, device,
    separator_options=None, free_gpu_fn=None,
):
    """Run stem separation or reuse cached stems. Returns the vocals path.

    Stem identity is the source file hash, separator backend, and separator
    options — never a detected key or tempo (see `_cached_separator_matches`
    below), so a BPM/key algorithm update never forces a re-separation.
    """
    progress(4, "Inspecting source codec and cache format...")
    lossless = source_is_lossless(audio_path)
    cache_extension = "flac" if lossless else "mp3"
    final_vocals = os.path.join(output_dir, f"{file_hash}_vocals.{cache_extension}")
    final_instrumental = os.path.join(output_dir, f"{file_hash}_instrumental.{cache_extension}")
    active_options = _active_separator_options(separator, separator_options)
    separator_matches = _cached_separator_matches(output_dir, file_hash, separator, active_options)

    if os.path.isfile(final_vocals) and os.path.isfile(final_instrumental) and separator_matches:
        progress(
            50,
            "Stems already cached, skipping separation",
            requested_device="cache",
            actual_device="cache",
        )
        return final_vocals

    if separator_matches:
        legacy = _find_legacy_stem_cache(output_dir, file_hash, cache_extension)
        if legacy is not None:
            progress(
                50,
                "Stems already cached, skipping separation",
                requested_device="cache",
                actual_device="cache",
            )
            return legacy[0]

    with tempfile.TemporaryDirectory(prefix="uta_studio_") as work_dir:
        if separator == "karaoke":
            # UVR does not currently offer a PyTorch XPU backend, so it runs
            # on CPU for Intel selections.  Keep the user's separator choice
            # authoritative instead of silently substituting Demucs.
            torch_home = os.environ.get("TORCH_HOME", "")
            models_base = os.path.dirname(torch_home) if torch_home else output_dir
            uvr_models_dir = os.path.join(models_base, "audio_separator")
            os.makedirs(uvr_models_dir, exist_ok=True)
            vp, ip = separate_stems_uvr(
                audio_path, work_dir, uvr_models_dir, device, active_options,
            )
        elif separator == "openvino_demucs":
            vp, ip = separate_stems_openvino_demucs(audio_path, work_dir, os.environ.get("OPENVINO_SEPARATOR_MODEL_DIR", output_dir))
        else:
            vp, ip = separate_stems(
                audio_path,
                work_dir,
                device,
                shifts=active_options.get("shifts", 1),
                overlap=active_options.get("overlap_pct", 25) / 100.0,
            )
        progress(51, "Saving stems to cache...")
        convert_to_cache_audio(vp, final_vocals, lossless=lossless)
        convert_to_cache_audio(ip, final_instrumental, lossless=lossless)

    _write_separator_marker(output_dir, file_hash, separator, active_options)

    if free_gpu_fn:
        free_gpu_fn()

    return final_vocals


def transcribe_or_align(
    vocals_path, audio_path, device, *,
    model_name, beam_size=5, batch_size=16,
    engine="whisper",
    lyrics_path=None, language_override=None,
    whisper_model=None, pre_align_cleanup=None,
):
    """Choose between lyrics alignment and full transcription."""
    if lyrics_path and os.path.isfile(lyrics_path):
        print(f"[uta-studio:LOG] Using pre-fetched lyrics: {lyrics_path}", flush=True)
        return align_lyrics(
            lyrics_path, vocals_path, device,
            model_name=model_name,
            language_override=language_override,
            whisper_model=whisper_model,
            pre_align_cleanup=pre_align_cleanup,
        )

    return transcribe_vocals(
        vocals_path, audio_path, device,
        model_name=model_name,
        beam_size=beam_size,
        batch_size=batch_size,
        engine=engine,
        language_override=language_override,
        whisper_model=whisper_model,
        pre_align_cleanup=pre_align_cleanup,
    )


MUSIC_ANALYSIS_VERSION = 1


def _music_analysis_path(output_dir, file_hash):
    return os.path.join(output_dir, f"{file_hash}_music_analysis.json")


def _read_music_analysis_cache(path):
    """Loads a previously written `music_analysis.json`, or `None` if it's
    missing, corrupt, or from an older version — regenerated rather than
    trusted in any of those cases."""
    if not os.path.isfile(path):
        return None
    try:
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except (OSError, ValueError) as e:
        print(f"[uta-studio:LOG] Music analysis cache unreadable, regenerating: {e}", flush=True)
        return None
    if (
        not isinstance(data, dict)
        or data.get("version") != MUSIC_ANALYSIS_VERSION
        or not isinstance(data.get("key"), dict)
        or not isinstance(data.get("rhythm"), dict)
    ):
        return None
    return data


def _write_music_analysis_cache(path, data):
    """Temp-file-plus-atomic-replace so a crash or kill mid-write never
    leaves a half-written (and therefore corrupt-looking, triggering a
    pointless re-analysis) JSON file behind."""
    directory = os.path.dirname(path) or "."
    fd, temp_path = tempfile.mkstemp(prefix="music_analysis_", suffix=".tmp", dir=directory)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
        os.replace(temp_path, path)
    except OSError as e:
        print(f"[uta-studio:LOG] Failed to write music analysis cache: {e}", flush=True)
        try:
            os.remove(temp_path)
        except OSError:
            pass


def analyze_music(audio_path, output_dir, file_hash):
    """Key + rhythm (+ a few extra descriptors when Essentia is installed),
    cached to `{file_hash}_music_analysis.json`. Reuses a valid existing
    cache instead of re-running analysis: realigning lyrics or re-running
    transcription must not repeat key/BPM detection, and re-running this
    alone must not touch stems or the transcript.

    Always returns a dict shaped like the cache file (`version`/`key`/
    `rhythm`, `descriptors` only when available) — key/rhythm fields are
    the explicit "unknown" shape on failure, never a fabricated default.
    """
    path = _music_analysis_path(output_dir, file_hash)
    cached = _read_music_analysis_cache(path)
    if cached is not None:
        progress(3, "Music analysis already cached, skipping...")
        return cached

    progress(3, "Analyzing musical key...", implementation="Essentia/NumPy FFT", model="KeyExtractor / Krumhansl chroma profiles")
    key = detect_key_structured(audio_path)

    progress(4, "Analyzing tempo and beat positions...", implementation="Essentia/NumPy FFT", model="RhythmExtractor2013 / onset autocorrelation")
    rhythm = analyze_rhythm(audio_path)

    if key.get("tonic") is None and rhythm.get("bpm") is None:
        progress(4, "Music analysis unavailable; continuing without beat grid...")

    result = {"version": MUSIC_ANALYSIS_VERSION, "key": key, "rhythm": rhythm}

    try:
        descriptors = analyze_extra_descriptors(audio_path)
        if descriptors:
            result["descriptors"] = descriptors
    except Exception as e:
        # Never let an optional descriptor pass take key/rhythm down with it.
        print(f"[uta-studio:LOG] Extra descriptors unavailable: {e}", flush=True)

    _write_music_analysis_cache(path, result)
    return result


def run_pipeline(
    audio_path, output_dir, file_hash, device, *,
    model_name="large-v3", beam_size=5, batch_size=16,
    separator="karaoke", separator_options=None, engine="whisper",
    lyrics_path=None, language_override=None,
    whisper_model=None, pre_align_cleanup=None, free_gpu_fn=None,
    skip_transcription=False,
    skip_separation=False,
):
    """Full analysis pipeline: stem separation -> transcription -> save.

    When ``skip_transcription`` is set, an existing (LRC-provided) transcript is
    kept as-is: only key detection and stem separation run, and the detected key
    is patched into the transcript. When ``skip_separation`` is also set, stem
    separation is skipped too (the song plays over its original mix); only the
    key is detected and stamped onto the provided transcript.
    """
    os.makedirs(output_dir, exist_ok=True)

    transcript_path = os.path.join(output_dir, f"{file_hash}_transcript.json")
    transcript_exists = os.path.isfile(transcript_path)
    pitch_track_path = os.path.join(output_dir, f"{file_hash}_pitch_track.json")
    pitch_notes_path = os.path.join(output_dir, f"{file_hash}_pitch_notes.json")
    pitch_ready = os.path.isfile(pitch_track_path) and os.path.isfile(pitch_notes_path)
    if transcript_exists and not skip_transcription and pitch_ready:
        progress(100, "Already analyzed, skipping")
        return

    progress(1, "Inspecting source audio and existing analysis cache...")
    progress(2, f"Using device: {device}")

    try:
        log_vram("phase:start")
        music = analyze_music(audio_path, output_dir, file_hash)
        detected_key = format_key(music["key"])
        detected_bpm = music["rhythm"]["bpm"]
        tempo = 1.0

        vocals_path = None
        if not skip_separation:
            vocals_path = separate_and_cache(
                audio_path, output_dir, file_hash, separator, device,
                separator_options=separator_options,
                free_gpu_fn=free_gpu_fn,
            )
            log_vram("phase:after_separation")

        # A failed guide must not make an otherwise playable karaoke analysis
        # fail. Existing pitchy-based reference detection remains the runtime
        # fallback for songs without these cache files.
        if vocals_path and pitch_ready:
            progress(
                54,
                "Pitch guide already cached, skipping extraction",
                implementation="RMVPE",
                model="RMVPE singing pitch model",
                requested_device="cache",
                actual_device="cache",
            )
        elif vocals_path:
            try:
                progress(52, "Extracting reference pitch...")
                analyze_pitch(
                    vocals_path,
                    output_dir,
                    file_hash,
                    os.environ.get("PITCH_MODEL_DIR", os.path.join(output_dir, "pitch-model")),
                )
                progress(54, "Building singing guide...")
            except Exception as e:
                print(f"[uta-studio:LOG] Pitch guide unavailable: {e}", flush=True)

        if transcript_exists and not skip_transcription:
            progress(100, "Transcript already cached")
            return

        if skip_transcription:
            # Keep the provided LRC transcript; only stamp key/tempo onto it.
            transcript = {}
            if transcript_exists:
                try:
                    with open(transcript_path, "r", encoding="utf-8") as f:
                        transcript = json.load(f)
                except (OSError, ValueError) as e:
                    print(f"[uta-studio:LOG] Failed to read provided transcript: {e}", flush=True)
            transcript["key"] = detected_key
            transcript["tempo"] = normalize_tempo(tempo)
            transcript["bpm"] = detected_bpm
            progress(95, "Writing transcript...")
            with open(transcript_path, "w", encoding="utf-8") as f:
                json.dump(transcript, f, ensure_ascii=False, indent=2)
            return

        if callable(whisper_model):
            whisper_model = whisper_model()

        transcript = transcribe_or_align(
            vocals_path, audio_path, device,
            model_name=model_name,
            beam_size=beam_size,
            batch_size=batch_size,
            engine=engine,
            lyrics_path=lyrics_path,
            language_override=language_override,
            whisper_model=whisper_model,
            pre_align_cleanup=pre_align_cleanup,
        )
        log_vram("phase:after_transcribe_or_align")

        transcript["key"] = detected_key
        transcript["tempo"] = normalize_tempo(tempo)
        transcript["bpm"] = detected_bpm

        progress(95, "Writing transcript...")
        with open(transcript_path, "w", encoding="utf-8") as f:
            json.dump(transcript, f, ensure_ascii=False, indent=2)
    finally:
        hard_free_gpu("pipeline_end")
