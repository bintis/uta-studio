"""Official OpenVINO GenAI Whisper backend for Intel Arc GPUs.

This module deliberately has no import-time OpenVINO dependency: CPU and CUDA
installations share the analyzer scripts, while only the Intel setup installs
``openvino-genai`` and downloads this model.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


MODEL_REPOSITORY = "OpenVINO/whisper-large-v3-turbo-fp16-ov"
_REQUIRED_FILES = ("config.json", "openvino_encoder_model.xml")


def default_model_dir() -> Path:
    configured = os.environ.get("OPENVINO_WHISPER_MODEL_DIR")
    if not configured:
        raise RuntimeError("OPENVINO_WHISPER_MODEL_DIR is not configured")
    return Path(configured).expanduser().resolve()


def is_model_ready(model_dir: str | Path) -> bool:
    directory = Path(model_dir)
    return all((directory / filename).is_file() for filename in _REQUIRED_FILES)


def download_model(model_dir: str | Path) -> Path:
    """Download the official pre-converted Whisper IR into the models cache."""
    from huggingface_hub import snapshot_download

    directory = Path(model_dir).expanduser().resolve()
    if is_model_ready(directory):
        print(f"OpenVINO Whisper model already available: {directory}", flush=True)
        return directory

    directory.mkdir(parents=True, exist_ok=True)
    print(
        f"Downloading {MODEL_REPOSITORY} to {directory}. This is a one-time Intel GPU model download...",
        flush=True,
    )
    snapshot_download(repo_id=MODEL_REPOSITORY, local_dir=str(directory))
    if not is_model_ready(directory):
        missing = ", ".join(
            filename for filename in _REQUIRED_FILES if not (directory / filename).is_file()
        )
        raise RuntimeError(f"OpenVINO Whisper download completed but required files are missing: {missing}")
    print("OpenVINO Whisper model download complete", flush=True)
    return directory


def _transcribe_in_process(
    audio, language: str | None, model_dir: str | Path | None = None,
) -> list[dict]:
    """Run Whisper on OpenVINO's GPU device in a process without PyTorch XPU."""
    import openvino as ov
    import openvino_genai
    from openvino_separation import _gpu_compile_config, _intel_gpu_device

    directory = Path(model_dir) if model_dir is not None else default_model_dir()
    if not is_model_ready(directory):
        raise RuntimeError(f"OpenVINO Whisper model is missing from {directory}")

    core = ov.Core()
    gpu_device = _intel_gpu_device(core)
    print(
        f"[uta-studio:LOG] Loading official OpenVINO Whisper on {gpu_device}: {directory}",
        file=sys.stderr,
        flush=True,
    )
    pipeline = openvino_genai.WhisperPipeline(
        str(directory), gpu_device, **_gpu_compile_config(),
    )
    options = {
        "max_new_tokens": 448,
        "task": "transcribe",
        "return_timestamps": True,
    }
    if language:
        options["language"] = f"<|{language}|>"

    # WhisperPipeline expects normalized mono 16kHz floating-point samples.
    result = pipeline.generate(audio.tolist(), **options)
    segments = []
    for chunk in getattr(result, "chunks", []) or []:
        text = str(getattr(chunk, "text", "")).strip()
        start = float(getattr(chunk, "start_ts", 0.0))
        end = float(getattr(chunk, "end_ts", start))
        if text and end >= start:
            segments.append({"start": start, "end": end, "text": text})

    if not segments:
        # Empty output is treated as a backend failure here. The caller then
        # uses the established CPU Whisper path rather than silently producing
        # an unusable transcript.
        raise RuntimeError("OpenVINO Whisper GPU returned no timestamped segments")
    return segments


def transcribe(audio, language: str | None, model_dir: str | Path | None = None) -> list[dict]:
    """Run OpenVINO Whisper in a clean helper process.

    The Intel PyTorch XPU wheel and OpenVINO GenAI both initialise Level Zero.
    Once PyTorch is imported, OpenVINO's GPU plugin can fail to create its
    context (``Context was not initialized for 0 device``).  The main analyzer
    needs PyTorch for WhisperX and alignment, so isolate the GenAI pipeline in
    a fresh Python process which imports OpenVINO but never imports PyTorch.
    """
    import numpy as np

    directory = Path(model_dir) if model_dir is not None else default_model_dir()
    if not is_model_ready(directory):
        raise RuntimeError(f"OpenVINO Whisper model is missing from {directory}")

    print(f"[uta-studio:LOG] Loading official OpenVINO Whisper on Intel GPU: {directory}", flush=True)
    with tempfile.TemporaryDirectory(prefix="uta-studio_openvino_whisper_") as work_dir:
        audio_path = Path(work_dir) / "audio.npy"
        np.save(audio_path, np.asarray(audio, dtype=np.float32))
        command = [
            sys.executable,
            str(Path(__file__).resolve()),
            "--transcribe",
            "--model-dir",
            str(directory),
            "--audio-npy",
            str(audio_path),
        ]
        if language:
            command.extend(("--language", language))
        result = subprocess.run(command, capture_output=True, text=True)

    if result.returncode:
        detail = (result.stderr or result.stdout).strip()
        raise RuntimeError(f"OpenVINO Whisper helper failed: {detail}")
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"OpenVINO Whisper helper returned invalid JSON: {result.stdout!r}") from exc
    if not isinstance(payload, list):
        raise RuntimeError("OpenVINO Whisper helper returned an invalid transcript")
    return payload


def main() -> None:
    parser = argparse.ArgumentParser(description="Uta Studio OpenVINO Whisper helper")
    parser.add_argument("--download-model", action="store_true")
    parser.add_argument("--transcribe", action="store_true")
    parser.add_argument("--model-dir", required=True)
    parser.add_argument("--audio-npy")
    parser.add_argument("--language")
    args = parser.parse_args()
    if args.download_model:
        download_model(args.model_dir)
        return
    if args.transcribe:
        if not args.audio_npy:
            parser.error("--audio-npy is required with --transcribe")
        import numpy as np

        audio = np.load(args.audio_npy)
        print(json.dumps(_transcribe_in_process(audio, args.language, args.model_dir)), flush=True)
        return
    parser.error("--download-model or --transcribe is required")


if __name__ == "__main__":
    main()
