"""Bound sustained MDXC XPU work to short, independently owned contexts."""

from __future__ import annotations

import os
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Mapping

from audio_processors.contracts import ProgressSink

MAX_WINDOW_SECONDS = 12
WINDOW_OVERLAP_SECONDS = 2
OUTPUT_SAMPLE_RATE = 44100
OUTPUT_CHANNELS = 2
INTEL_VENDOR_ID = "0x8086"
BATTLEMAGE_DEVICE_IDS = frozenset({"0xe20b", "0xe223"})


@dataclass(frozen=True)
class AudioWindow:
    index: int
    start_frame: int
    end_frame: int

    @property
    def frame_count(self) -> int:
        return self.end_frame - self.start_frame


@dataclass
class _StemMergeState:
    writer: object
    temporary: Path
    destination: Path
    pending: object | None = None


def audio_windows(total_frames: int, sample_rate: int) -> tuple[AudioWindow, ...]:
    if total_frames <= 0:
        raise ValueError("XPU input contains no audio frames")
    if sample_rate <= 0:
        raise ValueError("XPU input has an invalid sample rate")
    window_frames = MAX_WINDOW_SECONDS * sample_rate
    overlap_frames = WINDOW_OVERLAP_SECONDS * sample_rate
    stride_frames = window_frames - overlap_frames
    windows: list[AudioWindow] = []
    start = 0
    while start < total_frames:
        end = min(start + window_frames, total_frames)
        windows.append(AudioWindow(len(windows), start, end))
        if end == total_frames:
            break
        start += stride_frames
    return tuple(windows)


def intel_battlemage_present(drm_root: Path = Path("/sys/class/drm")) -> bool:
    """Detect affected Intel Xe2 cards through sysfs without initializing XPU."""
    try:
        render_nodes = tuple(drm_root.glob("renderD*"))
    except OSError:
        return False
    for render_node in render_nodes:
        try:
            vendor = (render_node / "device" / "vendor").read_text(
                encoding="ascii"
            ).strip().lower()
            device = (render_node / "device" / "device").read_text(
                encoding="ascii"
            ).strip().lower()
        except OSError:
            continue
        if vendor == INTEL_VENDOR_ID and device in BATTLEMAGE_DEVICE_IDS:
            return True
    return False


def run_segmented_mdxc_xpu(
    *,
    request: Mapping[str, object],
    input_path: Path,
    attempt_dir: Path,
    descriptor_names: Mapping[str, str],
    expected_stems: tuple[str, ...],
    run_worker: Callable[[Mapping[str, object]], Mapping[str, object]],
    progress_sink: ProgressSink | None = None,
    force_segmented: bool | None = None,
) -> dict[str, Path]:
    """Run short input directly or merge short, overlapping worker windows."""
    import soundfile as sf

    info = sf.info(str(input_path))
    windows = audio_windows(info.frames, info.samplerate)
    segmented = intel_battlemage_present() if force_segmented is None else force_segmented
    if len(windows) == 1 or not segmented:
        payload = run_worker({**request, "work_dir": str(attempt_dir)})
        return _payload_paths(payload)

    segment_root = attempt_dir / "xpu-windows"
    segment_root.mkdir(parents=True, exist_ok=True)
    states = _open_merge_states(attempt_dir, descriptor_names, expected_stems)
    completed = False
    try:
        with sf.SoundFile(str(input_path), mode="r") as source:
            for position, window in enumerate(windows):
                source.seek(window.start_frame)
                audio = source.read(
                    window.frame_count,
                    dtype="float32",
                    always_2d=True,
                )
                if len(audio) != window.frame_count:
                    raise RuntimeError(
                        f"short read while preparing XPU window {position + 1}/{len(windows)}"
                    )
                window_dir = segment_root / f"window-{position:04d}"
                window_dir.mkdir(parents=True, exist_ok=False)
                window_input = window_dir / "input.wav"
                sf.write(
                    str(window_input),
                    audio,
                    info.samplerate,
                    format="WAV",
                    subtype="FLOAT",
                )

                worker_dir = window_dir / "worker"
                payload = run_worker(
                    {
                        **request,
                        "input_path": str(window_input),
                        "work_dir": str(worker_dir),
                        "allow_missing_stems": True,
                    }
                )
                paths = _payload_paths(payload)
                _merge_window(
                    states=states,
                    paths=paths,
                    expected_stems=expected_stems,
                    window=window,
                    previous=windows[position - 1] if position else None,
                    following=windows[position + 1]
                    if position + 1 < len(windows)
                    else None,
                    input_sample_rate=info.samplerate,
                )
                if progress_sink is not None:
                    progress_sink(
                        8 + round(82 * (position + 1) / len(windows)),
                        f"XPU window {position + 1}/{len(windows)} complete",
                        xpu_window_index=position + 1,
                        xpu_window_count=len(windows),
                        xpu_window_max_seconds=MAX_WINDOW_SECONDS,
                    )

        _publish_merge_states(states)
        completed = True
        return {stem: states[stem].destination for stem in expected_stems}
    finally:
        if not completed:
            _close_merge_states(states)
        if completed:
            shutil.rmtree(segment_root, ignore_errors=True)


def _payload_paths(payload: Mapping[str, object]) -> dict[str, Path]:
    raw = payload.get("stems")
    if not isinstance(raw, Mapping):
        raise RuntimeError("XPU worker returned an invalid stem map")
    return {str(stem): Path(str(path)) for stem, path in raw.items()}


def _open_merge_states(
    attempt_dir: Path,
    descriptor_names: Mapping[str, str],
    expected_stems: tuple[str, ...],
) -> dict[str, _StemMergeState]:
    import soundfile as sf

    states: dict[str, _StemMergeState] = {}
    try:
        for stem in expected_stems:
            token = descriptor_names[stem]
            destination = attempt_dir / f"{token}.wav"
            temporary = attempt_dir / f".{token}.segmented.tmp"
            writer = sf.SoundFile(
                str(temporary),
                mode="w",
                samplerate=OUTPUT_SAMPLE_RATE,
                channels=OUTPUT_CHANNELS,
                format="WAV",
                subtype="FLOAT",
            )
            states[stem] = _StemMergeState(writer, temporary, destination)
        return states
    except BaseException:
        _close_merge_states(states)
        raise


def _merge_window(
    *,
    states: Mapping[str, _StemMergeState],
    paths: Mapping[str, Path],
    expected_stems: tuple[str, ...],
    window: AudioWindow,
    previous: AudioWindow | None,
    following: AudioWindow | None,
    input_sample_rate: int,
) -> None:
    import numpy as np
    import soundfile as sf

    expected_frames = _resampled_frames(window.frame_count, input_sample_rate)
    incoming_overlap = (
        _resampled_frames(previous.end_frame - window.start_frame, input_sample_rate)
        if previous is not None
        else 0
    )
    outgoing_overlap = (
        _resampled_frames(window.end_frame - following.start_frame, input_sample_rate)
        if following is not None
        else 0
    )

    for stem in expected_stems:
        path = paths.get(stem)
        if path is None:
            data = np.zeros((expected_frames, OUTPUT_CHANNELS), dtype=np.float32)
        else:
            data, sample_rate = sf.read(str(path), dtype="float32", always_2d=True)
            if sample_rate != OUTPUT_SAMPLE_RATE or data.shape[1] != OUTPUT_CHANNELS:
                raise RuntimeError(
                    f"XPU window {window.index + 1} returned {sample_rate} Hz/"
                    f"{data.shape[1]} channels for {stem}"
                )
            data = _fit_window_length(data, expected_frames, stem, window.index)

        state = states[stem]
        if previous is None:
            merged = data
        else:
            pending = state.pending
            if pending is None or len(pending) != incoming_overlap:
                raise RuntimeError(f"XPU overlap state is inconsistent for {stem}")
            if incoming_overlap:
                fade = np.linspace(
                    0.0,
                    1.0,
                    incoming_overlap,
                    endpoint=False,
                    dtype=np.float32,
                )[:, None]
                crossfade = pending * (1.0 - fade) + data[:incoming_overlap] * fade
                merged = np.concatenate((crossfade, data[incoming_overlap:]), axis=0)
            else:
                merged = data

        if outgoing_overlap:
            if outgoing_overlap >= len(merged):
                raise RuntimeError(f"XPU window overlap consumed all frames for {stem}")
            state.writer.write(merged[:-outgoing_overlap])
            state.pending = merged[-outgoing_overlap:].copy()
        else:
            state.writer.write(merged)
            state.pending = None


def _fit_window_length(data, expected_frames: int, stem: str, index: int):
    import numpy as np

    difference = len(data) - expected_frames
    if difference == 0:
        return data
    if difference > 0 and difference <= 2:
        return data[:expected_frames]
    if difference < 0 and difference >= -2:
        padding = np.zeros((-difference, data.shape[1]), dtype=data.dtype)
        return np.concatenate((data, padding), axis=0)
    raise RuntimeError(
        f"XPU window {index + 1} returned {len(data)} frames for {stem}; "
        f"expected {expected_frames}"
    )


def _resampled_frames(input_frames: int, input_sample_rate: int) -> int:
    return round(input_frames * OUTPUT_SAMPLE_RATE / input_sample_rate)


def _publish_merge_states(states: Mapping[str, _StemMergeState]) -> None:
    for state in states.values():
        state.writer.flush()
        state.writer.close()
        with state.temporary.open("rb") as handle:
            os.fsync(handle.fileno())
        os.replace(state.temporary, state.destination)
    directory_fd = os.open(
        next(iter(states.values())).destination.parent,
        os.O_RDONLY | getattr(os, "O_DIRECTORY", 0),
    )
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)


def _close_merge_states(states: Mapping[str, _StemMergeState]) -> None:
    for state in states.values():
        try:
            state.writer.close()
        except BaseException:
            pass
        try:
            state.temporary.unlink()
        except FileNotFoundError:
            pass
