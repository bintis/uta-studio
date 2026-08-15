#!/usr/bin/env python3
"""Offline reference-pitch analysis for Uta Studio.

RMVPE is deliberately used only while analysing an already separated vocal
stem. Playback keeps the lightweight browser detector for microphone latency.
The outputs are plain JSON so they survive model/runtime changes and remain
portable in exported song packages.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

import numpy as np


MODEL_ID = "rmvpe-onnx"
MODEL_VERSION = "0.2.3"
FORMAT_VERSION = 1
MODEL_FILENAME = "rmvpe.onnx"
VOICED_CONFIDENCE = 0.55
MIN_HZ = 50.0
MAX_HZ = 1_100.0
MIN_NOTE_SECONDS = 0.08
OPENVINO_GPU_DEVICE = os.environ.get("UTA_STUDIO_OPENVINO_GPU_DEVICE", "GPU.0")


def _model_path(models_dir: str | Path) -> Path:
    return Path(models_dir).expanduser().resolve() / MODEL_FILENAME


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _write_manifest(model_path: Path) -> None:
    manifest = {
        "model_id": MODEL_ID,
        "model_version": MODEL_VERSION,
        "filename": model_path.name,
        "sha256": _sha256(model_path),
    }
    model_path.with_name("manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )


def ensure_model(models_dir: str | Path):
    """Return a loaded RMVPE instance, downloading the pinned package model if needed."""
    from rmvpe_onnx import RMVPE

    model_path = _model_path(models_dir)
    model_path.parent.mkdir(parents=True, exist_ok=True)
    # rmvpe-onnx supports a custom path and downloads a missing model there.
    backend = os.environ.get("UTA_STUDIO_COMPUTE_BACKEND", "cpu").lower()
    # rmvpe-onnx exposes CUDA directly and routes Intel Arc through the
    # OpenVINO GPU execution provider.  CPU remains the explicit safe default.
    device = {
        "cuda": "cuda",
        "intel": f"openvino:{OPENVINO_GPU_DEVICE.lower()}",
    }.get(backend, "cpu")
    if device.startswith("openvino:"):
        # The analyzer environment may contain the Intel GPU runtime while
        # still having a CPU-only onnxruntime wheel. rmvpe-onnx raises before
        # inference in that case, so select its reliable CPU provider instead
        # of silently losing the pitch guide for the whole song.
        try:
            import onnxruntime as ort

            providers = ort.get_available_providers()
        except Exception as error:
            providers = []
            print(f"[uta-studio:LOG] Could not inspect ONNX providers: {error}", flush=True)
        if "OpenVINOExecutionProvider" not in providers:
            print(
                "[uta-studio:LOG] OpenVINO pitch provider unavailable; falling back to CPU",
                flush=True,
            )
            from whisper_compat import progress
            progress(
                52,
                "OpenVINO pitch provider unavailable; continuing on CPU...",
                requested_device="xpu",
                actual_device="cpu",
                fallback_from="xpu",
                fallback_reason="ONNX Runtime does not expose the OpenVINO execution provider",
            )
            device = "cpu"
    print(f"[uta-studio:LOG] Loading RMVPE pitch model on {device}", flush=True)
    estimator = RMVPE(model_path=str(model_path), device=device)
    if not model_path.is_file():
        raise RuntimeError("RMVPE did not create its model file")
    _write_manifest(model_path)
    return estimator


def _load_audio(audio_path: str) -> tuple[np.ndarray, int]:
    """Decode all supported stem formats through the app's bundled ffmpeg."""
    ffmpeg = os.environ.get("FFMPEG_PATH", "ffmpeg")
    proc = subprocess.run(
        [ffmpeg, "-v", "error", "-i", audio_path, "-ac", "1", "-ar", "16000", "-f", "f32le", "pipe:1"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        detail = proc.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(f"ffmpeg could not decode vocal stem: {detail}")
    audio = np.frombuffer(proc.stdout, dtype="<f4").copy()
    if audio.size == 0:
        raise RuntimeError("vocal stem contains no samples")
    return audio, 16000


def _clean_frame(time: float, hz: float, confidence: float) -> dict:
    voiced = confidence >= VOICED_CONFIDENCE and MIN_HZ <= hz <= MAX_HZ
    return {
        "time": round(float(time), 4),
        "hz": round(float(hz), 3) if voiced else None,
        "confidence": round(float(confidence), 4),
    }


def _midi(hz: float) -> float:
    return 69.0 + 12.0 * np.log2(hz / 440.0)


def segment_notes(frames: list[dict]) -> list[dict]:
    """Turn voiced F0 frames into stable, display-friendly semitone bars."""
    notes: list[dict] = []
    current: list[dict] = []
    current_midi: int | None = None

    def flush() -> None:
        nonlocal current, current_midi
        if not current or current_midi is None:
            current = []
            current_midi = None
            return
        start = current[0]["time"]
        # The last frame represents its hop, so make the drawn bar reach it.
        end = current[-1]["time"] + 0.01
        if end - start >= MIN_NOTE_SECONDS:
            notes.append(
                {
                    "start": round(start, 3),
                    "end": round(end, 3),
                    "midi": current_midi,
                    "confidence": round(
                        sum(frame["confidence"] for frame in current) / len(current), 4
                    ),
                }
            )
        current = []
        current_midi = None

    # A five-frame median removes short RMVPE fluctuations before quantising.
    voiced_hz = [frame["hz"] for frame in frames]
    for index, frame in enumerate(frames):
        hz = frame["hz"]
        if hz is None:
            flush()
            continue
        nearby = [
            value
            for value in voiced_hz[max(0, index - 2) : index + 3]
            if value is not None
        ]
        midi = int(round(_midi(float(np.median(nearby))))) if nearby else int(round(_midi(hz)))
        if current_midi is not None and midi != current_midi:
            flush()
        current_midi = midi
        current.append(frame)
    flush()

    # Merge adjacent fragments of the same note; consonants often make a
    # singer's otherwise continuous note briefly unvoiced.
    merged: list[dict] = []
    for note in notes:
        if (
            merged
            and note["midi"] == merged[-1]["midi"]
            and note["start"] - merged[-1]["end"] <= 0.06
        ):
            prior = merged[-1]
            prior["end"] = note["end"]
            prior["confidence"] = round((prior["confidence"] + note["confidence"]) / 2, 4)
        else:
            merged.append(note)
    return merged


def _analyze_pitch_in_process(vocals_path: str, output_dir: str, file_hash: str, models_dir: str) -> None:
    """Write the frame-level reference track and segmented guide notes."""
    output = Path(output_dir)
    track_path = output / f"{file_hash}_pitch_track.json"
    notes_path = output / f"{file_hash}_pitch_notes.json"
    if track_path.is_file() and notes_path.is_file():
        print("[uta-studio:LOG] Pitch guide already cached, skipping", flush=True)
        return

    estimator = ensure_model(models_dir)
    audio, sample_rate = _load_audio(vocals_path)
    times, frequencies, confidences, _activation = estimator.predict(audio, sample_rate)
    frames = [
        _clean_frame(time, hz, confidence)
        for time, hz, confidence in zip(times, frequencies, confidences, strict=True)
    ]
    track = {
        "format_version": FORMAT_VERSION,
        "model": {"id": MODEL_ID, "version": MODEL_VERSION},
        "hop_seconds": 0.01,
        "frames": frames,
    }
    notes = {"format_version": FORMAT_VERSION, "notes": segment_notes(frames)}

    track_path.write_text(json.dumps(track, ensure_ascii=False), encoding="utf-8")
    notes_path.write_text(json.dumps(notes, ensure_ascii=False), encoding="utf-8")


def analyze_pitch(vocals_path: str, output_dir: str, file_hash: str, models_dir: str) -> None:
    """Run the OpenVINO RMVPE backend without PyTorch XPU in this process.

    The persistent analyzer imports PyTorch for WhisperX.  Its Intel XPU wheel
    prevents ONNX Runtime's OpenVINO GPU provider from creating a Level Zero
    context, so pitch extraction must run in a clean interpreter.
    """
    track_path = Path(output_dir) / f"{file_hash}_pitch_track.json"
    notes_path = Path(output_dir) / f"{file_hash}_pitch_notes.json"
    if track_path.is_file() and notes_path.is_file():
        print("[uta-studio:LOG] Pitch guide already cached, skipping", flush=True)
        return

    result = subprocess.run(
        [
            sys.executable,
            str(Path(__file__).resolve()),
            "--models-dir",
            models_dir,
            "--audio",
            vocals_path,
            "--output-dir",
            output_dir,
            "--hash",
            file_hash,
            "--in-process",
        ],
        capture_output=True,
        text=True,
    )
    if result.returncode:
        detail = (result.stderr or result.stdout).strip()
        raise RuntimeError(f"RMVPE OpenVINO helper failed: {detail}")
    for line in result.stdout.splitlines():
        if line.startswith("[uta-studio:LOG]"):
            print(line, flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description="Uta Studio RMVPE pitch helper")
    parser.add_argument("--models-dir", required=True, help="Directory containing rmvpe.onnx")
    parser.add_argument("--download-model", action="store_true")
    parser.add_argument("--audio")
    parser.add_argument("--output-dir")
    parser.add_argument("--hash", dest="file_hash")
    parser.add_argument("--in-process", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()

    if args.download_model:
        ensure_model(args.models_dir)
        print("RMVPE model ready", flush=True)
        return

    if not args.audio or not args.output_dir or not args.file_hash:
        parser.error("--audio, --output-dir, and --hash are required when analysing pitch")
    _analyze_pitch_in_process(args.audio, args.output_dir, args.file_hash, args.models_dir)


if __name__ == "__main__":
    main()
