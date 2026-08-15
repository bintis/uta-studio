"""Parakeet TDT 0.6B v3 ASR backends.

Two interchangeable backends keyed off the runtime device:
  - CUDA       -> NVIDIA NeMo (`nemo_toolkit[asr]`)
  - Intel XPU  -> native OpenVINO encoder + ONNX Runtime CPU decoder
  - cpu / mps  -> ONNX Runtime via `onnx-asr`

Both produce raw segments compatible with ``transcribe.transcribe_vocals``'s
post-processing pipeline (wav2vec2 forced alignment + interpolation), so the
caller can swap engines without further changes downstream.
"""

import os
import subprocess
import json
import sys
import tempfile
from pathlib import Path

import numpy as np

PARAKEET_LANGS = {
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu",
    "it", "lv", "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "uk", "ru",
}

NEMO_MODEL_ID = "nvidia/parakeet-tdt-0.6b-v3"
ONNX_MODEL_ID = "istupakov/parakeet-tdt-0.6b-v3-onnx"
OPENVINO_GPU_DEVICE = os.environ.get("UTA_STUDIO_OPENVINO_GPU_DEVICE", "GPU.0")


class ParakeetEmptyOutputError(RuntimeError):
    """Raised when every Parakeet backend produced no usable words.

    Callers should treat this as a signal to fall back to Whisper for the song.
    """


def _gpu_helpers():
    """Import PyTorch-backed helpers only for non-OpenVINO code paths."""
    from gpu import gpu_model, hard_free_gpu, log_vram
    from whisper_compat import is_oom, progress

    return gpu_model, hard_free_gpu, log_vram, is_oom, progress


def is_supported(lang: str) -> bool:
    if not lang:
        return False
    return lang.lower() in PARAKEET_LANGS


def free_models():
    """No-op shim kept for backwards compatibility.

    Models are no longer cached at module level; each call to :func:`transcribe`
    loads inside a ``gpu_model`` scope that releases on exit. We still expose
    this symbol so existing callers (server.py, transcribe.py) don't break.
    """
    _, hard_free_gpu, _, _, _ = _gpu_helpers()
    hard_free_gpu("parakeet_free_models")


def transcribe(
    vocals_path: str,
    device: str,
    language: str,
    batch_size: int = 8,
    pre_load_cleanup=None,
) -> tuple[list[dict], str]:
    """Run Parakeet ASR over the full vocals file and return word-level data.

    Returns ``(words, backend)`` where ``words`` is a list of
    ``{"word": str, "start": float, "end": float}`` entries with timestamps
    already on the original timeline (Parakeet sees the untrimmed vocals file
    so no offset is needed). The caller is expected to skip wav2vec2 forced
    alignment and build segments directly from these timestamps.

    ``batch_size`` controls NeMo's internal chunk batching (ignored by the
    ONNX backend, which processes the file in a single pass).

    ``pre_load_cleanup`` is a callable that frees other models (Whisper,
    wav2vec2 align, etc.) before each backend load attempt — required so the
    NeMo path on small GPUs doesn't OOM on weights mapping or CUDA stream init.
    """
    if pre_load_cleanup:
        pre_load_cleanup()

    _, _, _, _, progress = _gpu_helpers()
    if device == "cuda":
        words, backend = _transcribe_nemo_with_fallback(vocals_path, batch_size, pre_load_cleanup)
    elif device == "xpu":
        progress(60, "Transcribing with Parakeet v3 (OpenVINO GPU)...")
        words = _transcribe_openvino_helper(vocals_path)
        backend = "onnx-openvino-gpu"
    else:
        progress(60, "Transcribing with Parakeet v3 (onnx)...")
        words = _transcribe_onnx(vocals_path, "CPU")
        backend = "onnx"

    for w in words:
        w["start"] = round(w["start"], 3)
        w["end"] = round(w["end"], 3)

    print(
        f"[uta-studio:LOG] Parakeet ({backend}) produced {len(words)} words for lang='{language}'",
        flush=True,
    )

    if not words:
        raise ParakeetEmptyOutputError(
            f"Parakeet backend '{backend}' produced no words for lang='{language}'"
        )

    return words, backend


def _transcribe_nemo_with_fallback(
    vocals_path: str, batch_size: int, pre_load_cleanup=None,
) -> tuple[list[dict], str]:
    """Try NeMo on CUDA; retry once after cleanup; finally fall back to ONNX on CPU."""
    _, hard_free_gpu, log_vram, is_oom, progress = _gpu_helpers()
    current_batch = max(1, batch_size)
    for attempt in (1, 2):
        progress(60, f"Transcribing with Parakeet v3 (nemo, attempt {attempt}, batch={current_batch})...")
        try:
            return _transcribe_nemo(vocals_path, current_batch), "nemo"
        except Exception as e:
            if not is_oom(e):
                raise
            log_vram(f"oom:parakeet_nemo_attempt{attempt}")
            print(
                f"[uta-studio:LOG] Parakeet NeMo OOM on attempt {attempt} (batch={current_batch}); "
                f"freeing models and retrying",
                flush=True,
            )
            if pre_load_cleanup:
                try:
                    pre_load_cleanup()
                except Exception:
                    pass
            hard_free_gpu(f"parakeet_nemo_oom_attempt{attempt}")
            current_batch = max(1, current_batch // 2)

    print(
        "[uta-studio:LOG] Parakeet NeMo OOM'd twice; falling back to ONNX on CPU",
        flush=True,
    )
    progress(
        60,
        "Transcribing with Parakeet v3 (onnx, CPU fallback)...",
        requested_device="cuda",
        actual_device="cpu",
        fallback_from="cuda",
        fallback_reason="Parakeet NeMo exhausted CUDA memory after two attempts",
    )
    hard_free_gpu("parakeet_nemo_to_onnx_fallback")
    return _transcribe_onnx(vocals_path), "onnx-cpu-fallback"


def _ensure_wav_16k_mono(src_path: str, work_dir: str) -> str:
    """Re-encode ``src_path`` to a 16kHz mono PCM WAV inside ``work_dir``."""
    out = os.path.join(work_dir, "parakeet_input.wav")
    ffmpeg = os.environ.get("FFMPEG_PATH", "ffmpeg")
    subprocess.run(
        [ffmpeg, "-y", "-i", src_path, "-ar", "16000", "-ac", "1", "-v", "error", out],
        check=True,
    )
    return out


def _load_nemo():
    import nemo.collections.asr as nemo_asr

    print(f"[uta-studio:LOG] Loading NeMo model {NEMO_MODEL_ID}", flush=True)
    model = nemo_asr.models.ASRModel.from_pretrained(model_name=NEMO_MODEL_ID)
    model.eval()
    model.cuda()
    return model


def _transcribe_nemo(vocals_path: str, batch_size: int = 8) -> list[dict]:
    gpu_model, _, _, _, _ = _gpu_helpers()
    with gpu_model("parakeet-nemo") as held:
        model = _load_nemo()
        held.append(model)

        with tempfile.TemporaryDirectory(prefix="uta-studio_parakeet_") as work_dir:
            wav_path = _ensure_wav_16k_mono(vocals_path, work_dir)
            outputs = model.transcribe([wav_path], timestamps=True, batch_size=batch_size)

    if not outputs:
        return []
    out = outputs[0]
    timestamp = getattr(out, "timestamp", None) or {}
    word_entries = timestamp.get("word") or []

    words: list[dict] = []
    for entry in word_entries:
        text = (entry.get("word") or entry.get("token") or "").strip()
        start = entry.get("start")
        end = entry.get("end")
        if not text or start is None or end is None:
            continue
        words.append({"word": text, "start": float(start), "end": float(end)})
    return words


def _load_onnx(device_type: str = "CPU"):
    import onnx_asr
    import onnxruntime as ort

    if device_type.startswith("GPU"):
        if "OpenVINOExecutionProvider" not in ort.get_available_providers():
            raise RuntimeError("Intel Arc requires onnxruntime-openvino, but its OpenVINO provider is unavailable")
        # ORT 1.24's OpenVINO graph partitioner loses one encoder output after
        # optimising this model ("Output names mismatch between OpenVINO and
        # ONNX"). Load the decoder on CPU, then replace only the compute-heavy
        # encoder session with native OpenVINO below.
        providers = ["CPUExecutionProvider"]
        label = f"native OpenVINO {device_type} encoder + CPU decoder"
    else:
        providers = ["CPUExecutionProvider"]
        label = "CPU"
    print(f"[uta-studio:LOG] Loading ONNX model {ONNX_MODEL_ID} ({label})", flush=True)
    try:
        model = onnx_asr.load_model(
            ONNX_MODEL_ID, quantization="int8", providers=providers,
        )
    except TypeError:
        try:
            model = onnx_asr.load_model(ONNX_MODEL_ID, providers=providers)
        except TypeError:
            model = onnx_asr.load_model(ONNX_MODEL_ID)

    if device_type.startswith("GPU"):
        _install_native_openvino_encoder(model)
    return model


class _NativeOpenVINOEncoder:
    """Small adapter matching the ``InferenceSession.run`` API onnx-asr uses."""

    def __init__(self, compiled_model):
        self._compiled_model = compiled_model
        self._outputs = {output.get_any_name(): output for output in compiled_model.outputs}

    def run(self, output_names, input_feed, _run_options=None):
        result = self._compiled_model(input_feed)
        return [np.asarray(result[self._outputs[name]]) for name in output_names]


def _install_native_openvino_encoder(model) -> None:
    """Put Parakeet's large encoder on Arc without ORT graph partitioning."""
    import openvino as ov
    from huggingface_hub import snapshot_download
    from openvino_separation import _gpu_compile_config, _intel_gpu_device

    snapshot = Path(
        snapshot_download(
            ONNX_MODEL_ID,
            local_files_only=True,
            allow_patterns=("encoder-model.int8.onnx",),
        )
    )
    encoder_path = snapshot / "encoder-model.int8.onnx"
    core = ov.Core()
    gpu_device = _intel_gpu_device(core)
    encoder = core.read_model(encoder_path)
    compiled = core.compile_model(encoder, gpu_device, _gpu_compile_config())
    model.asr._encoder = _NativeOpenVINOEncoder(compiled)
    print(
        f"[uta-studio:LOG] Parakeet encoder compiled with native OpenVINO on {gpu_device}",
        flush=True,
    )


def _transcribe_onnx(vocals_path: str, device_type: str = "CPU") -> list[dict]:
    gpu_model, _, _, _, _ = _gpu_helpers()
    with gpu_model("parakeet-onnx") as held:
        return _transcribe_onnx_in_process(vocals_path, device_type, held)


def _transcribe_onnx_in_process(
    vocals_path: str, device_type: str = "CPU", held: list | None = None,
) -> list[dict]:
    """Run ONNX ASR without loading any PyTorch helper in an OpenVINO worker."""
    model = _load_onnx(device_type)
    if held is not None:
        held.append(model)

    with tempfile.TemporaryDirectory(prefix="uta-studio_parakeet_") as work_dir:
        wav_path = _ensure_wav_16k_mono(vocals_path, work_dir)
        result = model.with_timestamps().recognize(wav_path)

    return _timestamped_result_to_words(result)


def _timestamped_result_to_words(result) -> list[dict]:
    """Group onnx-asr's timestamped subword tokens into word intervals."""
    tokens = list(getattr(result, "tokens", None) or [])
    timestamps = list(getattr(result, "timestamps", None) or [])
    if not tokens or len(tokens) != len(timestamps):
        return _extract_words_from_onnx_result(result)

    words: list[dict] = []
    current_text = ""
    current_start = 0.0
    for token, timestamp in zip(tokens, timestamps, strict=True):
        token = str(token)
        starts_word = token.startswith(" ")
        if starts_word and current_text.strip():
            words.append(
                {
                    "word": current_text.strip(),
                    "start": current_start,
                    "end": float(timestamp),
                }
            )
            current_text = ""
        if not current_text:
            current_start = float(timestamp)
        current_text += token

    if current_text.strip():
        # Parakeet timestamps identify token starts. Its 10 ms features are
        # subsampled by eight, so one final 80 ms frame is the best end bound.
        words.append(
            {
                "word": current_text.strip(),
                "start": current_start,
                "end": float(timestamps[-1]) + 0.08,
            }
        )
    return words


def _transcribe_openvino_helper(vocals_path: str) -> list[dict]:
    """Run native OpenVINO before PyTorch imports XPU."""
    result = subprocess.run(
        [sys.executable, str(os.path.abspath(__file__)), "--openvino-gpu", "--audio", vocals_path],
        capture_output=True,
        text=True,
    )
    if result.returncode:
        detail = (result.stderr or result.stdout).strip()
        raise RuntimeError(f"Parakeet OpenVINO helper failed: {detail}")
    lines = [line for line in result.stdout.splitlines() if line.strip()]
    try:
        words = json.loads(lines[-1])
    except (IndexError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"Parakeet OpenVINO helper returned invalid JSON: {result.stdout!r}") from exc
    if not isinstance(words, list):
        raise RuntimeError("Parakeet OpenVINO helper returned an invalid transcript")
    return words


def _extract_words_from_onnx_result(result) -> list[dict]:
    """Normalise the various shapes onnx-asr can return into a flat word list."""
    candidates = result if isinstance(result, list) else [result]
    words: list[dict] = []
    for cand in candidates:
        if isinstance(cand, dict):
            ws = (
                cand.get("word_timestamps")
                or cand.get("words")
                or cand.get("timestamps")
                or []
            )
        else:
            ws = (
                getattr(cand, "word_timestamps", None)
                or getattr(cand, "words", None)
                or getattr(cand, "timestamps", None)
                or []
            )
        for w in ws or []:
            if isinstance(w, dict):
                text = (w.get("word") or w.get("text") or w.get("token") or "").strip()
                start = w.get("start")
                end = w.get("end")
            else:
                text = str(
                    getattr(w, "word", "") or getattr(w, "text", "") or getattr(w, "token", "")
                ).strip()
                start = getattr(w, "start", None)
                end = getattr(w, "end", None)
            if not text or start is None or end is None:
                continue
            words.append({"word": text, "start": float(start), "end": float(end)})
    return words


def main() -> None:
    import argparse

    parser = argparse.ArgumentParser(description="Uta Studio Parakeet OpenVINO helper")
    parser.add_argument("--openvino-gpu", action="store_true")
    parser.add_argument("--audio")
    args = parser.parse_args()
    if not args.openvino_gpu:
        parser.error("--openvino-gpu is required")
    if not args.audio:
        parser.error("--audio is required with --openvino-gpu")
    print(json.dumps(_transcribe_onnx_in_process(args.audio, OPENVINO_GPU_DEVICE)), flush=True)


if __name__ == "__main__":
    main()
