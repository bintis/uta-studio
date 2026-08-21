"""Stem separation: lead vocals + instrumental."""

import os
import subprocess

import numpy as np
import soundfile as sf
import torch

from gpu import gpu_model
from whisper_compat import progress_node

def _ensure_wav(audio_path: str, work_dir: str) -> str:
    """Convert input audio to WAV so plain `soundfile` can decode it.

    We deliberately avoid `torchaudio.load`/`torchaudio.save` here: in
    torchaudio >= 2.9 those go through `torchcodec`, which `dlopen`s the
    FFmpeg shared libraries (`libavcodec.so.X`, ...). Our vendor dir only
    ships a static `ffmpeg` binary, and target machines aren't guaranteed to
    have the FFmpeg shared libs installed, so torchcodec fails to load.
    """
    if audio_path.lower().endswith(".wav"):
        return audio_path
    wav_path = os.path.join(work_dir, "input.wav")
    ffmpeg = os.environ.get("FFMPEG_PATH", "ffmpeg")
    subprocess.run(
        [ffmpeg, "-y", "-i", audio_path, "-ar", "44100", "-ac", "2", "-v", "error", wav_path],
        check=True,
    )
    return wav_path


def _load_wav_as_tensor(path: str) -> tuple[torch.Tensor, int]:
    """Load a WAV file as a (channels, frames) float32 tensor via soundfile."""
    data, sr = sf.read(path, dtype="float32", always_2d=True)
    tensor = torch.from_numpy(np.ascontiguousarray(data.T))
    return tensor, sr


def _save_wav_pcm16(wav: torch.Tensor, path: str, samplerate: int) -> None:
    """Save a (channels, frames) tensor as 16-bit PCM WAV with rescale clipping."""
    wav = wav.detach().cpu().to(torch.float32)
    peak = float(wav.abs().max().item())
    if peak > 1.0:
        wav = wav / (1.01 * peak)
    sf.write(path, wav.numpy().T, samplerate, subtype="PCM_16")


def separate_stems(
    audio_path: str,
    work_dir: str,
    device: str,
    *,
    shifts: int = 1,
    overlap: float = 0.25,
) -> tuple[str, str]:
    """Run Demucs to separate vocals and instrumental stems.

    Returns (vocals_path, instrumental_path).
    """
    from demucs.apply import apply_model
    from demucs.pretrained import get_model

    vocals_path = os.path.join(work_dir, "vocals.wav")
    instrumental_path = os.path.join(work_dir, "instrumental.wav")
    actual_device = torch.device(device if device != "mps" else "cpu")

    with gpu_model("demucs") as held:
        progress_node(
            "stems.multistem", "node_started",
            5,
            "Loading Demucs model...",
            requested_device=device,
            actual_device=str(actual_device),
            fallback_from=device if str(actual_device) != device else None,
            fallback_reason="Demucs does not use the selected accelerator on this platform"
            if str(actual_device) != device else None,
        )
        model = get_model("htdemucs")
        held.append(model)
        model.to(actual_device)

        progress_node(
            "stems.multistem", "node_progress", 10, "Loading audio file...",
            node_progress_pct=10,
        )
        load_path = _ensure_wav(audio_path, work_dir)
        wav, sr = _load_wav_as_tensor(load_path)
        wav = wav.to(actual_device)

        ref = wav.mean(0)
        wav_centered = wav - ref.mean()
        wav_scaled = wav_centered / ref.abs().max().clamp(min=1e-8)

        progress_node(
            "stems.multistem", "node_progress", 15,
            f"Separating vocals (shifts={shifts}, overlap={overlap:.2f})...",
            node_progress_pct=20,
        )
        sources = apply_model(
            model, wav_scaled[None], device=actual_device, shifts=shifts, overlap=overlap,
        )[0]

        source_names = model.sources
        vocals_idx = source_names.index("vocals")

        vocals = sources[vocals_idx] * ref.abs().max() + ref.mean()
        instrumental = wav - vocals

        progress_node(
            "stems.multistem", "node_progress", 45, "Saving separated stems...",
            node_progress_pct=90,
        )
        vocals_cpu = vocals.detach().cpu()
        instrumental_cpu = instrumental.detach().cpu()

        del wav, sources, wav_centered, wav_scaled, ref, vocals, instrumental

        _save_wav_pcm16(vocals_cpu, vocals_path, sr)
        _save_wav_pcm16(instrumental_cpu, instrumental_path, sr)
        del vocals_cpu, instrumental_cpu

    progress_node(
        "stems.multistem", "node_progress", 50, "Stem separation compute complete",
        node_progress_pct=98,
    )
    return vocals_path, instrumental_path


def separate_stems_openvino_demucs(audio_path: str, work_dir: str, models_dir: str) -> tuple[str, str]:
    from openvino_separation import separate_openvino_demucs

    return separate_openvino_demucs(audio_path, work_dir, models_dir)
