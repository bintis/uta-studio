"""Shared analysis pipeline used by both server.py and analyze.py."""

import json
import os
import subprocess
import tempfile

from gpu import hard_free_gpu, log_vram
from whisper_compat import progress
from key_detect import detect_key
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


def format_tempo(tempo):
    return f"{normalize_tempo(tempo):.1f}"


def sanitize_key(key):
    raw = str(key or "").strip()
    out = []
    for ch in raw:
        if ch.isalnum() or ch in ("#", "b"):
            out.append(ch)
        elif ch in (" ", "-", "_"):
            out.append("_")
    cleaned = "".join(out).strip("_")
    while "__" in cleaned:
        cleaned = cleaned.replace("__", "_")
    return cleaned or "Unknown"


def _separator_marker_path(output_dir, file_hash):
    return os.path.join(output_dir, f"{file_hash}_separator.json")


def _cached_separator_matches(output_dir, file_hash, separator):
    try:
        with open(_separator_marker_path(output_dir, file_hash), encoding="utf-8") as marker:
            return json.load(marker).get("separator") == separator
    except (OSError, ValueError, AttributeError):
        return False


def _write_separator_marker(output_dir, file_hash, separator):
    path = _separator_marker_path(output_dir, file_hash)
    with open(path, "w", encoding="utf-8") as marker:
        json.dump({"separator": separator}, marker)


def separate_and_cache(audio_path, output_dir, file_hash, separator, device, key, tempo, free_gpu_fn=None):
    """Run stem separation or reuse cached stems. Returns the vocals path."""
    key_safe = sanitize_key(key)
    tempo_safe = format_tempo(tempo)
    lossless = source_is_lossless(audio_path)
    cache_extension = "flac" if lossless else "mp3"
    final_vocals = os.path.join(output_dir, f"{file_hash}_vocals_{key_safe}_{tempo_safe}.{cache_extension}")
    final_instrumental = os.path.join(output_dir, f"{file_hash}_instrumental_{key_safe}_{tempo_safe}.{cache_extension}")

    if (
        os.path.isfile(final_vocals)
        and os.path.isfile(final_instrumental)
        and _cached_separator_matches(output_dir, file_hash, separator)
    ):
        progress(50, "Stems already cached, skipping separation")
        return final_vocals

    with tempfile.TemporaryDirectory(prefix="uta_studio_") as work_dir:
        if separator == "karaoke":
            # UVR does not currently offer a PyTorch XPU backend, so it runs
            # on CPU for Intel selections.  Keep the user's separator choice
            # authoritative instead of silently substituting Demucs.
            torch_home = os.environ.get("TORCH_HOME", "")
            models_base = os.path.dirname(torch_home) if torch_home else output_dir
            uvr_models_dir = os.path.join(models_base, "audio_separator")
            os.makedirs(uvr_models_dir, exist_ok=True)
            vp, ip = separate_stems_uvr(audio_path, work_dir, uvr_models_dir)
        elif separator == "openvino_demucs":
            vp, ip = separate_stems_openvino_demucs(audio_path, work_dir, os.environ.get("OPENVINO_SEPARATOR_MODEL_DIR", output_dir))
        else:
            vp, ip = separate_stems(audio_path, work_dir, device)
        progress(51, "Saving stems to cache...")
        convert_to_cache_audio(vp, final_vocals, lossless=lossless)
        convert_to_cache_audio(ip, final_instrumental, lossless=lossless)

    _write_separator_marker(output_dir, file_hash, separator)

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


def run_pipeline(
    audio_path, output_dir, file_hash, device, *,
    model_name="large-v3", beam_size=5, batch_size=16,
    separator="karaoke", engine="whisper",
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

    progress(2, f"Using device: {device}")

    try:
        log_vram("phase:start")
        detected_key = detect_key(audio_path)
        tempo = 1.0

        vocals_path = None
        if not skip_separation:
            vocals_path = separate_and_cache(
                audio_path, output_dir, file_hash, separator, device,
                key=detected_key,
                tempo=tempo,
                free_gpu_fn=free_gpu_fn,
            )
            log_vram("phase:after_separation")

        # A failed guide must not make an otherwise playable karaoke analysis
        # fail. Existing pitchy-based reference detection remains the runtime
        # fallback for songs without these cache files.
        if vocals_path and not pitch_ready:
            try:
                progress(62, "Extracting reference pitch...")
                analyze_pitch(
                    vocals_path,
                    output_dir,
                    file_hash,
                    os.environ.get("PITCH_MODEL_DIR", os.path.join(output_dir, "pitch-model")),
                )
                progress(67, "Building singing guide...")
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

        progress(95, "Writing transcript...")
        with open(transcript_path, "w", encoding="utf-8") as f:
            json.dump(transcript, f, ensure_ascii=False, indent=2)
    finally:
        hard_free_gpu("pipeline_end")
