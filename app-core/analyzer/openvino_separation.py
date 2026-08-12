"""Intel OpenVINO stem separation with HTDemucs v4.

Intel's OpenVINO port provides reliable 4-stem separation, with vocals and an
instrumental mix returned to Uta Studio.

It runs on Intel GPU through OpenVINO and falls back to CPU if compilation
fails. It runs in an isolated helper process so PyTorch XPU and OpenVINO do
not contend for the same Level Zero context.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

import numpy as np

INTEL_DEMUCS_REPO = "Intel/demucs-openvino"
DEMUCS_XML = "htdemucs_v4/htdemucs_fwd.xml"
DEMUCS_BIN = "htdemucs_v4/htdemucs_fwd.bin"

DEMUCS_SEGMENT = 343_980  # 7.8 seconds at 44.1 kHz, fixed by Intel's IR.
DEMUCS_HOP = 1024


def _progress(pct: int, message: str) -> None:
    """Report progress without importing PyTorch in the helper process."""
    if os.environ.get("UTA_STUDIO_OPENVINO_HELPER") == "1":
        print(f"[uta-studio:PROGRESS:{pct}] {message}", flush=True)
        return
    try:
        from whisper_compat import progress

        progress(pct, message)
    except ImportError:
        print(f"[uta-studio:PROGRESS:{pct}] {message}", flush=True)


def _ffmpeg() -> str:
    return os.environ.get("FFMPEG_PATH", "ffmpeg")


def _models_root(models_dir: str | Path) -> Path:
    return Path(models_dir).expanduser().resolve()


def demucs_model_path(models_dir: str | Path) -> Path:
    return _models_root(models_dir) / "openvino-demucs" / DEMUCS_XML


def models_ready(models_dir: str | Path) -> bool:
    root = _models_root(models_dir)
    return (
        (root / "openvino-demucs" / DEMUCS_XML).is_file()
        and (root / "openvino-demucs" / DEMUCS_BIN).is_file()
    )


def download_models(models_dir: str | Path) -> None:
    """Download pinned public model artifacts into the selected model cache."""
    from huggingface_hub import hf_hub_download

    root = _models_root(models_dir)
    demucs_dir = root / "openvino-demucs"
    demucs_dir.mkdir(parents=True, exist_ok=True)

    artifacts = (
        (INTEL_DEMUCS_REPO, DEMUCS_XML, demucs_dir),
        (INTEL_DEMUCS_REPO, DEMUCS_BIN, demucs_dir),
    )
    for repo, filename, destination in artifacts:
        print(f"Downloading {repo}/{filename}...", flush=True)
        hf_hub_download(repo_id=repo, filename=filename, local_dir=str(destination))

    if not models_ready(root):
        raise RuntimeError("OpenVINO separation model download completed with missing files")
    print("OpenVINO separation model ready", flush=True)


def _load_audio_44k(path: str, work_dir: str) -> np.ndarray:
    """Decode audio through ffmpeg to a contiguous stereo float32 array."""
    wav = os.path.join(work_dir, "input.wav")
    subprocess.run(
        [_ffmpeg(), "-y", "-i", path, "-ar", "44100", "-ac", "2", "-v", "error", wav],
        check=True,
    )
    import soundfile as sf

    audio, sr = sf.read(wav, dtype="float32", always_2d=True)
    if sr != 44100 or audio.size == 0:
        raise RuntimeError("Could not decode a non-empty 44.1 kHz source")
    return np.ascontiguousarray(audio.T)


def _save_wav(path: str, audio: np.ndarray) -> None:
    import soundfile as sf

    audio = np.asarray(audio, dtype=np.float32)
    peak = float(np.max(np.abs(audio))) if audio.size else 0.0
    if peak > 1.0:
        audio = audio / (peak * 1.01)
    sf.write(path, audio.T, 44100, subtype="PCM_16")


def _gpu_compile_config() -> dict:
    """Return a conservative single-request configuration for Intel Arc.

    Source-separation graphs have much larger temporary tensors than the
    vision models OpenVINO's automatic throughput tuning is designed around.
    Keeping one stream/request and reusing kernels prevents the plugin from
    multiplying that footprint while the low-priority queue keeps long audio
    kernels from starving desktop work.
    """
    return {
        "PERFORMANCE_HINT": "LATENCY",
        "PERFORMANCE_HINT_NUM_REQUESTS": 1,
        "NUM_STREAMS": "1",
        "INFERENCE_PRECISION_HINT": "f16",
        "EXECUTION_MODE_HINT": "PERFORMANCE",
        "GPU_ENABLE_LOOP_UNROLLING": False,
        "GPU_ENABLE_KERNELS_REUSE": True,
        "GPU_QUEUE_PRIORITY": "LOW",
        "GPU_QUEUE_THROTTLE": "LOW",
    }


def _intel_gpu_device(core) -> str:
    """Select the Intel GPU explicitly on mixed Intel/AMD systems."""
    candidates = [device for device in core.available_devices if device.startswith("GPU")]
    for device in candidates:
        try:
            name = str(core.get_property(device, "FULL_DEVICE_NAME"))
        except Exception:
            continue
        normalized = name.lower()
        if "intel" in normalized or "arc" in normalized:
            return device
    raise RuntimeError(
        "OpenVINO could not find an Intel GPU device; available GPU devices: "
        + (", ".join(candidates) or "none")
    )


def _compile(model_path: Path):
    import openvino as ov

    core = ov.Core()
    # Avoid OpenVINO's persistent compiled-model cache here.  With the Intel
    # GPU plugin it can retain a failed context probe and incorrectly hide the
    # GPU from a later clean helper process.
    try:
        gpu_device = _intel_gpu_device(core)
        compiled = core.compile_model(str(model_path), gpu_device, _gpu_compile_config())
        print(
            f"[uta-studio:LOG] OpenVINO separator using Intel {gpu_device}: {model_path.name}",
            flush=True,
        )
        return compiled, "GPU"
    except Exception as exc:
        print(
            f"[uta-studio:LOG] OpenVINO GPU separator failed ({exc}); falling back to OpenVINO CPU",
            flush=True,
        )
        return core.compile_model(str(model_path), "CPU"), "CPU"


def _demucs_spectrogram(mix):
    """Match the OpenVINO HTDemucs export's normalized frequency input."""
    import torch
    import torch.nn.functional as F

    length = mix.shape[-1]
    le = int(np.ceil(length / DEMUCS_HOP))
    pad = DEMUCS_HOP // 2 * 3
    mix_for_spec = F.pad(mix, (pad, pad + le * DEMUCS_HOP - length), mode="reflect")
    flattened = mix_for_spec.reshape(-1, mix_for_spec.shape[-1])
    window = torch.hann_window(4096, device=flattened.device, dtype=flattened.dtype)
    spec = torch.stft(
        flattened,
        n_fft=4096,
        hop_length=DEMUCS_HOP,
        win_length=4096,
        window=window,
        center=True,
        pad_mode="reflect",
        normalized=True,
        onesided=True,
        return_complex=True,
    )
    spec = spec.view(mix.shape[0], mix.shape[1], spec.shape[-2], spec.shape[-1])
    spec = spec[..., :-1, 2 : 2 + le]
    # (B, C, F, T) complex -> (B, C*2, F, T) real/imag, exactly as htdemucs.cpp.
    return torch.view_as_real(spec).permute(0, 1, 4, 2, 3).reshape(
        mix.shape[0], mix.shape[1] * 2, spec.shape[-2], spec.shape[-1]
    )


def _demucs_ispectrogram(mask, length: int):
    """Inverse of ``_demucs_spectrogram``, matching Intel's C++ pipeline."""
    import torch
    import torch.nn.functional as F

    batch, sources, channels, complex_parts, freqs, frames = mask.shape
    if complex_parts != 2:
        raise RuntimeError(f"Expected real/imaginary mask pairs, got {complex_parts} parts")
    complex_mask = torch.view_as_complex(mask.permute(0, 1, 2, 4, 5, 3).contiguous())
    # Match Intel's reference pipeline exactly: restore the Nyquist frequency
    # bin, then restore the two leading and two trailing STFT frames removed by
    # `_demucs_spectrogram`.  Padding only one trailing frame makes the requested
    # output longer than the available Hann-window envelope and fails PyTorch's
    # NOLA check ("window overlap add min").
    complex_mask = F.pad(complex_mask, (0, 0, 0, 1))
    complex_mask = F.pad(complex_mask, (2, 2))
    padded_length = DEMUCS_HOP * int(np.ceil(length / DEMUCS_HOP)) + 2 * (DEMUCS_HOP // 2 * 3)
    flat = complex_mask.reshape(-1, complex_mask.shape[-2], complex_mask.shape[-1])
    window = torch.hann_window(4096, device=flat.device, dtype=torch.float32)
    audio = torch.istft(
        flat,
        n_fft=4096,
        hop_length=DEMUCS_HOP,
        win_length=4096,
        window=window,
        center=True,
        normalized=True,
        onesided=True,
        length=padded_length,
    )
    audio = audio.view(batch, sources, channels, -1)
    pad = DEMUCS_HOP // 2 * 3
    return audio[..., pad : pad + length]


def _demucs_run_chunk(compiled, chunk: np.ndarray) -> np.ndarray:
    """Run one fixed-size HTDemucs OpenVINO chunk and return four stems."""
    import torch

    mix = torch.from_numpy(chunk[None]).to(torch.float32)
    freq = _demucs_spectrogram(mix)
    mean = freq.mean((1, 2, 3), keepdim=True)
    std = freq.std((1, 2, 3), keepdim=True)
    freq = (freq - mean) / (std + 1e-5)
    time_mean = mix.mean((1, 2), keepdim=True)
    time_std = mix.std((1, 2), keepdim=True)
    time = (mix - time_mean) / (time_std + 1e-5)

    result = compiled([freq.numpy(), time.numpy()])
    outputs = list(result.values())
    if len(outputs) != 2:
        raise RuntimeError(f"Unexpected OpenVINO Demucs output count: {len(outputs)}")
    # Intel's export has frequency output (1,16,2048,336) and time output
    # (1,8,343980). Do not rely on output ordering from a particular OV build.
    freq_out, time_out = sorted(outputs, key=lambda value: np.asarray(value).ndim, reverse=True)
    freq_out = torch.from_numpy(np.asarray(freq_out, dtype=np.float32))
    time_out = torch.from_numpy(np.asarray(time_out, dtype=np.float32))
    if freq_out.ndim != 4 or time_out.ndim != 3:
        raise RuntimeError("Unexpected OpenVINO Demucs output shapes")

    # The 16 output channels are 4 sources × 2 stereo channels × real/imag.
    mask = freq_out.view(1, 4, 2, 2, freq_out.shape[-2], freq_out.shape[-1])
    mask = mask * std[:, None] + mean[:, None]
    frequency_audio = _demucs_ispectrogram(mask, DEMUCS_SEGMENT)
    time_audio = time_out.view(1, 4, -1, DEMUCS_SEGMENT)
    time_audio = time_audio * time_std[:, None] + time_mean[:, None]
    return (frequency_audio + time_audio).squeeze(0).numpy()


def _openvino_demucs(audio: np.ndarray, model_path: Path) -> np.ndarray:
    compiled, execution_device = _compile(model_path)
    ref = audio.mean(axis=0)
    ref_std = float(ref.std())
    if ref_std < 1e-8:
        raise RuntimeError("Input audio is silent")
    normalized = (audio - float(ref.mean())) / ref_std
    length = normalized.shape[-1]
    stride = int(DEMUCS_SEGMENT * 0.75)
    starts = list(range(0, length, stride))
    weights = np.minimum(np.arange(1, DEMUCS_SEGMENT + 1), np.arange(DEMUCS_SEGMENT, 0, -1)).astype(np.float32)
    weights /= weights.max()
    out = np.zeros((4, 2, length), dtype=np.float32)
    total = np.zeros(length, dtype=np.float32)

    for index, start in enumerate(starts, 1):
        end = min(start + DEMUCS_SEGMENT, length)
        actual = end - start
        padded = np.zeros((2, DEMUCS_SEGMENT), dtype=np.float32)
        padded[:, :actual] = normalized[:, start:end]
        stems = _demucs_run_chunk(compiled, padded)[..., :actual]
        weight = weights[:actual]
        out[..., start:end] += stems * weight
        total[start:end] += weight
        _progress(
            15 + int(index / len(starts) * 32),
            f"OpenVINO Demucs {execution_device}: chunk {index}/{len(starts)}",
        )

    out /= np.maximum(total, 1e-8)[None, None]
    return out * ref_std + float(ref.mean())


def _separate_openvino_demucs_in_process(
    audio_path: str, work_dir: str, models_dir: str,
) -> tuple[str, str]:
    model = demucs_model_path(models_dir)
    if not model.is_file():
        raise RuntimeError("OpenVINO Demucs model is missing; re-run setup with Intel Arc selected")
    _progress(5, "Loading OpenVINO Demucs for Intel GPU...")
    audio = _load_audio_44k(audio_path, work_dir)
    stems = _openvino_demucs(audio, model)
    vocals_path = os.path.join(work_dir, "vocals.wav")
    instrumental_path = os.path.join(work_dir, "instrumental.wav")
    _save_wav(vocals_path, stems[3])
    _save_wav(instrumental_path, stems[:3].sum(axis=0))
    _progress(50, "OpenVINO Demucs stem separation complete")
    return vocals_path, instrumental_path


def _run_helper(audio_path: str, work_dir: str, models_dir: str) -> tuple[str, str]:
    """Run an OpenVINO separator before any PyTorch import can touch XPU.

    PyTorch's Intel XPU runtime and OpenVINO cannot currently share Level Zero
    GPU contexts reliably in one Python process. The isolated helper prevents
    that collision while keeping stem separation on Arc; spectral pre/post
    processing remains CPU-only.
    """
    from whisper_compat import progress

    progress(5, "Loading OpenVINO Demucs for Intel GPU...")
    process = subprocess.Popen(
        [
            sys.executable,
            str(Path(__file__).resolve()),
            "--separate",
            "demucs",
            "--audio",
            audio_path,
            "--work-dir",
            work_dir,
            "--models-dir",
            models_dir,
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
        env={**os.environ, "UTA_STUDIO_OPENVINO_HELPER": "1"},
    )
    recent: list[str] = []
    assert process.stdout is not None
    for raw_line in process.stdout:
        line = raw_line.rstrip()
        if not line:
            continue
        if line.startswith("[uta-studio:PROGRESS:") and "]" in line:
            header, message = line.split("]", 1)
            try:
                pct = int(header.rsplit(":", 1)[1])
            except ValueError:
                print(line, flush=True)
            else:
                progress(pct, message.strip())
        else:
            print(line, flush=True)
        recent.append(line)
        if len(recent) > 80:
            recent.pop(0)
    returncode = process.wait()
    if returncode:
        detail = "\n".join(recent).strip() or f"helper exited with status {returncode}"
        raise RuntimeError(f"OpenVINO Demucs helper failed: {detail}")
    progress(50, "OpenVINO Demucs stem separation complete")
    return os.path.join(work_dir, "vocals.wav"), os.path.join(work_dir, "instrumental.wav")


def separate_openvino_demucs(audio_path: str, work_dir: str, models_dir: str) -> tuple[str, str]:
    if not demucs_model_path(models_dir).is_file():
        raise RuntimeError("OpenVINO Demucs model is missing; re-run setup with Intel Arc selected")
    return _run_helper(audio_path, work_dir, models_dir)


def main() -> None:
    parser = argparse.ArgumentParser(description="Download the Uta Studio OpenVINO separation model")
    parser.add_argument("--download-models", action="store_true")
    parser.add_argument("--models-dir", required=True)
    parser.add_argument("--separate", choices=("demucs",))
    parser.add_argument("--audio")
    parser.add_argument("--work-dir")
    args = parser.parse_args()
    if args.download_models:
        download_models(args.models_dir)
        return
    if args.separate:
        if not args.audio or not args.work_dir:
            parser.error("--audio and --work-dir are required with --separate")
        _separate_openvino_demucs_in_process(args.audio, args.work_dir, args.models_dir)
        return
    parser.error("--download-models or --separate is required")


if __name__ == "__main__":
    main()
