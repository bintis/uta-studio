"""Centralized GPU lifecycle helpers for the analyzer.

Every model load in the analyzer goes through ``gpu_model`` so its weights are
guaranteed to be evicted from VRAM regardless of how the call exits, and so the
VRAM cost of each phase is logged for visibility.
"""

from __future__ import annotations

import gc
from contextlib import contextmanager

import torch


def _cuda_available() -> bool:
    try:
        return torch.cuda.is_available()
    except Exception:
        return False


def _xpu_available() -> bool:
    try:
        return bool(getattr(torch, "xpu", None) and torch.xpu.is_available())
    except Exception:
        return False


def _xpu_initialized() -> bool:
    """Check existing state without creating a persistent Level Zero context."""
    try:
        xpu = getattr(torch, "xpu", None)
        is_initialized = getattr(xpu, "is_initialized", None)
        return bool(xpu is not None and callable(is_initialized) and is_initialized())
    except Exception:
        return False


def vram_snapshot() -> dict:
    """Return current/peak accelerator memory usage in MiB."""
    if _cuda_available():
        memory = torch.cuda
        backend = "cuda"
    elif _xpu_initialized():
        memory = torch.xpu
        backend = "xpu"
    else:
        return {}
    try:
        memory.synchronize()
    except Exception:
        pass
    mib = 1024 * 1024
    try:
        return {
            "backend": backend,
            "allocated": memory.memory_allocated() // mib,
            "reserved": memory.memory_reserved() // mib,
            "peak_alloc": memory.max_memory_allocated() // mib,
            "peak_reserved": memory.max_memory_reserved() // mib,
        }
    except Exception:
        return {}


def log_vram(tag: str) -> None:
    snap = vram_snapshot()
    if not snap:
        return
    print(
        f"[uta-studio:LOG] [vram:{tag}] backend={snap['backend']} "
        f"alloc={snap['allocated']}MiB reserved={snap['reserved']}MiB "
        f"peak={snap['peak_alloc']}MiB peak_reserved={snap['peak_reserved']}MiB",
        flush=True,
    )


def reset_peak_stats() -> None:
    for available, memory in (
        (_cuda_available, torch.cuda),
        (_xpu_initialized, getattr(torch, "xpu", None)),
    ):
        if memory is None or not available():
            continue
        try:
            memory.reset_peak_memory_stats()
        except Exception:
            pass


def hard_free_gpu(tag: str = "") -> None:
    """Aggressively free GPU memory: gc x2, sync, empty_cache, ipc_collect."""
    for _ in range(2):
        gc.collect()
    if _cuda_available():
        try:
            torch.cuda.synchronize()
        except Exception:
            pass
        try:
            torch.cuda.empty_cache()
        except Exception:
            pass
        try:
            torch.cuda.ipc_collect()
        except Exception:
            pass
    if _xpu_initialized():
        try:
            torch.xpu.synchronize()
            torch.xpu.empty_cache()
        except Exception:
            pass
    if tag:
        log_vram(f"after_free:{tag}")


def move_to_cpu(model, _seen: set[int] | None = None) -> None:
    """Best-effort eviction of model weights from VRAM to host RAM."""
    if model is None:
        return
    if _seen is None:
        _seen = set()
    identity = id(model)
    if identity in _seen:
        return
    _seen.add(identity)
    for attr in (
        "model",
        "model_run",
        "model_run_obj",
        "separator",
        "_separator",
        "model_instance",
    ):
        inner = getattr(model, attr, None)
        if inner is not None and inner is not model:
            try:
                move_to_cpu(inner, _seen)
            except Exception:
                pass
    try:
        to = getattr(model, "to", None)
        if callable(to):
            to("cpu")
            return
    except Exception:
        pass
    try:
        cpu = getattr(model, "cpu", None)
        if callable(cpu):
            cpu()
    except Exception:
        pass


def release(model, name: str = "") -> None:
    """Move ``model`` to CPU, drop the reference, and free GPU memory."""
    try:
        move_to_cpu(model)
    except Exception:
        pass
    del model
    hard_free_gpu(name)


@contextmanager
def gpu_model(name: str):
    """Context manager that guarantees GPU cleanup for the model(s) it holds.

    Usage:
        with gpu_model("demucs") as held:
            model = load_demucs()
            held.append(model)
            ...

    On exit (success or exception), every model in ``held`` is moved to CPU,
    references are dropped, and GPU memory is forcibly reclaimed.
    """
    log_vram(f"before_load:{name}")
    holder: list = []
    try:
        yield holder
    finally:
        for m in holder:
            try:
                move_to_cpu(m)
            except Exception:
                pass
        holder.clear()
        hard_free_gpu(name)


def end_of_song_cleanup() -> None:
    """Best-effort full reset between queued songs.

    Calls into per-backend free hooks (kept for backwards compat) and then runs
    the bulletproof free pass. Safe to call repeatedly.
    """
    try:
        import parakeet
        parakeet.free_models()
    except Exception:
        pass
    hard_free_gpu("end_of_song")
