"""PyTorch / device compatibility helpers for Uta Studio analyzer."""

import os

import torch

from gpu import hard_free_gpu, log_vram, reset_peak_stats, vram_snapshot, gpu_model

_original_torch_load = torch.load


def _patched_torch_load(*args, **kwargs):
    kwargs["weights_only"] = False
    return _original_torch_load(*args, **kwargs)


torch.load = _patched_torch_load


def _default_progress_sink(pct: int, msg: str):
    print(f"[uta-studio:PROGRESS:{pct}] {msg}", flush=True)


_progress_sink = _default_progress_sink


def set_progress_sink(fn):
    global _progress_sink
    _progress_sink = fn or _default_progress_sink


def progress(pct: int, msg: str):
    _progress_sink(pct, msg)


_align_backend = "whisperx"


def set_align_backend(name):
    """Select the forced-alignment backend:

    - ``"whisperx"`` (default): pure-Python CTC Viterbi.
    - ``"ctc"``: torchaudio ``forced_align`` C++/CUDA kernel.
    - ``"qwen"``: Qwen3-ForcedAligner token-classification model. Handled
      directly by the transcribe/align callers (see ``qwen_align``); when a song
      falls outside its support (unsupported language, over-length audio, or any
      failure) the callers fall through to the wav2vec2 path, where ``_run_align``
      treats any non-``"ctc"`` backend as ``whisperx``.
    - ``"mms_karaoke"``: Japanese karaoke-tuned MMS/Wav2Vec2 CTC alignment.
      Handled directly by ``align.align_lyrics`` when known lyrics are present;
      unsupported requests fall through to the WhisperX path.
    """
    global _align_backend
    _align_backend = (
        name
        if name in ("whisperx", "ctc", "qwen", "mms_karaoke")
        else "whisperx"
    )


def get_align_backend() -> str:
    return _align_backend


def detect_device() -> str:
    requested = os.environ.get("UTA_STUDIO_COMPUTE_BACKEND", "cpu").lower()
    if requested == "cuda":
        if torch.cuda.is_available():
            return "cuda"
        raise RuntimeError("NVIDIA CUDA was selected, but PyTorch cannot access a CUDA device. Reinstall the CUDA runtime or choose CPU.")
    if requested == "intel":
        xpu = getattr(torch, "xpu", None)
        if xpu is not None and xpu.is_available():
            return "xpu"
        raise RuntimeError("Intel Arc was selected, but PyTorch cannot access an XPU device. Reinstall the Intel runtime or update the Intel GPU driver.")
    if requested == "mps" and torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def align_device_for(device: str) -> str:
    # WhisperX/CTranslate2 does not yet provide a supported XPU execution
    # path. Demucs can still use XPU while text models safely use CPU.
    return "cpu" if device in ("mps", "xpu") else device


def compute_type_for(device: str) -> str:
    if device != "cuda":
        return "float32"
    major, _ = torch.cuda.get_device_capability()
    if major >= 7:
        return "float16"
    return "int8"


def is_oom(err):
    lower = str(err).lower()
    return "out of memory" in lower or "outofmemoryerror" in lower


def free_gpu():
    """Backwards-compatible alias for :func:`gpu.hard_free_gpu`."""
    hard_free_gpu()


def _run_align(raw_segments, audio, language, device, model_name=None):
    import whisperx
    key_suffix = model_name or language
    with gpu_model(f"wav2vec2:{key_suffix}:{device}") as held:
        if model_name:
            print(
                f"[uta-studio:LOG] Loading align model '{model_name}' for "
                f"language='{language}' on device='{device}'",
                flush=True,
            )
        else:
            print(
                f"[uta-studio:LOG] Loading align model for language='{language}' on device='{device}'",
                flush=True,
            )
        align_model, metadata = whisperx.load_align_model(
            language_code=language, device=device, model_name=model_name,
        )
        held.append(align_model)

        if get_align_backend() == "ctc":
            try:
                import ctc_align
                print(
                    f"[uta-studio:LOG] Aligning with torchaudio forced_align (ctc) on {device}",
                    flush=True,
                )
                return ctc_align.ctc_align(
                    raw_segments, align_model, metadata, audio, device,
                )
            except Exception as e:
                if is_oom(e):
                    # Let align_with_fallback's OOM handling retry (it re-enters
                    # _run_align, keeping the fast ctc path, and finally drops to
                    # CPU) instead of silently switching to the slow whisperx path.
                    raise
                print(
                    f"[uta-studio:LOG] ctc align backend failed ({e}); "
                    f"falling back to whisperx.align",
                    flush=True,
                )

        return whisperx.align(raw_segments, align_model, metadata, audio, device)


def align_with_fallback(raw_segments, audio, language, device, pre_align_cleanup=None, model_name=None):
    """Run whisperx.align with OOM fallback: retry after cleanup, then CPU.

    ``model_name`` overrides the default WhisperX align model for the given
    language (e.g. swap in slplab's hiragana CTC for ``ja``).
    """
    try:
        return _run_align(raw_segments, audio, language, device, model_name=model_name)
    except Exception as e:
        if not is_oom(e):
            raise
        log_vram("oom:align_attempt1")

    if pre_align_cleanup:
        print(
            f"[uta-studio:LOG] Alignment OOM, freeing other models and retrying on {device}",
            flush=True,
        )
        try:
            pre_align_cleanup()
        except Exception:
            pass
        try:
            return _run_align(raw_segments, audio, language, device, model_name=model_name)
        except Exception as e2:
            if not is_oom(e2):
                raise
            log_vram("oom:align_attempt2")

    print("[uta-studio:LOG] Alignment OOM, falling back to CPU", flush=True)
    return _run_align(raw_segments, audio, language, "cpu", model_name=model_name)
